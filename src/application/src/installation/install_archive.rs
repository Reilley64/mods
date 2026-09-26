use self::fomod::DependencyFacts;
use self::fomod::condition_tree_matches;
use self::fomod::evaluate;
use crate::ports::ProgressEvent;
use crate::ports::ReportProgress;
mod fomod;
mod planning;

use self::planning::plan_candidates;
use crate::conflicts::InstallationParticipation;
use crate::conflicts::project_installation;
use crate::errors::ErrorMarker;
use crate::installation::AcceptedChoice;
use crate::installation::AdditionalSelectionsRequired;
use crate::installation::ApprovedInstallation;
use crate::installation::AutomaticChoiceEvent;
use crate::installation::CandidateDecision;
use crate::installation::ConditionEvaluation;
use crate::installation::IndexedInstaller;
use crate::installation::InstallMode;
use crate::installation::InstallPlan;
use crate::installation::InstallPreview;
use crate::installation::InstallWarning;
use crate::installation::InstalledArchive;
use crate::installation::ProjectedModState;
use crate::installation::ResolvedFlag;
use crate::installation::UnresolvedGroup;
use crate::ports::AssessInstallation;
use crate::ports::BeginInstallation;
use crate::ports::ExtractApprovedFiles;
use crate::ports::IndexArchive as IndexArchivePort;
use crate::ports::InstallationStateAccess;
use crate::ports::LoadInstallationState;
use crate::ports::ReadConflictContent;
use crate::ports::ReadGameVersion;
use crate::ports::ReadXnvseVersion;
use crate::ports::ScanEnvironmentConflicts;
use domain::ArchiveIdentity;
use domain::ArchivePath;
use domain::FomodChoice;
use domain::FomodCondition;
use domain::InstallCandidate;
use domain::ModName;
use domain::ModPriority;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct InstallArchiveDependencies {
	pub report_progress: Option<ReportProgress>,
	pub scan_environment_conflicts: ScanEnvironmentConflicts,
	pub read_conflict_content: ReadConflictContent,
	pub load_installation_state: LoadInstallationState,
	pub assess_installation: AssessInstallation,
	pub index_archive: IndexArchivePort,
	pub read_game_version: ReadGameVersion,
	pub read_xnvse_version: ReadXnvseVersion,
	pub begin_installation: BeginInstallation,
	pub extract_approved_files: ExtractApprovedFiles,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallArchiveOutput {
	AdditionalSelectionsRequired(Box<AdditionalSelectionsRequired>),
	Preview(Box<InstallPreview>),
	Installed(Box<InstalledArchive>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallArchiveError;

impl fmt::Display for InstallArchiveError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to install archive")
	}
}

#[tracing::instrument(skip_all)]
pub async fn install_archive(
	dependencies: InstallArchiveDependencies,
	archive: ArchivePath,
	mod_name: Option<ModName>,
	replace: bool,
	choices: Vec<FomodChoice>,
	dry_run: bool,
	cancellation: CancellationToken,
) -> Result<InstallArchiveOutput, InstallArchiveError> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("settings_load"))
			.context(InstallArchiveError));
	}

	let state = dependencies
		.load_installation_state
		.call((
			if dry_run {
				InstallationStateAccess::Preview
			} else {
				InstallationStateAccess::Mutation
			},
			cancellation.clone(),
		))
		.await
		.map_err(|mut report| {
			report.current_context_mut().set_phase_if_missing("settings_load");
			report
		})
		.context(InstallArchiveError)?;

	if cancellation.is_cancelled() {
		return Err(
			report!(ErrorMarker::operation_cancelled().with_phase("game_binding_validation"))
				.context(InstallArchiveError),
		);
	}

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::SettingsLoaded,)).await;
	}

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ScanningArchive,)).await;
	}

	let index = dependencies
		.index_archive
		.call((
			archive.clone(),
			dependencies.report_progress.clone(),
			cancellation.clone(),
		))
		.await
		.map_err(|mut report| {
			report.current_context_mut().set_phase_if_missing("archive_validation");
			report
		})
		.context(InstallArchiveError)?;

	if cancellation.is_cancelled() {
		return Err(
			report!(ErrorMarker::operation_cancelled().with_phase("archive_validation"))
				.context(InstallArchiveError),
		);
	}

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ArchiveIndexed,)).await;
	}

	let requested_name = if let Some(name) = mod_name {
		name
	} else {
		archive.derived_mod_name()
			.context(ErrorMarker::invalid_mod_name())
			.context(InstallArchiveError)?
	};
	let existing = state
		.installed_mods
		.iter()
		.find(|installed| installed.name == requested_name);
	let selected_name = match (replace, existing) {
		(false, Some(installed)) => {
			return Err(
				report!(ErrorMarker::mod_already_exists().with_mod_name(installed.name.clone()))
					.context(InstallArchiveError),
			);
		}
		(true, None) => {
			return Err(
				report!(ErrorMarker::mod_not_found().with_mod_name(requested_name.clone()))
					.context(InstallArchiveError),
			);
		}
		(true, Some(installed)) => installed.name.clone(),
		(false, None) => requested_name,
	};

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::EvaluatingInstaller,)).await;
	}

	let evaluation = match &index.installer {
		IndexedInstaller::Plain { candidates, warnings } => {
			if !matches!(index.identity, ArchiveIdentity::DataArchive { .. }) {
				return Err(report!(ErrorMarker::unsafe_archive().with_phase("archive_validation"))
					.context(InstallArchiveError));
			}
			if let Some(choice) = choices.first() {
				return Err(report!(ErrorMarker::invalid_selection(
					"choices",
					Some(choice.group_id.clone()),
					Some(choice.option_id.clone()),
					Some(0),
				))
				.context(InstallArchiveError));
			}

			InstallerEvaluation {
				candidate_conditions: HashMap::new(),
				fomod_schema_version: None,
				choices: Vec::new(),
				automatic_events: Vec::new(),
				resolved_flags: Vec::new(),
				warnings: warnings.clone(),
				unresolved_groups: Vec::new(),
				candidates: candidates.clone(),
			}
		}
		IndexedInstaller::Fomod(installer) => {
			if !matches!(index.identity, ArchiveIdentity::Fomod { .. }) {
				return Err(report!(ErrorMarker::unsafe_archive().with_phase("archive_validation"))
					.context(InstallArchiveError));
			}
			let game_version = if condition_tree_matches(
				installer,
				|condition| matches!(condition, FomodCondition::GameDependency { .. }),
				&cancellation,
			)
			.context(InstallArchiveError)?
			{
				let version = dependencies
					.read_game_version
					.call((state.game_binding.clone(), cancellation.clone()))
					.await
					.map_err(|mut report| {
						report.current_context_mut().set_phase_if_missing("fomod_evaluation");
						report
					})
					.context(InstallArchiveError)?;
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					)
					.context(InstallArchiveError));
				}
				Some(version)
			} else {
				None
			};
			let nvse_version = if condition_tree_matches(
				installer,
				|condition| matches!(condition, FomodCondition::NvseDependency { .. }),
				&cancellation,
			)
			.context(InstallArchiveError)?
			{
				let version = dependencies
					.read_xnvse_version
					.call((state.game_binding.clone(), cancellation.clone()))
					.await
					.map_err(|mut report| {
						report.current_context_mut().set_phase_if_missing("fomod_evaluation");
						report
					})
					.context(InstallArchiveError)?;
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					)
					.context(InstallArchiveError));
				}
				version
			} else {
				None
			};
			let facts = DependencyFacts {
				file_dependencies: &state.file_dependencies,
				game_version: game_version.as_deref(),
				nvse_version: nvse_version.as_deref(),
			};
			let evaluation =
				evaluate(installer, &choices, facts, &cancellation).context(InstallArchiveError)?;
			InstallerEvaluation {
				candidate_conditions: evaluation.candidate_conditions,
				fomod_schema_version: Some(installer.schema_version.clone()),
				choices: evaluation.choices,
				automatic_events: evaluation.automatic_events,
				resolved_flags: evaluation.resolved_flags,
				warnings: evaluation.warnings,
				unresolved_groups: evaluation.unresolved_groups,
				candidates: evaluation.candidates,
			}
		}
	};

	if !evaluation.unresolved_groups.is_empty() {
		let output =
			InstallArchiveOutput::AdditionalSelectionsRequired(Box::new(AdditionalSelectionsRequired {
				archive_identity: index.identity,
				mod_name: selected_name,
				accepted_choices: evaluation.choices,
				automatic_events: evaluation.automatic_events,
				resolved_flags: evaluation.resolved_flags,
				unresolved_groups: evaluation.unresolved_groups,
				warnings: evaluation.warnings,
			}));

		if cancellation.is_cancelled() {
			return Err(
				report!(ErrorMarker::operation_cancelled().with_phase("fomod_evaluation"))
					.context(InstallArchiveError),
			);
		}
		return Ok(output);
	}

	let (mut planned_candidates, planning_warnings) =
		plan_candidates(evaluation.candidates, &state.current_winners, &cancellation)
			.context(InstallArchiveError)?;

	for candidate in &mut planned_candidates {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("planning"))
				.context(InstallArchiveError));
		}
		candidate.origin_condition_evaluation = evaluation
			.candidate_conditions
			.get(&candidate.candidate.candidate_id)
			.cloned();
	}

	let mut warnings = evaluation.warnings;
	for warning in planning_warnings {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("planning"))
				.context(InstallArchiveError));
		}
		warnings.push(warning);
	}

	let (mode, priority, list_position, enabled) = if let Some(installed) = existing {
		let position = state
			.installed_mods
			.iter()
			.position(|candidate| candidate.name == installed.name)
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))
			.context(InstallArchiveError)?;
		(
			InstallMode::Replacement,
			installed.priority,
			position as u64,
			installed.enabled,
		)
	} else {
		let priority = state
			.installed_mods
			.iter()
			.map(|installed| installed.priority.get())
			.max()
			.map_or(Some(0), |priority| priority.checked_add(1))
			.ok_or_else(|| report!(ErrorMarker::ambiguous_install_plan()))
			.context(InstallArchiveError)?;
		(
			InstallMode::NewInstall,
			ModPriority::new(priority),
			state.installed_mods.len() as u64,
			false,
		)
	};
	let source_basename = archive
		.as_path()
		.file_name()
		.and_then(|name| name.to_str())
		.filter(|name| !name.is_empty())
		.ok_or_else(|| report!(ErrorMarker::unsafe_archive().with_phase("archive_validation")))
		.context(InstallArchiveError)?;
	let projected_state = ProjectedModState {
		mode,
		mod_name: selected_name,
		priority,
		list_position,
		enabled,
		overlaps: Vec::new(),
	};
	let mut plan = InstallPlan {
		archive_identity: index.identity,
		mod_name: projected_state.mod_name.clone(),
		replacement: replace,
		accepted_choices: evaluation.choices,
		automatic_events: evaluation.automatic_events,
		resolved_flags: evaluation.resolved_flags,
		warnings,
		candidates: planned_candidates,
		projected_state,
	};

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::InstallationPlanned,)).await;
	}

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ScanningConflicts,)).await;
	}

	let assessment = dependencies
		.assess_installation
		.call((plan.clone(), cancellation.clone()))
		.await
		.map_err(|mut report| {
			report.current_context_mut().set_phase_if_missing("conflict_scan");
			report
		})
		.context(InstallArchiveError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan"))
			.context(InstallArchiveError));
	}
	plan.projected_state.overlaps = assessment.overlaps;

	let scan = dependencies
		.scan_environment_conflicts
		.call((cancellation.clone(),))
		.await
		.map_err(|mut report| {
			report.current_context_mut().set_phase_if_missing("conflict_scan");
			report
		})
		.context(InstallArchiveError)?;

	let participation = if dry_run {
		InstallationParticipation::HypotheticalEnabled
	} else {
		InstallationParticipation::Actual
	};
	let conflicts = project_installation(
		scan,
		&plan,
		participation,
		dependencies.read_conflict_content,
		cancellation.clone(),
	)
	.await
	.context(InstallArchiveError)?;

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ConflictsScanned,)).await;
	}

	if dry_run {
		let output = InstallArchiveOutput::Preview(Box::new(InstallPreview {
			plan,
			hypothetical_enabled_conflicts: conflicts,
		}));

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan"))
				.context(InstallArchiveError));
		}
		return Ok(output);
	}

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("extraction"))
			.context(InstallArchiveError));
	}

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ExtractingFiles,)).await;
	}

	let change = dependencies
		.begin_installation
		.call((
			ApprovedInstallation {
				source_basename: source_basename.to_owned(),
				fomod_schema_version: evaluation.fomod_schema_version,
				plan: plan.clone(),
			},
			cancellation.clone(),
		))
		.await
		.map_err(|mut report| {
			report.current_context_mut().set_phase_if_missing("publication");
			report
		})
		.context(InstallArchiveError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("extraction"))
			.context(InstallArchiveError));
	}

	let mut winners = Vec::new();
	for candidate in &plan.candidates {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("extraction"))
				.context(InstallArchiveError));
		}
		if matches!(candidate.decision, CandidateDecision::Winner { .. }) {
			winners.push(candidate.candidate.clone());
		}
	}

	dependencies
		.extract_approved_files
		.call((
			archive,
			plan.archive_identity.clone(),
			winners,
			change.begin_file,
			dependencies.report_progress.clone(),
			cancellation.clone(),
		))
		.await
		.map_err(|mut report| {
			report.current_context_mut().set_phase_if_missing("extraction");
			report
		})
		.context(InstallArchiveError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("extraction"))
			.context(InstallArchiveError));
	}

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::FilesExtracted,)).await;
	}

	change.finish.call((cancellation,)).await.context(InstallArchiveError)?;

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::InstallationPublished,)).await;
	}

	Ok(InstallArchiveOutput::Installed(Box::new(InstalledArchive {
		warnings: plan.warnings.clone(),
		plan,
		conflicts,
	})))
}

struct InstallerEvaluation {
	candidate_conditions: HashMap<u64, ConditionEvaluation>,
	fomod_schema_version: Option<String>,
	choices: Vec<AcceptedChoice>,
	automatic_events: Vec<AutomaticChoiceEvent>,
	resolved_flags: Vec<ResolvedFlag>,
	warnings: Vec<InstallWarning>,
	unresolved_groups: Vec<UnresolvedGroup>,
	candidates: Vec<InstallCandidate>,
}
#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "fixture construction and error-path assertions require known-success and known-error values"
)]
mod tests {
	use super::InstallArchiveDependencies;
	use super::InstallArchiveOutput;
	use super::install_archive;
	use crate::ErrorCode;
	use crate::ErrorMarker;
	use crate::conflicts::EnvironmentConflictScan;
	use crate::conflicts::IndexedConflictFile;
	use crate::conflicts::IndexedConflictFileId;
	use crate::conflicts::ScannedConflictProvider;
	use crate::installation::ArchiveIndex;
	use crate::installation::CandidateDecision;
	use crate::installation::FileDependencyFact;
	use crate::installation::FileDependencyKind;
	use crate::installation::FomodFlagWrite;
	use crate::installation::FomodGroup;
	use crate::installation::FomodInstaller;
	use crate::installation::FomodOption;
	use crate::installation::FomodOptionTypePattern;
	use crate::installation::IndexedInstaller;
	use crate::installation::InstallWarning;
	use crate::installation::InstallationAssessment;
	use crate::installation::InstallationState;
	use crate::ports::InstallationChange;
	use crate::ports::InstallationFile;
	use crate::ports::InstallationStateAccess;
	use crate::ports::PortFuture;
	use crate::ports::ProgressEvent;
	use domain::ArchiveIdentity;
	use domain::ArchivePath;
	use domain::ConflictRow;
	use domain::DataRelativePath;
	use domain::FileDependencyState;
	use domain::FomodCardinality;
	use domain::FomodChoice;
	use domain::FomodCondition;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::InstallCandidate;
	use domain::InstallCandidateOrigin;
	use domain::InstallationPhase;
	use domain::InvalidModName;
	use domain::OptionFileTrigger;
	use domain::Participation;
	use domain::ProviderIdentity;
	use domain::ProviderReference;
	use domain::ResolutionStatus;
	use domain::ResolvedOptionType;
	use domain::Sha256Digest;
	use domain::SteamBuildId;
	use domain::Tombstone;
	use domain::TombstoneScope;
	use rootcause::Result;
	use rootcause::report;
	use std::collections::HashMap;
	use std::env::temp_dir;
	use std::error::Error;
	use std::result::Result as StdResult;
	use std::sync::Arc;
	use std::sync::Mutex;
	use std::sync::atomic::AtomicUsize;
	use std::sync::atomic::Ordering;
	use tokio_util::sync::CancellationToken;

	fn identity() -> ArchiveIdentity {
		ArchiveIdentity::DataArchive {
			archive_sha256: Sha256Digest::new("a".repeat(64)).expect("hash"),
			package_root: String::new(),
		}
	}

	fn candidate() -> InstallCandidate {
		InstallCandidate {
			candidate_id: 1,
			origin: InstallCandidateOrigin::Required,
			phase: InstallationPhase::Required,
			declared_priority: 0,
			descriptor_order: 0,
			source_member: "Data/meshes/a.nif".into(),
			destination: DataRelativePath::new("meshes/a.nif".into()).expect("path"),
		}
	}

	fn binding() -> GameBinding {
		GameBinding::new(
			GameInstallationPath::new(temp_dir().join("fnv-install-test")).expect("game path"),
			SteamBuildId::new(1).expect("build"),
		)
	}

	fn record(order: &Mutex<Vec<&'static str>>, value: &'static str) {
		if let Ok(mut values) = order.lock() {
			values.push(value)
		}
	}

	fn dependencies(
		order: Arc<Mutex<Vec<&'static str>>>,
		cancel_after_chunk: bool,
		version_calls: Arc<AtomicUsize>,
	) -> InstallArchiveDependencies {
		InstallArchiveDependencies {
			report_progress: None,
			scan_environment_conflicts: Arc::new(|_| {
				Box::pin(async {
					Ok(EnvironmentConflictScan {
						providers: Vec::new(),
						problems: Vec::new(),
					})
				}) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			load_installation_state: Arc::new({
				let order = order.clone();
				move |access, _| {
					record(
						&order,
						match access {
							InstallationStateAccess::Preview => "load_preview",
							InstallationStateAccess::Mutation => "load_mutation",
						},
					);
					Box::pin(async {
						Ok(InstallationState {
							game_binding: binding(),
							installed_mods: Vec::new(),
							current_winners: HashMap::new(),
							file_dependencies: HashMap::new(),
						})
					}) as PortFuture<_>
				}
			}),
			assess_installation: Arc::new({
				let order = order.clone();
				move |_, _| {
					record(&order, "assess");
					Box::pin(async { Ok(InstallationAssessment { overlaps: Vec::new() }) })
						as PortFuture<_>
				}
			}),
			index_archive: Arc::new({
				let order = order.clone();
				move |_, _, _| {
					record(&order, "index");
					Box::pin(async {
						Ok(ArchiveIndex {
							identity: identity(),
							installer: IndexedInstaller::Plain {
								candidates: vec![candidate()],
								warnings: Vec::new(),
							},
						})
					}) as PortFuture<_>
				}
			}),
			read_game_version: Arc::new({
				let calls = version_calls.clone();
				move |_, _| {
					calls.fetch_add(1, Ordering::SeqCst);
					Box::pin(async { Ok(vec![1_u32, 4]) }) as PortFuture<_>
				}
			}),
			read_xnvse_version: Arc::new({
				let calls = version_calls;
				move |_, _| {
					calls.fetch_add(1, Ordering::SeqCst);
					Box::pin(async { Ok(Some(vec![6_u32, 3])) }) as PortFuture<_>
				}
			}),
			begin_installation: Arc::new({
				let order = order.clone();
				move |_, _| {
					record(&order, "begin");
					let begin_file = Arc::new({
						let order = order.clone();
						move |_, _| {
							record(&order, "begin_file");
							let write_chunk = Arc::new({
								let order = order.clone();
								move |_: Vec<u8>, token: CancellationToken| {
									record(&order, "chunk");
									if cancel_after_chunk {
										token.cancel()
									}
									Box::pin(async { Ok(()) }) as PortFuture<_>
								}
							});
							let finish = Arc::new({
								let order = order.clone();
								move |_| {
									record(&order, "finish_file");
									Box::pin(async { Ok(()) }) as PortFuture<_>
								}
							});
							Box::pin(async move {
								let file = InstallationFile { write_chunk, finish };
								Ok(file)
							}) as PortFuture<_>
						}
					});
					let finish = Arc::new({
						let order = order.clone();
						move |_| {
							record(&order, "finish_change");
							Box::pin(async { Ok(()) }) as PortFuture<_>
						}
					});
					Box::pin(async move { Ok(InstallationChange { begin_file, finish }) })
						as PortFuture<_>
				}
			}),
			extract_approved_files: Arc::new({
				let order = order.clone();
				move |_, _, candidates, begin_file, _, token| {
					record(&order, "extract");
					Box::pin(async move {
						let file = begin_file
							.call((candidates[0].destination.clone(), token.clone()))
							.await?;
						file.write_chunk.call((vec![1, 2, 3], token.clone())).await?;
						if token.is_cancelled() {
							return Ok(());
						}
						file.finish.call((token,)).await
					}) as PortFuture<_>
				}
			}),
		}
	}
	fn fomod_option(id: &str, option_type: ResolvedOptionType, flags: &[(&str, &str)]) -> FomodOption {
		FomodOption {
			id: id.to_owned(),
			label: id.to_owned(),
			description: String::new(),
			condition: FomodCondition::Constant(true),
			default_type: option_type,
			type_patterns: Vec::new(),
			flag_writes: flags
				.iter()
				.map(|(name, value)| FomodFlagWrite {
					name: (*name).to_owned(),
					value: (*value).to_owned(),
				})
				.collect(),
			file_candidates: Vec::new(),
			file_effects: Vec::new(),
		}
	}

	fn fomod_installer(groups: Vec<FomodGroup>) -> FomodInstaller {
		FomodInstaller {
			schema_version: "5.0".to_owned(),
			module_condition: FomodCondition::Constant(true),
			groups,
			required_candidates: Vec::new(),
			conditional_candidates: Vec::new(),
			warnings: Vec::new(),
		}
	}

	fn fomod_dependencies(
		installer: FomodInstaller,
		file_dependencies: HashMap<String, FileDependencyFact>,
	) -> InstallArchiveDependencies {
		let mut dependencies =
			dependencies(Arc::new(Mutex::new(Vec::new())), false, Arc::new(AtomicUsize::new(0)));
		dependencies.load_installation_state = Arc::new(move |_, _| {
			let file_dependencies = file_dependencies.clone();
			Box::pin(async move {
				Ok(InstallationState {
					game_binding: binding(),
					installed_mods: Vec::new(),
					current_winners: HashMap::new(),
					file_dependencies,
				})
			}) as PortFuture<_>
		});
		dependencies.index_archive = Arc::new(move |_, _, _| {
			let installer = installer.clone();
			Box::pin(async move {
				Ok(ArchiveIndex {
					identity: ArchiveIdentity::Fomod {
						archive_sha256: Sha256Digest::new("c".repeat(64)).expect("hash"),
						package_root: String::new(),
						config_member: "fomod/ModuleConfig.xml".into(),
						config_sha256: Sha256Digest::new("b".repeat(64)).expect("hash"),
					},
					installer: IndexedInstaller::Fomod(installer),
				})
			}) as PortFuture<_>
		});
		dependencies
	}

	#[tokio::test]
	async fn cancelled_dependency_retains_cause_and_owning_phase() -> Result<()> {
		let mut dependencies =
			dependencies(Arc::new(Mutex::new(Vec::new())), false, Arc::new(AtomicUsize::new(0)));
		dependencies.load_installation_state = Arc::new(|_, _| {
			Box::pin(async { Err(report!("dependency cause").context(ErrorMarker::operation_cancelled())) })
				as PortFuture<_>
		});

		let report = install_archive(
			dependencies,
			ArchivePath::new(temp_dir().join("fixture.zip"))?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.expect_err("cancelled dependency");
		let marker = report
			.iter_reports()
			.find_map(|report| report.downcast_current_context::<ErrorMarker>())
			.expect("semantic marker");

		assert_eq!(marker.code(), ErrorCode::OperationCancelled);
		assert_eq!(marker.phase(), Some("settings_load"));
		assert!(format!("{report:?}").contains("dependency cause"));
		Ok(())
	}

	#[tokio::test]
	async fn plain_install_streams_and_never_reads_versions() -> StdResult<(), Box<dyn Error>> {
		let order = Arc::new(Mutex::new(Vec::new()));
		let calls = Arc::new(AtomicUsize::new(0));
		let output = install_archive(
			dependencies(order.clone(), false, calls.clone()),
			ArchivePath::new(temp_dir().join("Plain Mod.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			false,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "install")?;
		let InstallArchiveOutput::Installed(installed) = output else {
			return Err("expected installed".into());
		};
		assert!(installed.warnings.is_empty());
		assert_eq!(installed.plan.mod_name.as_str(), "Plain Mod");
		assert!(!installed.plan.projected_state.enabled);
		assert!(installed.conflicts.rows.is_empty());
		assert_eq!(calls.load(Ordering::SeqCst), 0);
		assert_eq!(
			*order.lock().map_err(|_| "lock")?,
			vec![
				"load_mutation",
				"index",
				"assess",
				"begin",
				"extract",
				"begin_file",
				"chunk",
				"finish_file",
				"finish_change"
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn progress_reports_real_installation_checkpoints() -> Result<()> {
		let events = Arc::new(Mutex::new(Vec::new()));
		let mut dependencies =
			dependencies(Arc::new(Mutex::new(Vec::new())), false, Arc::new(AtomicUsize::new(0)));
		dependencies.report_progress = Some(Arc::new({
			let events = events.clone();
			move |event| {
				let events = events.clone();
				Box::pin(async move {
					events.lock().expect("events").push(event);
				})
			}
		}));

		install_archive(
			dependencies,
			ArchivePath::new(temp_dir().join("Progress.zip"))?,
			None,
			false,
			Vec::new(),
			false,
			CancellationToken::new(),
		)
		.await?;

		assert_eq!(
			*events.lock().expect("events"),
			vec![
				ProgressEvent::SettingsLoaded,
				ProgressEvent::ScanningArchive,
				ProgressEvent::ArchiveIndexed,
				ProgressEvent::EvaluatingInstaller,
				ProgressEvent::InstallationPlanned,
				ProgressEvent::ScanningConflicts,
				ProgressEvent::ConflictsScanned,
				ProgressEvent::ExtractingFiles,
				ProgressEvent::FilesExtracted,
				ProgressEvent::InstallationPublished,
			]
		);
		Ok(())
	}

	#[tokio::test]
	async fn pending_mutation_stops_before_archive_evaluation() -> StdResult<(), Box<dyn Error>> {
		let order = Arc::new(Mutex::new(Vec::new()));
		let mut dependencies = dependencies(order.clone(), false, Arc::new(AtomicUsize::new(0)));
		dependencies.load_installation_state = Arc::new({
			let order = order.clone();
			move |access, _| {
				record(
					&order,
					match access {
						InstallationStateAccess::Preview => "load_preview",
						InstallationStateAccess::Mutation => "load_mutation",
					},
				);
				Box::pin(async { Err(report!(ErrorMarker::manual_cleanup_required())) })
					as PortFuture<_>
			}
		});

		let result = install_archive(
			dependencies,
			ArchivePath::new(temp_dir().join("pending.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			false,
			CancellationToken::new(),
		)
		.await;

		let report = result.expect_err("pending mutation must fail");
		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::ManualCleanupRequired)));
		assert_eq!(*order.lock().map_err(|_| "lock")?, vec!["load_mutation"]);
		Ok(())
	}

	#[tokio::test]
	async fn invalid_derived_mod_name_preserves_domain_cause() -> StdResult<(), Box<dyn Error>> {
		let result = install_archive(
			dependencies(Arc::new(Mutex::new(Vec::new())), false, Arc::new(AtomicUsize::new(0))),
			ArchivePath::new(temp_dir().join("CON.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			false,
			CancellationToken::new(),
		)
		.await;

		let report = result.expect_err("reserved archive stem must fail");
		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::InvalidModName)));
		assert!(report
			.iter_reports()
			.any(|report| report.downcast_current_context::<InvalidModName>().is_some()));
		Ok(())
	}

	#[tokio::test]
	async fn cancellation_after_chunk_preserves_partial_file_without_finishing() -> StdResult<(), Box<dyn Error>> {
		let order = Arc::new(Mutex::new(Vec::new()));
		let events = Arc::new(Mutex::new(Vec::new()));
		let mut dependencies = dependencies(order.clone(), true, Arc::new(AtomicUsize::new(0)));
		dependencies.report_progress = Some(Arc::new({
			let events = events.clone();
			move |event| {
				let events = events.clone();
				Box::pin(async move {
					events.lock().expect("events").push(event);
				})
			}
		}));

		let result = install_archive(
			dependencies,
			ArchivePath::new(temp_dir().join("cancel.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			false,
			CancellationToken::new(),
		)
		.await;
		let report = result.expect_err("cancelled");
		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::OperationCancelled)));
		assert!(!order.lock().map_err(|_| "lock")?.contains(&"finish_file"));
		assert!(!order.lock().map_err(|_| "lock")?.contains(&"finish_change"));
		let events = events.lock().expect("events");
		assert!(events.contains(&ProgressEvent::ExtractingFiles));
		assert!(!events.contains(&ProgressEvent::FilesExtracted));
		assert!(!events.contains(&ProgressEvent::InstallationPublished));

		Ok(())
	}

	#[tokio::test]
	async fn dry_run_does_not_create_change() -> StdResult<(), Box<dyn Error>> {
		let order = Arc::new(Mutex::new(Vec::new()));
		let output = install_archive(
			dependencies(order.clone(), false, Arc::new(AtomicUsize::new(0))),
			ArchivePath::new(temp_dir().join("preview.7z")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "preview")?;
		assert!(matches!(output, InstallArchiveOutput::Preview(_)));
		assert_eq!(
			*order.lock().map_err(|_| "lock")?,
			vec!["load_preview", "index", "assess"]
		);
		Ok(())
	}

	#[tokio::test]
	async fn unresolved_conditions_retain_results_for_short_circuited_children() -> Result<()> {
		let installer = fomod_installer(vec![FomodGroup {
			id: "choice".into(),
			label: "Choice".into(),
			description: String::new(),
			cardinality: FomodCardinality::SelectExactlyOne,
			condition: FomodCondition::Any(vec![
				FomodCondition::Constant(true),
				FomodCondition::FlagDependency {
					name: "absent".into(),
					value: "on".into(),
				},
			]),
			options: vec![
				fomod_option("a", ResolvedOptionType::Optional, &[]),
				fomod_option("b", ResolvedOptionType::Optional, &[]),
			],
		}]);

		let output = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("conditions.zip"))?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await?;
		let InstallArchiveOutput::AdditionalSelectionsRequired(output) = output else {
			return Err(report!("expected choices"));
		};
		let evaluation = &output.unresolved_groups[0].condition_evaluation;
		assert!(evaluation.result);
		assert_eq!(
			evaluation.children.iter().map(|child| child.result).collect::<Vec<_>>(),
			[true, false]
		);
		Ok(())
	}

	#[tokio::test]
	async fn required_fomod_flag_reaches_fixed_point_and_reveals_group() -> StdResult<(), Box<dyn Error>> {
		let installer = fomod_installer(vec![
			FomodGroup {
				id: "core".into(),
				label: "Core".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option(
					"required",
					ResolvedOptionType::Required,
					&[("mode", "on")],
				)],
			},
			FomodGroup {
				id: "extra".into(),
				label: "Extra".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::FlagDependency {
					name: "mode".into(),
					value: "on".into(),
				},
				options: vec![fomod_option("pick", ResolvedOptionType::Recommended, &[])],
			},
		]);

		let output = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("fixed-point.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "evaluate")?;

		let InstallArchiveOutput::AdditionalSelectionsRequired(required) = output else {
			return Err("expected additional selections".into());
		};
		assert_eq!(required.unresolved_groups[0].id, "extra");
		assert_eq!(required.mod_name.as_str(), "fixed-point");
		assert_eq!(required.automatic_events.len(), 1);
		assert_eq!(required.automatic_events[0].sequence, 0);
		assert_eq!(required.automatic_events[0].group_id, "core");
		assert_eq!(required.automatic_events[0].option_id, "required");
		assert_eq!(required.resolved_flags[0].name, "mode");
		assert_eq!(required.resolved_flags[0].value, "on");
		assert_eq!(required.resolved_flags[0].winning_event.sequence, 0);
		assert_eq!(
			required.unresolved_groups[0].condition,
			FomodCondition::FlagDependency {
				name: "mode".into(),
				value: "on".into()
			}
		);
		assert_eq!(
			required.unresolved_groups[0].options[0].condition,
			FomodCondition::Constant(true)
		);
		assert!(required.unresolved_groups[0].options[0].flag_effects.is_empty());
		assert!(required.unresolved_groups[0].options[0].file_effects.is_empty());
		assert!(matches!(required.archive_identity, ArchiveIdentity::Fomod { .. }));
		Ok(())
	}

	#[tokio::test]
	async fn visible_not_usable_fomod_options_are_returned_as_unselectable() -> StdResult<(), Box<dyn Error>> {
		let installer = fomod_installer(vec![FomodGroup {
			id: "choice".into(),
			label: "Choice".into(),
			description: String::new(),
			cardinality: FomodCardinality::SelectExactlyOne,
			condition: FomodCondition::Constant(true),
			options: vec![
				fomod_option("available", ResolvedOptionType::Optional, &[]),
				fomod_option("unavailable", ResolvedOptionType::NotUsable, &[]),
			],
		}]);

		let output = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("visible-not-usable.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "evaluate")?;

		let InstallArchiveOutput::AdditionalSelectionsRequired(required) = output else {
			return Err("expected additional selections".into());
		};
		let options = &required.unresolved_groups[0].options;
		assert_eq!(options.len(), 2);
		let unavailable = options
			.iter()
			.find(|option| option.id == "unavailable")
			.ok_or("unavailable option")?;
		assert_eq!(unavailable.resolved_type, ResolvedOptionType::NotUsable);
		assert!(!unavailable.selectable);
		assert!(!unavailable.synthetic);
		let available = options
			.iter()
			.find(|option| option.id == "available")
			.ok_or("available option")?;
		assert!(available.selectable);
		Ok(())
	}

	#[tokio::test]
	async fn positive_minimum_fomod_groups_without_selectable_options_are_unsupported()
	-> StdResult<(), Box<dyn Error>> {
		for cardinality in [FomodCardinality::SelectExactlyOne, FomodCardinality::SelectAtLeastOne] {
			let mut unavailable = fomod_option("hidden", ResolvedOptionType::Optional, &[]);
			unavailable.condition = FomodCondition::Constant(false);
			let installer = fomod_installer(vec![FomodGroup {
				id: "choice".into(),
				label: "Choice".into(),
				description: String::new(),
				cardinality,
				condition: FomodCondition::Constant(true),
				options: vec![
					fomod_option("not-usable", ResolvedOptionType::NotUsable, &[]),
					unavailable,
				],
			}]);

			let result = install_archive(
				fomod_dependencies(installer, HashMap::new()),
				ArchivePath::new(temp_dir().join(format!("unsatisfiable-{cardinality:?}.zip")))
					.map_err(|_| "archive")?,
				None,
				false,
				Vec::new(),
				true,
				CancellationToken::new(),
			)
			.await;

			let report = result.expect_err("positive minimum without selectable options must fail");
			assert!(report.iter_reports().any(|report| report
				.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::UnsupportedInstaller)));
		}
		Ok(())
	}

	#[tokio::test]
	async fn zero_minimum_fomod_groups_without_selectable_options_offer_none() -> StdResult<(), Box<dyn Error>> {
		for cardinality in [FomodCardinality::SelectAtMostOne, FomodCardinality::SelectAny] {
			let installer = fomod_installer(vec![FomodGroup {
				id: "choice".into(),
				label: "Choice".into(),
				description: String::new(),
				cardinality,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option("not-usable", ResolvedOptionType::NotUsable, &[])],
			}]);

			let output = install_archive(
				fomod_dependencies(installer, HashMap::new()),
				ArchivePath::new(temp_dir().join(format!("zero-minimum-{cardinality:?}.zip")))
					.map_err(|_| "archive")?,
				None,
				false,
				Vec::new(),
				true,
				CancellationToken::new(),
			)
			.await
			.map_err(|_| "evaluate")?;

			let InstallArchiveOutput::AdditionalSelectionsRequired(required) = output else {
				return Err("expected none decision".into());
			};
			let options = &required.unresolved_groups[0].options;
			assert_eq!(options.len(), 2);
			assert!(options.iter().any(|option| {
				option.id == "not-usable" && !option.selectable && !option.synthetic
			}));
			assert!(options
				.iter()
				.any(|option| option.id == "none" && option.selectable && option.synthetic));
		}
		Ok(())
	}

	#[tokio::test]
	async fn not_usable_fomod_options_do_not_count_toward_select_all_cardinality() -> StdResult<(), Box<dyn Error>>
	{
		let mut available = fomod_option("available", ResolvedOptionType::Optional, &[]);
		let mut available_candidate = candidate();
		available_candidate.origin = InstallCandidateOrigin::Option {
			group_id: "all".into(),
			option_id: "available".into(),
			trigger: OptionFileTrigger::Selected,
		};
		available.file_candidates.push(available_candidate);
		let mut unavailable = fomod_option("unavailable", ResolvedOptionType::NotUsable, &[]);
		let mut unavailable_candidate = candidate();
		unavailable_candidate.candidate_id = 2;
		unavailable_candidate.origin = InstallCandidateOrigin::Option {
			group_id: "all".into(),
			option_id: "unavailable".into(),
			trigger: OptionFileTrigger::Selected,
		};
		unavailable.file_candidates.push(unavailable_candidate);
		let installer = fomod_installer(vec![FomodGroup {
			id: "all".into(),
			label: "All".into(),
			description: String::new(),
			cardinality: FomodCardinality::SelectAll,
			condition: FomodCondition::Constant(true),
			options: vec![available, unavailable],
		}]);

		let output = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("select-all-cardinality.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "evaluate")?;

		let InstallArchiveOutput::Preview(preview) = output else {
			return Err("expected complete preview".into());
		};
		assert_eq!(
			preview.plan
				.candidates
				.iter()
				.map(|candidate| candidate.candidate.candidate_id)
				.collect::<Vec<_>>(),
			[1]
		);
		Ok(())
	}

	#[tokio::test]
	async fn conflicting_flags_report_values_and_writer_provenance() -> StdResult<(), Box<dyn Error>> {
		let installer = fomod_installer(vec![
			FomodGroup {
				id: "first".into(),
				label: "First".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option(
					"legacy",
					ResolvedOptionType::Optional,
					&[("mode", "legacy")],
				)],
			},
			FomodGroup {
				id: "second".into(),
				label: "Second".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option(
					"modern",
					ResolvedOptionType::Optional,
					&[("mode", "modern")],
				)],
			},
		]);
		let choices = vec![
			FomodChoice {
				group_id: "first".into(),
				option_id: "legacy".into(),
			},
			FomodChoice {
				group_id: "second".into(),
				option_id: "modern".into(),
			},
		];

		let output = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("conflicting-flags.zip")).map_err(|_| "archive")?,
			None,
			false,
			choices,
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "evaluate")?;

		let InstallArchiveOutput::Preview(preview) = output else {
			return Err("expected preview".into());
		};
		assert_eq!(preview.plan.accepted_choices.len(), 2);
		let Some(InstallWarning::FomodConflictingFlagValues {
			flag_name,
			values,
			resolved_value,
			writers,
			winning_event,
		}) = preview.plan.warnings.first()
		else {
			return Err("missing conflicting flag warning".into());
		};
		assert_eq!(flag_name, "mode");
		assert_eq!(values, &["legacy", "modern"]);
		assert_eq!(resolved_value, "modern");
		assert_eq!(writers.len(), 2);
		assert_eq!(writers[0].sequence, 0);
		assert_eq!(winning_event.sequence, 1);
		assert_eq!(winning_event.group_id, "second");
		assert_eq!(winning_event.option_id, "modern");
		Ok(())
	}

	#[tokio::test]
	async fn later_flag_hiding_group_rejects_prior_synthetic_none_event() -> StdResult<(), Box<dyn Error>> {
		let installer = fomod_installer(vec![
			FomodGroup {
				id: "automatic".into(),
				label: "Automatic".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option("required", ResolvedOptionType::Required, &[])],
			},
			FomodGroup {
				id: "conditional".into(),
				label: "Conditional".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectAtMostOne,
				condition: FomodCondition::FlagDependency {
					name: "mode".into(),
					value: String::new(),
				},
				options: vec![fomod_option("optional", ResolvedOptionType::Optional, &[])],
			},
			FomodGroup {
				id: "control".into(),
				label: "Control".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option(
					"hide",
					ResolvedOptionType::Optional,
					&[("mode", "hidden")],
				)],
			},
		]);

		let result = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("none-group-hidden.zip")).map_err(|_| "archive")?,
			None,
			false,
			vec![
				FomodChoice {
					group_id: "conditional".into(),
					option_id: "none".into(),
				},
				FomodChoice {
					group_id: "control".into(),
					option_id: "hide".into(),
				},
			],
			true,
			CancellationToken::new(),
		)
		.await;

		let report = result.expect_err("hidden group must invalidate its prior none choice");
		let marker = report
			.iter_reports()
			.find_map(|report| report.downcast_current_context::<ErrorMarker>())
			.ok_or("marker")?;
		assert_eq!(marker.code(), ErrorCode::InvalidSelection);
		assert_eq!(marker.field(), Some("choices"));
		assert_eq!(marker.group_id(), Some("conditional"));
		assert_eq!(marker.option_id(), Some("none"));
		assert_eq!(marker.supplied_sequence(), Some(1));
		Ok(())
	}

	#[tokio::test]
	async fn later_required_selection_rejects_prior_synthetic_none_event() -> StdResult<(), Box<dyn Error>> {
		let mut conditional = fomod_option("conditional", ResolvedOptionType::Optional, &[]);
		conditional.type_patterns.push(FomodOptionTypePattern {
			condition: FomodCondition::FlagDependency {
				name: "enable".into(),
				value: "yes".into(),
			},
			option_type: ResolvedOptionType::Required,
		});
		let installer = fomod_installer(vec![
			FomodGroup {
				id: "conditional".into(),
				label: "Conditional".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectAtMostOne,
				condition: FomodCondition::Constant(true),
				options: vec![conditional],
			},
			FomodGroup {
				id: "control".into(),
				label: "Control".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option(
					"enable",
					ResolvedOptionType::Optional,
					&[("enable", "yes")],
				)],
			},
		]);

		let result = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("none-becomes-required.zip")).map_err(|_| "archive")?,
			None,
			false,
			vec![
				FomodChoice {
					group_id: "conditional".into(),
					option_id: "none".into(),
				},
				FomodChoice {
					group_id: "control".into(),
					option_id: "enable".into(),
				},
			],
			true,
			CancellationToken::new(),
		)
		.await;

		let report = result.expect_err("automatic real selection must conflict with prior none");
		let marker = report
			.iter_reports()
			.find_map(|report| report.downcast_current_context::<ErrorMarker>())
			.ok_or("marker")?;
		assert_eq!(marker.code(), ErrorCode::InvalidSelection);
		assert_eq!(marker.field(), Some("choices"));
		assert_eq!(marker.group_id(), Some("conditional"));
		assert_eq!(marker.option_id(), Some("none"));
		assert_eq!(marker.supplied_sequence(), Some(0));
		Ok(())
	}

	#[tokio::test]
	async fn withdrawn_automatic_selection_returns_the_now_incomplete_group() -> StdResult<(), Box<dyn Error>> {
		let mut automatic = fomod_option("auto", ResolvedOptionType::Required, &[]);
		automatic.type_patterns.push(FomodOptionTypePattern {
			condition: FomodCondition::FlagDependency {
				name: "disable".into(),
				value: "yes".into(),
			},
			option_type: ResolvedOptionType::Optional,
		});
		let installer = fomod_installer(vec![
			FomodGroup {
				id: "automatic".into(),
				label: "Automatic".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectAtMostOne,
				condition: FomodCondition::Constant(true),
				options: vec![automatic],
			},
			FomodGroup {
				id: "control".into(),
				label: "Control".into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![fomod_option(
					"disable",
					ResolvedOptionType::Optional,
					&[("disable", "yes")],
				)],
			},
		]);

		let output = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("withdrawal.zip")).map_err(|_| "archive")?,
			None,
			false,
			vec![FomodChoice {
				group_id: "control".into(),
				option_id: "disable".into(),
			}],
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "evaluate")?;

		let InstallArchiveOutput::AdditionalSelectionsRequired(required) = output else {
			return Err("expected additional selections".into());
		};
		assert_eq!(required.unresolved_groups.len(), 1);
		assert_eq!(required.unresolved_groups[0].id, "automatic");
		assert_eq!(
			required.unresolved_groups[0].options[0].resolved_type,
			ResolvedOptionType::Optional
		);
		assert!(required.unresolved_groups[0].options[0].selectable);
		Ok(())
	}

	#[tokio::test]
	async fn oscillating_automatic_fomod_state_is_unsupported() -> StdResult<(), Box<dyn Error>> {
		let mut automatic = fomod_option("auto", ResolvedOptionType::Required, &[("on", "yes")]);
		automatic.type_patterns.push(FomodOptionTypePattern {
			condition: FomodCondition::FlagDependency {
				name: "on".into(),
				value: "yes".into(),
			},
			option_type: ResolvedOptionType::Optional,
		});
		let installer = fomod_installer(vec![FomodGroup {
			id: "automatic".into(),
			label: "Automatic".into(),
			description: String::new(),
			cardinality: FomodCardinality::SelectAtMostOne,
			condition: FomodCondition::Constant(true),
			options: vec![automatic],
		}]);

		let result = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("oscillation.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await;

		let report = result.expect_err("oscillation must fail");
		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::UnsupportedInstaller)));
		Ok(())
	}

	#[tokio::test]
	async fn file_game_nvse_and_fomm_conditions_evaluate_through_install() -> StdResult<(), Box<dyn Error>> {
		let mut installer = fomod_installer(Vec::new());
		installer.module_condition = FomodCondition::All(vec![
			FomodCondition::FileDependency {
				path: "plugin.esp".into(),
				state: FileDependencyState::Active,
			},
			FomodCondition::GameDependency {
				minimum_version: "1.4".into(),
			},
			FomodCondition::NvseDependency {
				minimum_version: "6.3".into(),
			},
			FomodCondition::FommDependency {
				minimum_version: "999".into(),
			},
		]);
		let files = HashMap::from([(
			"plugin.esp".into(),
			FileDependencyFact {
				kind: FileDependencyKind::Plugin,
				state: FileDependencyState::Active,
			},
		)]);

		let output = install_archive(
			fomod_dependencies(installer, files),
			ArchivePath::new(temp_dir().join("conditions.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "evaluate")?;

		let InstallArchiveOutput::Preview(preview) = output else {
			return Err("expected preview".into());
		};
		assert!(preview.plan.warnings.iter().any(|warning| matches!(
			warning,
			InstallWarning::FomodFommDependencyAssumedCompatible { minimum_version }
				if minimum_version == "999"
		)));
		Ok(())
	}

	#[tokio::test]
	async fn plugin_file_dependency_extensions_are_case_insensitive_application_policy()
	-> StdResult<(), Box<dyn Error>> {
		for path in ["Plugin.ESP", "Plugin.EsM", "Plugin.ESL"] {
			let mut installer = fomod_installer(Vec::new());
			installer.module_condition = FomodCondition::FileDependency {
				path: path.to_owned(),
				state: FileDependencyState::Inactive,
			};

			let result = install_archive(
				fomod_dependencies(installer, HashMap::new()),
				ArchivePath::new(temp_dir().join(format!("{path}.zip"))).map_err(|_| "archive")?,
				None,
				false,
				Vec::new(),
				true,
				CancellationToken::new(),
			)
			.await;

			let report = result.expect_err("missing inactive plugin must fail its dependency");
			assert!(report.iter_reports().any(|report| report
				.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::DependencyUnsatisfied)));
		}
		Ok(())
	}

	#[tokio::test]
	async fn normalization_distinct_destinations_keep_separate_planning_winners() -> StdResult<(), Box<dyn Error>> {
		let candidates = [(1_u64, "textures/ﬃ.dds"), (2_u64, "textures/ffi.dds")]
			.into_iter()
			.map(|(order, destination)| {
				Ok(InstallCandidate {
					candidate_id: order,
					origin: InstallCandidateOrigin::Required,
					phase: InstallationPhase::Required,
					declared_priority: 0,
					descriptor_order: order,
					source_member: format!("{order}.dds"),
					destination: DataRelativePath::new(destination.to_owned())
						.map_err(|_| "path")?,
				})
			})
			.collect::<StdResult<Vec<_>, Box<dyn Error>>>()?;
		let mut dependencies =
			dependencies(Arc::new(Mutex::new(Vec::new())), false, Arc::new(AtomicUsize::new(0)));
		dependencies.index_archive = Arc::new(move |_, _, _| {
			let candidates = candidates.clone();
			Box::pin(async move {
				Ok(ArchiveIndex {
					identity: identity(),
					installer: IndexedInstaller::Plain {
						candidates,
						warnings: Vec::new(),
					},
				})
			}) as PortFuture<_>
		});

		let output = install_archive(
			dependencies,
			ArchivePath::new(temp_dir().join("nfkc.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "preview")?;

		let InstallArchiveOutput::Preview(preview) = output else {
			return Err("expected preview".into());
		};
		assert!(preview
			.plan
			.candidates
			.iter()
			.all(|candidate| matches!(candidate.decision, CandidateDecision::Winner { .. })));
		Ok(())
	}

	#[tokio::test]
	async fn later_equal_priority_warning_reaches_installed_output() -> StdResult<(), Box<dyn Error>> {
		let destination = DataRelativePath::new("textures/a.dds".into()).map_err(|_| "path")?;
		let candidates = [1_u64, 2]
			.into_iter()
			.map(|order| InstallCandidate {
				candidate_id: order,
				origin: InstallCandidateOrigin::Required,
				phase: InstallationPhase::SelectedOrForced,
				declared_priority: 5,
				descriptor_order: order,
				source_member: format!("{order}.dds"),
				destination: destination.clone(),
			})
			.collect::<Vec<_>>();
		let mut dependencies =
			dependencies(Arc::new(Mutex::new(Vec::new())), false, Arc::new(AtomicUsize::new(0)));
		dependencies.index_archive = Arc::new(move |_, _, _| {
			let candidates = candidates.clone();
			Box::pin(async move {
				Ok(ArchiveIndex {
					identity: identity(),
					installer: IndexedInstaller::Plain {
						candidates,
						warnings: Vec::new(),
					},
				})
			}) as PortFuture<_>
		});

		let output = install_archive(
			dependencies,
			ArchivePath::new(temp_dir().join("tie.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			false,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "install")?;

		let InstallArchiveOutput::Installed(installed) = output else {
			return Err("expected installed output".into());
		};
		assert!(matches!(
			installed.warnings.as_slice(),
			[InstallWarning::FomodEqualPriorityTieResolved {
				winner_candidate_id: 2,
				..
			}]
		));
		Ok(())
	}

	#[tokio::test]
	async fn hidden_fomod_step_contributes_only_forced_usable_files() -> StdResult<(), Box<dyn Error>> {
		fn option_candidate(
			id: u64,
			trigger: OptionFileTrigger,
		) -> StdResult<InstallCandidate, Box<dyn Error>> {
			Ok(InstallCandidate {
				candidate_id: id,
				origin: InstallCandidateOrigin::Option {
					group_id: "hidden".into(),
					option_id: "choice".into(),
					trigger,
				},
				phase: InstallationPhase::SelectedOrForced,
				declared_priority: 0,
				descriptor_order: id,
				source_member: format!("{id}.bin"),
				destination: DataRelativePath::new(format!("{id}.bin")).map_err(|_| "path")?,
			})
		}

		let mut usable = fomod_option("usable", ResolvedOptionType::Required, &[("hidden", "value")]);
		usable.file_candidates = vec![
			option_candidate(1, OptionFileTrigger::Selected)?,
			option_candidate(2, OptionFileTrigger::AlwaysInstall)?,
			option_candidate(3, OptionFileTrigger::InstallIfUsable)?,
		];
		let mut unusable = fomod_option("unusable", ResolvedOptionType::NotUsable, &[]);
		unusable.file_candidates = vec![option_candidate(4, OptionFileTrigger::InstallIfUsable)?];
		let installer = fomod_installer(vec![FomodGroup {
			id: "hidden".into(),
			label: "Hidden".into(),
			description: String::new(),
			cardinality: FomodCardinality::SelectAll,
			condition: FomodCondition::Constant(false),
			options: vec![usable, unusable],
		}]);

		let output = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("hidden-forced.zip")).map_err(|_| "archive")?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "preview")?;

		let InstallArchiveOutput::Preview(preview) = output else {
			return Err("expected preview".into());
		};
		assert_eq!(
			preview.plan
				.candidates
				.iter()
				.map(|candidate| candidate.candidate.candidate_id)
				.collect::<Vec<_>>(),
			[2, 3]
		);
		assert!(preview.plan.accepted_choices.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn invalid_selection_reports_safe_choice_details_and_total_sequence() -> StdResult<(), Box<dyn Error>> {
		let installer = fomod_installer(vec![FomodGroup {
			id: "known".into(),
			label: "Known".into(),
			description: String::new(),
			cardinality: FomodCardinality::SelectExactlyOne,
			condition: FomodCondition::Constant(true),
			options: vec![fomod_option("required", ResolvedOptionType::Required, &[])],
		}]);
		let result = install_archive(
			fomod_dependencies(installer, HashMap::new()),
			ArchivePath::new(temp_dir().join("invalid-choice.zip")).map_err(|_| "archive")?,
			None,
			false,
			vec![FomodChoice {
				group_id: "missing".into(),
				option_id: "unknown".into(),
			}],
			true,
			CancellationToken::new(),
		)
		.await;

		let report = result.expect_err("selection must fail");
		let marker = report
			.iter_reports()
			.find_map(|report| report.downcast_current_context::<ErrorMarker>())
			.ok_or("marker")?;
		assert_eq!(marker.field(), Some("choices"));
		assert_eq!(marker.group_id(), Some("missing"));
		assert_eq!(marker.option_id(), Some("unknown"));
		assert_eq!(marker.supplied_sequence(), Some(1));
		Ok(())
	}

	#[tokio::test]
	async fn preview_keeps_unrelated_tombstone_conflicts_from_full_namespace() -> Result<()> {
		let mut dependencies =
			dependencies(Arc::new(Mutex::new(Vec::new())), false, Arc::new(AtomicUsize::new(0)));
		let path = DataRelativePath::new("unrelated.txt".to_owned())?;
		let scan = EnvironmentConflictScan {
			providers: vec![
				ScannedConflictProvider {
					identity: ProviderIdentity::SteamData,
					enabled: true,
					files: vec![IndexedConflictFile {
						id: IndexedConflictFileId::new(
							ProviderIdentity::SteamData,
							path.clone(),
						),
						provider: ProviderReference::SteamData {
							original_path: path.clone(),
						},
					}],
					directories: Vec::new(),
					tombstones: Vec::new(),
					problems: Vec::new(),
				},
				ScannedConflictProvider {
					identity: ProviderIdentity::Overwrite,
					enabled: true,
					files: Vec::new(),
					directories: Vec::new(),
					problems: Vec::new(),
					tombstones: vec![Tombstone {
						scope: TombstoneScope::ExactFile,
						owner: ProviderReference::Overwrite { original_path: path },
					}],
				},
			],
			problems: Vec::new(),
		};
		dependencies.scan_environment_conflicts = Arc::new(move |_| {
			let scan = scan.clone();
			Box::pin(async move { Ok(scan) }) as PortFuture<_>
		});

		let output = install_archive(
			dependencies,
			ArchivePath::new(temp_dir().join("Plain Mod.zip"))?,
			None,
			false,
			Vec::new(),
			true,
			CancellationToken::new(),
		)
		.await?;
		let InstallArchiveOutput::Preview(preview) = output else {
			return Err(report!("expected preview"));
		};
		assert!(preview.plan.projected_state.overlaps.is_empty());
		assert_eq!(
			preview.hypothetical_enabled_conflicts.resolution_status,
			ResolutionStatus::Exact
		);
		assert!(
			matches!(preview.hypothetical_enabled_conflicts.rows.as_slice(), [ConflictRow::Tombstone { normalized_key, participation: Participation::Hypothetical, .. }] if normalized_key == "unrelated.txt")
		);
		Ok(())
	}
}
