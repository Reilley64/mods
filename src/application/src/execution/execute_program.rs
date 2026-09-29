use crate::ErrorMarker;
use crate::execution::ExecutionWarning;
use crate::execution::RetainedExecutionInis;
use crate::ports::CheckProfileState;
use crate::ports::CreateVirtualFileSystem;
use crate::ports::FinishProgramOutput;
use crate::ports::LaunchProgram;
use crate::ports::PrepareLaunchPlan;
use crate::ports::PreserveExecutionProfile;
use crate::ports::ProfileWarning;
use crate::ports::ProgressEvent;
use crate::ports::ProjectExecutionProfile;
use crate::ports::ReportProgress;
use crate::ports::ResolveLaunchTarget;
use crate::ports::StageExecutionProfile;
use crate::ports::SuperviseProgram;
use domain::GameBinding;
use domain::OutputTarget;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::ProviderIdentity;
use domain::WorkingDirectory;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ExecuteProgramDependencies {
	pub report_progress: Option<ReportProgress>,
	pub resolve_launch_target: ResolveLaunchTarget,
	pub prepare_launch_plan: PrepareLaunchPlan,
	pub project_execution_profile: ProjectExecutionProfile,
	pub stage_execution_profile: StageExecutionProfile,
	pub create_virtual_file_system: CreateVirtualFileSystem,
	pub launch_program: LaunchProgram,
	pub supervise_program: SuperviseProgram,
	pub preserve_execution_profile: PreserveExecutionProfile,
	pub finish_program_output: FinishProgramOutput,
	pub check_profile_state: CheckProfileState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteProgramOutput {
	pub status: ProcessStatus,
	pub warnings: Vec<ExecutionWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecuteProgramError;
impl fmt::Display for ExecuteProgramError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to execute program")
	}
}

/// Runs one managed program inside the virtual file system.
///
/// Without `working_directory`, the child starts in the bound game directory.
/// Program lookup does not use the working directory.
///
/// Handles pass between steps in order, so native calls never overlap. Exec
/// composition also runs this use case on one dedicated thread.
///
/// # Errors
///
/// Returns [`ExecuteProgramError`] with the failing step's marker. After the
/// profile is staged, every failure before successful preservation also carries
/// [`RetainedExecutionInis`].
#[tracing::instrument(skip_all)]
pub async fn execute_program(
	dependencies: ExecuteProgramDependencies,
	game_binding: GameBinding,
	output_target: OutputTarget,
	working_directory: Option<WorkingDirectory>,
	program: Program,
	arguments: Vec<ProgramArgument>,
	cancellation: CancellationToken,
) -> Result<ExecuteProgramOutput, ExecuteProgramError> {
	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::PreparingExecution,)).await;
	}

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(ExecuteProgramError));
	}

	// The game resolves `Data\` and its script-extender loaders relative to its
	// working directory, so a child without `--cwd` starts in the game directory.
	let working_directory = if let Some(working_directory) = working_directory {
		working_directory
	} else {
		WorkingDirectory::new(game_binding.game_directory().as_path().to_owned())
			.context(ErrorMarker::invalid_working_directory())
			.context(ExecuteProgramError)?
	};

	let target = dependencies
		.resolve_launch_target
		.call((program, arguments, working_directory))
		.await
		.context(ExecuteProgramError)?;

	let plan = dependencies
		.prepare_launch_plan
		.call((cancellation.clone(),))
		.await
		.context(ExecuteProgramError)?;

	let output_mod = if let OutputTarget::DataMod(name) = output_target {
		let Some(provider) = plan.providers.iter().find(
			|provider| matches!(&provider.identity, ProviderIdentity::DataMod { mod_name, .. } if *mod_name == name),
		) else {
			return Err(report!(ErrorMarker::output_target_not_found().with_mod_name(name))
				.context(ExecuteProgramError));
		};

		if !provider.enabled {
			return Err(report!(ErrorMarker::output_target_disabled().with_mod_name(name))
				.context(ExecuteProgramError));
		}
		Some(name)
	} else {
		None
	};

	let projection = dependencies
		.project_execution_profile
		.call((&plan,))
		.await
		.context(ExecuteProgramError)?;

	let mut warnings: Vec<_> = projection
		.warnings
		.into_iter()
		.map(|warning| match warning {
			ProfileWarning::LoadOrderNotEnforced => ExecutionWarning::LoadOrderNotEnforced,
			ProfileWarning::Unavailable { file, plugin } if file.eq_ignore_ascii_case("plugins.txt") => {
				ExecutionWarning::StalePluginEntry { name: plugin }
			}
			ProfileWarning::Unavailable { plugin, .. } => {
				ExecutionWarning::StaleLoadOrderEntry { name: plugin }
			}
			ProfileWarning::Duplicate { file, plugin } => {
				ExecutionWarning::DuplicatePluginEntry { file, name: plugin }
			}
			ProfileWarning::Unlisted { plugin } => ExecutionWarning::UnlistedPlugin { name: plugin },
		})
		.collect();

	let staged = dependencies
		.stage_execution_profile
		.call((&plan, cancellation.clone()))
		.await
		.context(ExecuteProgramError)?;
	let retained = RetainedExecutionInis {
		path: staged.directory.clone(),
	};

	// The staged profile holds the child's INI edits. Until preservation succeeds,
	// every failure keeps it and reports its path for manual recovery.
	let supervision = async {
		let file_system = dependencies
			.create_virtual_file_system
			.call((plan, projection.profile, &staged, output_mod, cancellation.clone()))
			.await?;

		if cancellation.is_cancelled() {
			drop(file_system);
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let program = dependencies.launch_program.call((file_system, target)).await?;

		if let Some(progress) = &dependencies.report_progress {
			progress.call((ProgressEvent::ExecutionPrepared,)).await;
		}

		let supervision = dependencies
			.supervise_program
			.call((program, cancellation.clone()))
			.await?;

		if !supervision.job_drained.as_ref().is_ok_and(|drained| *drained) {
			let mut failure = report!(retained.clone())
				.context(ErrorMarker::execution_supervision_failed().with_phase("profile_retained"));
			if let Err(error) = supervision.job_drained {
				failure.children_mut().push(error.into_cloneable());
			}
			if let Err(error) = supervision.exit {
				failure.children_mut().push(error.into_dynamic().into_cloneable());
			}
			return Err(failure);
		}

		dependencies.preserve_execution_profile.call((staged,)).await?;

		Ok(supervision)
	}
	.await
	.map_err(|mut error| {
		error.children_mut()
			.push(report!(retained).into_dynamic().into_cloneable());
		error
	})
	.context(ExecuteProgramError)?;

	dependencies
		.finish_program_output
		.call((supervision.output,))
		.await
		.context(ExecuteProgramError)?;

	let exit = supervision.exit.context(ExecuteProgramError)?;

	// Deliberate exception to passing the caller's token: the post-run check gets
	// an unlinked token so it still runs after the caller cancels. It only adds a
	// warning about the retained Profile State.
	let profile_state = dependencies.check_profile_state.call((CancellationToken::new(),)).await;
	if profile_state.is_err() {
		warnings.push(ExecutionWarning::ProfileStateInvalid);
	}

	if exit.forced {
		return Err(
			report!(ErrorMarker::operation_cancelled().with_phase("cleanup")).context(ExecuteProgramError)
		);
	}

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ExecutionFinished,)).await;
	}

	Ok(ExecuteProgramOutput {
		status: exit.status,
		warnings,
	})
}

#[cfg(test)]
mod tests {
	use super::ExecuteProgramDependencies;
	use super::ExecuteProgramError;
	use super::ExecuteProgramOutput;
	use super::execute_program;
	use crate::ErrorMarker;
	use crate::execution::ExecutionWarning;
	use crate::execution::RetainedExecutionInis;
	use crate::ports::AdapterState;
	use crate::ports::ExecutionProfile;
	use crate::ports::ExecutionProfileProjection;
	use crate::ports::LaunchPlan;
	use crate::ports::LaunchProvider;
	use crate::ports::LaunchTarget;
	use crate::ports::PortFuture;
	use crate::ports::ProfileWarning;
	use crate::ports::ProgramExit;
	use crate::ports::ProgramOutput;
	use crate::ports::ProgramSupervision;
	use crate::ports::RunningProgram;
	use crate::ports::StagedExecutionProfile;
	use crate::ports::VirtualFileSystem;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ModName;
	use domain::ModPriority;
	use domain::OutputTarget;
	use domain::ProcessStatus;
	use domain::Program;
	use domain::ProviderIdentity;
	use domain::WorkingDirectory;
	use rootcause::Report;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;
	use rootcause::report;
	use std::env::temp_dir;
	use std::future::Future;
	use std::future::ready;
	use std::path::PathBuf;
	use std::pin::Pin;
	use std::sync::Arc;
	use std::sync::Mutex;
	use tokio_util::sync::CancellationToken;

	#[derive(Clone, Copy)]
	enum Supervision {
		Exited,
		Forced,
		Failed,
		FailedAndUndrained,
		Undrained,
		DrainUnknown,
	}

	#[derive(Clone)]
	struct Scenario {
		providers: Vec<LaunchProvider>,
		warnings: Vec<ProfileWarning>,
		cancel_after_file_system: bool,
		supervision: Supervision,
		preservation_fails: bool,
		profile_state_valid: bool,
	}

	type Steps = Arc<Mutex<Vec<String>>>;

	/// Records when the use case drops an unlaunched file system.
	struct ClosedOnDrop(Steps);
	impl Drop for ClosedOnDrop {
		fn drop(&mut self) {
			record(&self.0, "close");
		}
	}

	fn default_scenario() -> Result<Scenario, ErrorMarker> {
		Ok(Scenario {
			providers: vec![
				provider("Target", true)?,
				provider("Disabled", false)?,
				LaunchProvider {
					identity: ProviderIdentity::Overwrite,
					enabled: true,
				},
			],
			warnings: Vec::new(),
			cancel_after_file_system: false,
			supervision: Supervision::Exited,
			preservation_fails: false,
			profile_state_valid: true,
		})
	}

	fn provider(name: &str, enabled: bool) -> Result<LaunchProvider, ErrorMarker> {
		Ok(LaunchProvider {
			identity: ProviderIdentity::DataMod {
				mod_name: mod_name(name)?,
				priority: ModPriority::new(0),
			},
			enabled,
		})
	}

	fn mod_name(name: &str) -> Result<ModName, ErrorMarker> {
		ModName::new(name.to_owned()).map_err(|error| error.context(ErrorMarker::invalid_mod_name()))
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

	fn fake_dependencies(scenario: Scenario) -> (ExecuteProgramDependencies, Steps) {
		let steps = Steps::default();
		let record_step = |step: &'static str| {
			let steps = steps.clone();
			move || record(&steps, step)
		};
		let resolved = record_step("resolve");
		let prepared = record_step("prepare");
		let projected = record_step("project");
		let staged = record_step("stage");
		let launched = record_step("launch");
		let supervised = record_step("supervise");
		let preserved = record_step("preserve");
		let finished = record_step("finish");
		let checked = record_step("check");
		let created = steps.clone();
		let progress = steps.clone();

		let dependencies = ExecuteProgramDependencies {
			report_progress: Some(Arc::new(move |event| {
				record(&progress, format!("progress:{event:?}"));
				Box::pin(ready(())) as Pin<Box<dyn Future<Output = ()> + Send>>
			})),
			resolve_launch_target: Arc::new(move |_, _, _| {
				resolved();
				complete(Ok(LaunchTarget(AdapterState::new("target"))))
			}),
			prepare_launch_plan: Arc::new(move |_| {
				prepared();
				complete(Ok(LaunchPlan {
					providers: scenario.providers.clone(),
					state: AdapterState::new("plan"),
				}))
			}),
			project_execution_profile: Arc::new(move |plan: &LaunchPlan| {
				assert_eq!(plan.state.downcast_ref::<&str>(), Some(&"plan"));
				projected();
				complete(Ok(ExecutionProfileProjection {
					profile: ExecutionProfile(AdapterState::new("profile")),
					warnings: scenario.warnings.clone(),
				}))
			}),
			stage_execution_profile: Arc::new(move |_: &LaunchPlan, _| {
				staged();
				complete(Ok(StagedExecutionProfile {
					directory: PathBuf::from("staged-inis"),
					state: AdapterState::new("staged"),
				}))
			}),
			create_virtual_file_system: Arc::new(
				move |plan: LaunchPlan,
				      profile: ExecutionProfile,
				      staged: &StagedExecutionProfile,
				      output_mod: Option<ModName>,
				      cancellation: CancellationToken| {
					assert_eq!(plan.state.downcast::<&str>(), Some("plan"));
					assert_eq!(profile.0.downcast::<&str>(), Some("profile"));
					assert_eq!(staged.directory, PathBuf::from("staged-inis"));
					let output = output_mod
						.map_or_else(|| "overwrite".to_owned(), |name| name.to_string());
					record(&created, format!("create:{output}"));
					if scenario.cancel_after_file_system {
						cancellation.cancel();
						let file_system = ClosedOnDrop(created.clone());
						return complete(Ok(VirtualFileSystem(AdapterState::new(file_system))));
					}
					complete(Ok(VirtualFileSystem(AdapterState::new("file system"))))
				},
			),
			launch_program: Arc::new(move |file_system: VirtualFileSystem, target: LaunchTarget| {
				assert_eq!(file_system.0.downcast::<&str>(), Some("file system"));
				assert_eq!(target.0.downcast::<&str>(), Some("target"));
				launched();
				complete(Ok(RunningProgram(AdapterState::new("program"))))
			}),
			supervise_program: Arc::new(move |program: RunningProgram, cancellation: CancellationToken| {
				assert_eq!(program.0.downcast::<&str>(), Some("program"));
				supervised();

				// Forced termination follows caller cancellation, so the caller's token
				// is cancelled before the post-run check runs.
				if matches!(scenario.supervision, Supervision::Forced) {
					cancellation.cancel();
				}

				let exit = match scenario.supervision {
					Supervision::Failed | Supervision::FailedAndUndrained => {
						Err(report!(ErrorMarker::execution_supervision_failed()
							.with_phase("running")))
					}
					Supervision::Forced => Ok(ProgramExit {
						status: ProcessStatus::new(0xc000_013a),
						forced: true,
					}),
					Supervision::Exited | Supervision::Undrained | Supervision::DrainUnknown => {
						Ok(ProgramExit {
							status: ProcessStatus::new(259),
							forced: false,
						})
					}
				};
				let job_drained = match scenario.supervision {
					Supervision::Undrained | Supervision::FailedAndUndrained => Ok(false),
					Supervision::DrainUnknown => {
						Err(report!(ErrorMarker::io_failure()).into_dynamic())
					}
					Supervision::Exited | Supervision::Forced | Supervision::Failed => Ok(true),
				};

				complete(Ok(ProgramSupervision {
					exit,
					job_drained,
					output: ProgramOutput(AdapterState::new("output")),
				}))
			}),
			preserve_execution_profile: Arc::new(move |staged: StagedExecutionProfile| {
				assert_eq!(staged.state.downcast::<&str>(), Some("staged"));
				preserved();
				if scenario.preservation_fails {
					return complete(Err(report!(ErrorMarker::environment_invalid(Some(
						"profile_changed"
					)))));
				}
				complete(Ok(()))
			}),
			finish_program_output: Arc::new(move |output: ProgramOutput| {
				assert_eq!(output.0.downcast::<&str>(), Some("output"));
				finished();
				complete(Ok(()))
			}),
			check_profile_state: Arc::new(move |cancellation: CancellationToken| {
				assert!(!cancellation.is_cancelled());
				checked();
				if !scenario.profile_state_valid {
					return complete(Err(report!(ErrorMarker::environment_invalid(None))));
				}
				complete(Ok(()))
			}),
		};

		(dependencies, steps)
	}

	async fn run(
		dependencies: ExecuteProgramDependencies,
		output_target: OutputTarget,
		cancellation: CancellationToken,
	) -> Result<ExecuteProgramOutput, ExecuteProgramError> {
		let program = Program::new("tool.exe".into()).map_err(|error| {
			error.context(ErrorMarker::program_unsupported())
				.context(ExecuteProgramError)
		})?;

		execute_program(
			dependencies,
			game_binding()?,
			output_target,
			None,
			program,
			Vec::new(),
			cancellation,
		)
		.await
	}

	fn game_directory() -> PathBuf {
		temp_dir().join("Fallout New Vegas")
	}

	fn game_binding() -> Result<GameBinding, ExecuteProgramError> {
		let path = GameInstallationPath::new(game_directory()).map_err(|error| {
			error.context(ErrorMarker::game_install_invalid())
				.context(ExecuteProgramError)
		})?;

		Ok(GameBinding::new(path))
	}

	fn has_marker(error: &Report<ExecuteProgramError>, marker: &ErrorMarker) -> bool {
		error.iter_reports()
			.any(|cause| cause.downcast_current_context::<ErrorMarker>() == Some(marker))
	}

	fn retained_paths(error: &Report<ExecuteProgramError>) -> Vec<PathBuf> {
		error.iter_reports()
			.filter_map(|cause| cause.downcast_current_context::<RetainedExecutionInis>())
			.map(|retained| retained.path.clone())
			.collect()
	}

	#[tokio::test]
	async fn composes_steps_in_order_and_maps_warnings() -> Result<(), ErrorMarker> {
		let mut scenario = default_scenario()?;
		scenario.warnings = vec![
			ProfileWarning::LoadOrderNotEnforced,
			ProfileWarning::Unavailable {
				file: "Plugins.TXT".into(),
				plugin: "Missing.esp".into(),
			},
			ProfileWarning::Unavailable {
				file: "loadorder.txt".into(),
				plugin: "Ordered.esp".into(),
			},
			ProfileWarning::Duplicate {
				file: "plugins.txt".into(),
				plugin: "Duplicate.esp".into(),
			},
			ProfileWarning::Unlisted {
				plugin: "Unlisted.esp".into(),
			},
		];
		scenario.profile_state_valid = false;
		let (dependencies, steps) = fake_dependencies(scenario);

		let output = run(
			dependencies,
			OutputTarget::DataMod(mod_name("target")?),
			CancellationToken::new(),
		)
		.await
		.context(ErrorMarker::execution_supervision_failed())?;

		assert_eq!(
			recorded(&steps),
			[
				"progress:PreparingExecution",
				"resolve",
				"prepare",
				"project",
				"stage",
				"create:target",
				"launch",
				"progress:ExecutionPrepared",
				"supervise",
				"preserve",
				"finish",
				"check",
				"progress:ExecutionFinished",
			]
		);
		assert_eq!(output.status.value(), 259);
		assert_eq!(
			output.warnings,
			[
				ExecutionWarning::LoadOrderNotEnforced,
				ExecutionWarning::StalePluginEntry {
					name: "Missing.esp".into()
				},
				ExecutionWarning::StaleLoadOrderEntry {
					name: "Ordered.esp".into()
				},
				ExecutionWarning::DuplicatePluginEntry {
					file: "plugins.txt".into(),
					name: "Duplicate.esp".into()
				},
				ExecutionWarning::UnlistedPlugin {
					name: "Unlisted.esp".into()
				},
				ExecutionWarning::ProfileStateInvalid,
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn overwrite_target_needs_no_data_mod() -> Result<(), ErrorMarker> {
		let (dependencies, steps) = fake_dependencies(default_scenario()?);

		let output = run(dependencies, OutputTarget::Overwrite, CancellationToken::new())
			.await
			.context(ErrorMarker::execution_supervision_failed())?;

		assert!(output.warnings.is_empty());
		assert!(recorded(&steps).contains(&"create:overwrite".to_owned()));
		Ok(())
	}

	#[tokio::test]
	async fn data_mod_output_target_must_exist_and_be_enabled() -> Result<(), ErrorMarker> {
		for (name, expected) in [
			("Missing", ErrorMarker::output_target_not_found()),
			("Disabled", ErrorMarker::output_target_disabled()),
		] {
			let (dependencies, steps) = fake_dependencies(default_scenario()?);

			let Err(error) = run(
				dependencies,
				OutputTarget::DataMod(mod_name(name)?),
				CancellationToken::new(),
			)
			.await
			else {
				return Err(report!(ErrorMarker::execution_supervision_failed()));
			};

			assert!(has_marker(&error, &expected.with_mod_name(mod_name(name)?)));
			assert_eq!(recorded(&steps), ["progress:PreparingExecution", "resolve", "prepare"]);
		}
		Ok(())
	}

	#[tokio::test]
	async fn cancellation_before_launch_stops_without_later_steps() -> Result<(), ErrorMarker> {
		let (dependencies, steps) = fake_dependencies(default_scenario()?);
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let Err(error) = run(dependencies, OutputTarget::Overwrite, cancellation).await else {
			return Err(report!(ErrorMarker::execution_supervision_failed()));
		};

		assert!(has_marker(&error, &ErrorMarker::operation_cancelled()));
		assert_eq!(recorded(&steps), ["progress:PreparingExecution"]);

		let mut scenario = default_scenario()?;
		scenario.cancel_after_file_system = true;
		let (dependencies, steps) = fake_dependencies(scenario);

		let Err(error) = run(dependencies, OutputTarget::Overwrite, CancellationToken::new()).await else {
			return Err(report!(ErrorMarker::execution_supervision_failed()));
		};

		assert!(has_marker(&error, &ErrorMarker::operation_cancelled()));
		assert_eq!(retained_paths(&error), [PathBuf::from("staged-inis")]);
		assert_eq!(
			recorded(&steps),
			[
				"progress:PreparingExecution",
				"resolve",
				"prepare",
				"project",
				"stage",
				"create:overwrite",
				"close",
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn uncertain_job_drain_retains_the_staged_profile() -> Result<(), ErrorMarker> {
		for (supervision, causes) in [
			(Supervision::Undrained, Vec::new()),
			(Supervision::DrainUnknown, vec![ErrorMarker::io_failure()]),
			(
				Supervision::FailedAndUndrained,
				vec![ErrorMarker::execution_supervision_failed().with_phase("running")],
			),
		] {
			let mut scenario = default_scenario()?;
			scenario.supervision = supervision;
			let (dependencies, steps) = fake_dependencies(scenario);

			let Err(error) = run(dependencies, OutputTarget::Overwrite, CancellationToken::new()).await
			else {
				return Err(report!(ErrorMarker::execution_supervision_failed()));
			};

			assert!(has_marker(
				&error,
				&ErrorMarker::execution_supervision_failed().with_phase("profile_retained")
			));
			for cause in causes {
				assert!(has_marker(&error, &cause));
			}
			assert_eq!(
				retained_paths(&error),
				[PathBuf::from("staged-inis"), PathBuf::from("staged-inis")]
			);
			assert_eq!(recorded(&steps).last().map(String::as_str), Some("supervise"));
		}
		Ok(())
	}

	#[tokio::test]
	async fn drained_supervision_failure_preserves_the_profile_before_failing() -> Result<(), ErrorMarker> {
		let mut scenario = default_scenario()?;
		scenario.supervision = Supervision::Failed;
		let (dependencies, steps) = fake_dependencies(scenario);

		let Err(error) = run(dependencies, OutputTarget::Overwrite, CancellationToken::new()).await else {
			return Err(report!(ErrorMarker::execution_supervision_failed()));
		};

		assert!(has_marker(
			&error,
			&ErrorMarker::execution_supervision_failed().with_phase("running")
		));
		assert!(retained_paths(&error).is_empty());
		assert_eq!(recorded(&steps).last().map(String::as_str), Some("finish"));
		Ok(())
	}

	#[tokio::test]
	async fn failed_preservation_retains_the_staged_profile() -> Result<(), ErrorMarker> {
		let mut scenario = default_scenario()?;
		scenario.preservation_fails = true;
		let (dependencies, steps) = fake_dependencies(scenario);

		let Err(error) = run(dependencies, OutputTarget::Overwrite, CancellationToken::new()).await else {
			return Err(report!(ErrorMarker::execution_supervision_failed()));
		};

		assert!(has_marker(
			&error,
			&ErrorMarker::environment_invalid(Some("profile_changed"))
		));
		assert_eq!(retained_paths(&error), [PathBuf::from("staged-inis")]);
		assert_eq!(recorded(&steps).last().map(String::as_str), Some("preserve"));
		Ok(())
	}

	#[tokio::test]
	async fn forced_cancellation_fails_after_preservation_and_the_postrun_check() -> Result<(), ErrorMarker> {
		let mut scenario = default_scenario()?;
		scenario.supervision = Supervision::Forced;
		let (dependencies, steps) = fake_dependencies(scenario);
		let cancellation = CancellationToken::new();

		let Err(error) = run(dependencies, OutputTarget::Overwrite, cancellation.clone()).await else {
			return Err(report!(ErrorMarker::execution_supervision_failed()));
		};

		// The fake check asserts that its own token is not cancelled.
		assert!(cancellation.is_cancelled());
		assert!(has_marker(
			&error,
			&ErrorMarker::operation_cancelled().with_phase("cleanup")
		));
		assert!(retained_paths(&error).is_empty());
		assert_eq!(recorded(&steps)[8..], ["supervise", "preserve", "finish", "check"]);
		Ok(())
	}

	#[tokio::test]
	async fn child_defaults_to_the_game_directory_unless_a_directory_is_given() -> Result<(), ErrorMarker> {
		let explicit = temp_dir().join("explicit");
		for (requested, expected) in [(None, game_directory()), (Some(explicit.clone()), explicit)] {
			let (mut dependencies, _steps) = fake_dependencies(default_scenario()?);
			let observed = Arc::new(Mutex::new(None));
			dependencies.resolve_launch_target = Arc::new({
				let observed = observed.clone();
				move |_, _, working_directory: WorkingDirectory| {
					if let Ok(mut observed) = observed.lock() {
						*observed = Some(working_directory.as_path().to_owned());
					}
					complete(Ok(LaunchTarget(AdapterState::new("target"))))
				}
			});
			let requested = requested
				.map(WorkingDirectory::new)
				.transpose()
				.map_err(|error| error.context(ErrorMarker::invalid_working_directory()))?;
			let program = Program::new("tool.exe".into())
				.map_err(|error| error.context(ErrorMarker::program_unsupported()))?;

			execute_program(
				dependencies,
				game_binding().context(ErrorMarker::game_install_invalid())?,
				OutputTarget::Overwrite,
				requested,
				program,
				Vec::new(),
				CancellationToken::new(),
			)
			.await
			.context(ErrorMarker::execution_supervision_failed())?;

			assert_eq!(
				observed.lock().ok().and_then(|observed| observed.clone()),
				Some(expected)
			);
		}
		Ok(())
	}

	#[tokio::test]
	async fn preserves_launcher_cause_under_fixed_use_case_context() -> Result<(), ErrorMarker> {
		let (mut dependencies, _steps) = fake_dependencies(default_scenario()?);
		dependencies.resolve_launch_target =
			Arc::new(|_, _, _| complete(Err(report!(ErrorMarker::program_not_found()))));

		let Err(error) = run(dependencies, OutputTarget::Overwrite, CancellationToken::new()).await else {
			return Err(report!(ErrorMarker::execution_supervision_failed()));
		};

		assert_eq!(*error.current_context(), ExecuteProgramError);
		assert!(has_marker(&error, &ErrorMarker::program_not_found()));
		Ok(())
	}
}
