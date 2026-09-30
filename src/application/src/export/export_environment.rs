mod inventory;
use super::ListExportFiles;
use super::ValidateExportDestination;
use super::WriteExport;
use crate::ErrorMarker;
use crate::export::CompletedExport;
use crate::export::ExportFile;
use crate::export::RetainedExport;
use crate::ports::DiscardStagedProfile;
use crate::ports::LoadOrderTarget;
use crate::ports::PrepareEnvironmentPlan;
use crate::ports::ProfilePurpose;
use crate::ports::ProjectProfile;
use crate::ports::RetainedProfile;
use crate::ports::SetLoadOrderTimes;
use crate::ports::StageProfile;
use crate::preparation::PluginWarning;
use crate::preparation::PreparedEnvironment;
use crate::preparation::prepare_environment;
use inventory::plan_inventory;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub struct ExportEnvironmentDependencies {
	pub validate_export_destination: ValidateExportDestination,
	pub prepare_environment_plan: PrepareEnvironmentPlan,
	pub project_profile: ProjectProfile,
	pub stage_profile: StageProfile,
	pub list_export_files: ListExportFiles,
	pub write_export: WriteExport,
	pub set_load_order_times: SetLoadOrderTimes,
	pub discard_staged_profile: DiscardStagedProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEnvironmentOutput {
	pub files: Vec<ExportFile>,
	pub total_bytes: u64,
	pub published: bool,
	pub warnings: Vec<PluginWarning>,
}

#[derive(Debug, Clone, Copy)]
pub struct ExportEnvironmentError;
impl fmt::Display for ExportEnvironmentError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to export environment")
	}
}

/// Copies what the game sees under exec into a new standalone folder.
///
/// Export shares preparation, projection, profile staging, and load-order times
/// with exec. It stages the derived profile INIs in the environment's `temp`
/// directory, copies them into the output, gives the output plugins and archives
/// their load-order times, and then removes the stage on every path, including
/// `dry_run` and failures.
///
/// # Errors
///
/// Returns [`ExportEnvironmentError`] with the failing step's marker. A failed
/// write or load-order step also carries `RetainedExport`, and a stage that cannot be removed
/// carries [`RetainedProfile`]. If only the removal failed after a write, the
/// error also carries [`CompletedExport`].
#[tracing::instrument(skip_all)]
pub async fn export_environment(
	dependencies: ExportEnvironmentDependencies,
	output: PathBuf,
	include_saves: bool,
	dry_run: bool,
	cancellation: CancellationToken,
) -> Result<ExportEnvironmentOutput, ExportEnvironmentError> {
	dependencies
		.validate_export_destination
		.call((output.clone(),))
		.await
		.context(ExportEnvironmentError)?;

	let PreparedEnvironment { plan, warnings } = prepare_environment(
		&dependencies.prepare_environment_plan,
		&dependencies.project_profile,
		cancellation.clone(),
	)
	.await
	.context(ExportEnvironmentError)?;

	let staged = dependencies
		.stage_profile
		.call((&plan, ProfilePurpose::Export, cancellation.clone()))
		.await
		.context(ExportEnvironmentError)?;
	let retained = RetainedProfile {
		path: staged.directory.clone(),
	};

	let exported = async {
		let listing = dependencies
			.list_export_files
			.call((&plan, &staged, include_saves, cancellation.clone()))
			.await?;

		let (files, total_bytes) = plan_inventory(listing.files)?;

		if !dry_run {
			dependencies
				.write_export
				.call((listing.sources, files.clone(), output.clone(), cancellation.clone()))
				.await?;

			// The copies keep whatever time the copy gives them, so only the plugins
			// and archives get times, in load order.
			dependencies
				.set_load_order_times
				.call((&plan, LoadOrderTarget::Export(output.clone()), cancellation))
				.await
				.map_err(|mut error| {
					error.current_context_mut().set_phase_if_missing("load_order");
					error.children_mut()
						.push(report!(RetainedExport { path: output.clone() })
							.into_dynamic()
							.into_cloneable());
					error
				})?;
		}

		Ok::<_, Report<ErrorMarker>>((files, total_bytes))
	}
	.await;

	// The stage holds only derived copies, so it is removed on every path, also
	// after cancellation. Only a stage that cannot be removed is reported, and a
	// complete output is marked as complete.
	let discarded = dependencies.discard_staged_profile.call((staged,)).await;
	let exported = match (exported, discarded) {
		(exported, Ok(())) => exported,
		(Ok(_), Err(mut error)) => {
			if !dry_run {
				error.children_mut().push(report!(CompletedExport { path: output })
					.into_dynamic()
					.into_cloneable());
			}
			error.children_mut()
				.push(report!(retained).into_dynamic().into_cloneable());
			Err(error)
		}
		(Err(mut error), Err(discard_error)) => {
			error.children_mut().push(discard_error.into_dynamic().into_cloneable());
			error.children_mut()
				.push(report!(retained).into_dynamic().into_cloneable());
			Err(error)
		}
	};

	let (files, total_bytes) = exported.context(ExportEnvironmentError)?;

	Ok(ExportEnvironmentOutput {
		files,
		total_bytes,
		published: !dry_run,
		warnings,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::export::ExportListing;
	use crate::export::ExportProvider;
	use crate::export::ExportSources;
	use crate::ports::AdapterState;
	use crate::ports::EnvironmentPlan;
	use crate::ports::PortFuture;
	use crate::ports::ProfileProjection;
	use crate::ports::ProfileWarning;
	use crate::ports::StagedProfile;
	use domain::DataRelativePath;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ProviderIdentity;
	use std::future::ready;
	use std::sync::Arc;
	use std::sync::Mutex;

	type Steps = Arc<Mutex<Vec<String>>>;

	#[derive(Clone)]
	struct Scenario {
		files: Vec<ExportFile>,
		destination_valid: bool,
		write_fails: bool,
		load_order_fails: bool,
		discard_fails: bool,
	}

	fn record(steps: &Steps, step: impl Into<String>) {
		if let Ok(mut steps) = steps.lock() {
			steps.push(step.into());
		}
	}

	fn recorded(steps: &Steps) -> Vec<String> {
		steps.lock().map(|steps| steps.clone()).unwrap_or_default()
	}

	fn complete<T: Send + 'static>(result: Result<T, ErrorMarker>) -> PortFuture<T> {
		Box::pin(ready(result))
	}

	fn entry(id: usize, path: &str, provider: ExportProvider) -> Result<ExportFile, ErrorMarker> {
		Ok(ExportFile {
			source_id: id,
			path: DataRelativePath::new(path.to_owned()).context(ErrorMarker::invalid_data_path())?,
			provider,
			bytes: 4,
		})
	}

	fn scenario() -> Result<Scenario, ErrorMarker> {
		Ok(Scenario {
			files: vec![
				entry(0, "Data/Base.esm", ExportProvider::Data(ProviderIdentity::SteamData))?,
				entry(
					1,
					"Data/Textures/Low.dds",
					ExportProvider::Data(ProviderIdentity::DataMod {
						mod_name: ModName::new("Mod".to_owned())
							.context(ErrorMarker::invalid_mod_name())?,
						priority: ModPriority::new(0),
					}),
				)?,
				entry(
					2,
					"Data/TEXTURES/High.dds",
					ExportProvider::Data(ProviderIdentity::Overwrite),
				)?,
				entry(3, "profile/Fallout.ini", ExportProvider::Profile)?,
			],
			destination_valid: true,
			write_fails: false,
			load_order_fails: false,
			discard_fails: false,
		})
	}

	fn fake_dependencies(scenario: Scenario) -> (ExportEnvironmentDependencies, Steps) {
		let steps = Steps::default();
		let step = |name: &'static str| {
			let steps = steps.clone();
			move || record(&steps, name)
		};
		let validated = step("validate");
		let prepared = step("prepare");
		let projected = step("project");
		let staged = step("stage");
		let discarded = step("discard");
		let listed = steps.clone();
		let written = steps.clone();
		let timed = steps.clone();
		let Scenario {
			files,
			destination_valid,
			write_fails,
			load_order_fails,
			discard_fails,
		} = scenario;

		let dependencies = ExportEnvironmentDependencies {
			validate_export_destination: Arc::new(move |output| {
				assert_eq!(output, PathBuf::from("/output"));
				validated();
				if !destination_valid {
					return complete(Err(report!(ErrorMarker::environment_already_initialized())));
				}
				complete(Ok(()))
			}),
			prepare_environment_plan: Arc::new(move |_| {
				prepared();
				complete(Ok(EnvironmentPlan {
					providers: Vec::new(),
					state: AdapterState::new("plan"),
				}))
			}),
			project_profile: Arc::new(move |_: &EnvironmentPlan| {
				projected();
				complete(Ok(ProfileProjection {
					warnings: vec![ProfileWarning::Unlisted {
						plugin: "Unlisted.esp".into(),
					}],
				}))
			}),
			stage_profile: Arc::new(move |_: &EnvironmentPlan, purpose, _| {
				assert_eq!(purpose, ProfilePurpose::Export);
				staged();
				complete(Ok(StagedProfile {
					directory: PathBuf::from("stage"),
					state: AdapterState::new("stage"),
				}))
			}),
			list_export_files: Arc::new(move |_: &EnvironmentPlan, staged: &StagedProfile, saves, _| {
				assert_eq!(staged.directory, PathBuf::from("stage"));
				record(&listed, format!("list:saves={saves}"));
				complete(Ok(ExportListing {
					files: files.clone(),
					sources: ExportSources(AdapterState::new("sources")),
				}))
			}),
			write_export: Arc::new(move |sources: ExportSources, files: Vec<ExportFile>, output, _| {
				assert_eq!(sources.0.downcast::<&str>(), Some("sources"));
				assert_eq!(output, PathBuf::from("/output"));
				record(&written, format!("write:{}", files.len()));
				if write_fails {
					return complete(Err(report!(ErrorMarker::io_failure())));
				}
				complete(Ok(()))
			}),
			set_load_order_times: Arc::new(move |plan: &EnvironmentPlan, target, _| {
				assert_eq!(plan.state.downcast_ref::<&str>(), Some(&"plan"));
				assert_eq!(target, LoadOrderTarget::Export(PathBuf::from("/output")));
				record(&timed, "load_order");
				if load_order_fails {
					return complete(Err(report!(ErrorMarker::io_failure())));
				}
				complete(Ok(()))
			}),
			discard_staged_profile: Arc::new(move |staged: StagedProfile| {
				assert_eq!(staged.state.downcast::<&str>(), Some("stage"));
				discarded();
				if discard_fails {
					return complete(Err(report!(ErrorMarker::io_failure())));
				}
				complete(Ok(()))
			}),
		};

		(dependencies, steps)
	}

	async fn run(
		dependencies: ExportEnvironmentDependencies,
		dry_run: bool,
	) -> Result<ExportEnvironmentOutput, ExportEnvironmentError> {
		export_environment(
			dependencies,
			PathBuf::from("/output"),
			true,
			dry_run,
			CancellationToken::new(),
		)
		.await
	}

	fn retained_profiles(error: &Report<ExportEnvironmentError>) -> Vec<PathBuf> {
		error.iter_reports()
			.filter_map(|cause| cause.downcast_current_context::<RetainedProfile>())
			.map(|retained| retained.path.clone())
			.collect()
	}

	#[tokio::test]
	async fn dry_run_lists_the_shared_preparation_and_removes_the_stage_without_writing() -> Result<(), ErrorMarker>
	{
		let (dependencies, steps) = fake_dependencies(scenario()?);

		let output = run(dependencies, true).await.context(ErrorMarker::io_failure())?;

		assert_eq!(
			recorded(&steps),
			["validate", "prepare", "project", "stage", "list:saves=true", "discard"]
		);
		assert!(!output.published);
		assert_eq!(
			output.warnings,
			[PluginWarning::UnlistedPlugin {
				name: "Unlisted.esp".into()
			}]
		);
		assert_eq!(output.total_bytes, 12);
		assert!(output
			.files
			.iter()
			.any(|file| file.path.as_str() == "Data/TEXTURES/Low.dds"));
		assert!(!output.files.iter().any(|file| file.path.as_str().contains("Base")));
		Ok(())
	}

	#[tokio::test]
	async fn export_writes_the_planned_files_and_sets_load_order_times_before_removing_the_stage()
	-> Result<(), ErrorMarker> {
		let (dependencies, steps) = fake_dependencies(scenario()?);

		let output = run(dependencies, false).await.context(ErrorMarker::io_failure())?;

		assert!(output.published);
		assert_eq!(
			recorded(&steps),
			[
				"validate",
				"prepare",
				"project",
				"stage",
				"list:saves=true",
				"write:3",
				"load_order",
				"discard"
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn an_unusable_destination_stops_before_preparation() -> Result<(), ErrorMarker> {
		let mut scenario = scenario()?;
		scenario.destination_valid = false;
		let (dependencies, steps) = fake_dependencies(scenario);

		assert!(run(dependencies, false).await.is_err());

		assert_eq!(recorded(&steps), ["validate"]);
		Ok(())
	}

	#[tokio::test]
	async fn structural_conflicts_fail_before_writing_and_still_remove_the_stage() -> Result<(), ErrorMarker> {
		let mut scenario = scenario()?;
		scenario.files = vec![
			entry(0, "Data/Meshes", ExportProvider::Data(ProviderIdentity::Overwrite))?,
			entry(
				1,
				"Data/meshes/file.nif",
				ExportProvider::Data(ProviderIdentity::Overwrite),
			)?,
		];
		let (dependencies, steps) = fake_dependencies(scenario);

		let Err(error) = run(dependencies, false).await else {
			return Err(report!(ErrorMarker::io_failure()));
		};

		assert!(retained_profiles(&error).is_empty());
		assert_eq!(recorded(&steps).last().map(String::as_str), Some("discard"));
		assert!(!recorded(&steps).iter().any(|step| step.starts_with("write")));
		Ok(())
	}

	#[tokio::test]
	async fn a_stage_that_cannot_be_removed_is_reported_as_retained() -> Result<(), ErrorMarker> {
		for write_fails in [false, true] {
			let mut scenario = scenario()?;
			scenario.write_fails = write_fails;
			scenario.discard_fails = true;
			let (dependencies, _) = fake_dependencies(scenario);

			let Err(error) = run(dependencies, false).await else {
				return Err(report!(ErrorMarker::io_failure()));
			};

			assert_eq!(retained_profiles(&error), [PathBuf::from("stage")]);
			let completed = error
				.iter_reports()
				.any(|cause| cause.downcast_current_context::<CompletedExport>().is_some());
			assert_eq!(completed, !write_fails);
		}
		Ok(())
	}

	#[tokio::test]
	async fn a_failed_load_order_step_keeps_the_output_and_still_removes_the_stage() -> Result<(), ErrorMarker> {
		let mut scenario = scenario()?;
		scenario.load_order_fails = true;
		let (dependencies, steps) = fake_dependencies(scenario);

		let Err(error) = run(dependencies, false).await else {
			return Err(report!(ErrorMarker::io_failure()));
		};

		let marker = error
			.iter_reports()
			.find_map(|cause| cause.downcast_current_context::<ErrorMarker>());
		assert_eq!(marker.and_then(ErrorMarker::phase), Some("load_order"));
		let retained = error
			.iter_reports()
			.find_map(|cause| cause.downcast_current_context::<RetainedExport>());
		assert_eq!(
			retained.map(|retained| retained.path.clone()),
			Some(PathBuf::from("/output"))
		);
		assert!(retained_profiles(&error).is_empty());
		assert_eq!(recorded(&steps).last().map(String::as_str), Some("discard"));
		Ok(())
	}
}
