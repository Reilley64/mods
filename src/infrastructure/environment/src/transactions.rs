use crate::profile::MAX_PROFILE_BYTES;
use crate::profile::stage_plugin_maintenance;
use crate::publication::OPERATION_DIRECTORY;
use crate::publication::cleanup_best_effort;
use crate::safe_fs::EntryBudget;
use crate::safe_fs::SafeDir;
use crate::safe_fs::SafeFile;
use crate::safe_fs::read_bounded;
use crate::safe_fs::sync_tree;
use crate::safe_fs::validate_exact_entries;
use crate::snapshot::MAX_PROVIDER_ENTRIES;
use crate::snapshot::append_disabled_mod;
use crate::snapshot::load_during_publication;
use crate::snapshot::validate_prospective_namespace;
use crate::snapshot::validate_staged_provider;
use application::ErrorCode;
use application::ErrorMarker;
use application::installation::ApprovedInstallation;
use application::installation::CandidateDecision;
use application::installation::InstallMode;
use application::installation::InstallPlan;
use application::installation::InstallWarning;
use domain::ArchiveIdentity;
use domain::DataRelativePath;
use domain::InstalledMod;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Serialize;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use toml::to_string_pretty;

pub(crate) struct InstallationTransaction {
	root_path: PathBuf,
	approved: ApprovedInstallation,
	remaining: HashSet<String>,
	in_progress: HashSet<String>,
	poisoned: bool,
	finished: bool,
}

impl InstallationTransaction {
	pub(crate) fn begin(
		root_path: &Path,
		approved: ApprovedInstallation,
		cancellation: &CancellationToken,
	) -> Result<Self, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let root = SafeDir::open_absolute(root_path).map_err(|error| {
			if error.current_context().kind() == io::ErrorKind::NotFound {
				error.context(ErrorMarker::environment_not_initialized())
			} else {
				error.context(ErrorMarker::environment_root_unsafe())
			}
		})?;
		let temp = root.open_dir("temp").context(ErrorMarker::environment_invalid(None))?;
		let operation = match temp.create_dir(OPERATION_DIRECTORY) {
			Ok(operation) => operation,
			Err(error) if error.current_context().kind() == io::ErrorKind::AlreadyExists => {
				return Err(error.context(ErrorMarker::manual_cleanup_required()));
			}
			Err(error) => return Err(error.context(ErrorMarker::transaction_failure())),
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let current = load_during_publication(root_path, cancellation)?;
		validate_intent(&current.installed_mods, &approved.plan)?;

		let stage = operation
			.create_dir("stage")
			.context(ErrorMarker::transaction_failure())?;
		stage.create_dir("mod").context(ErrorMarker::transaction_failure())?;
		stage.create_dir("profile")
			.context(ErrorMarker::transaction_failure())?;
		operation
			.create_dir("backup")
			.context(ErrorMarker::transaction_failure())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let remaining = approved
			.plan
			.candidates
			.iter()
			.filter(|candidate| matches!(candidate.decision, CandidateDecision::Winner { .. }))
			.map(|candidate| candidate.candidate.destination.comparison_key().to_owned())
			.collect();
		Ok(Self {
			root_path: root_path.to_owned(),
			approved,
			remaining,
			in_progress: HashSet::new(),
			poisoned: false,
			finished: false,
		})
	}

	pub(crate) fn begin_file(
		&mut self,
		path: &DataRelativePath,
		cancellation: &CancellationToken,
	) -> Result<SafeFile, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let key = path.comparison_key().to_owned();
		if self.finished
			|| self.poisoned || path.as_str().eq_ignore_ascii_case("meta.toml")
			|| path.as_str().eq_ignore_ascii_case("Fallout - Invalidation.bsa")
			|| !self.remaining.remove(&key)
		{
			return Err(report!(ErrorMarker::transaction_failure()));
		}

		let result = (|| {
			let root =
				SafeDir::open_absolute(&self.root_path).context(ErrorMarker::transaction_failure())?;
			let temp = root.open_dir("temp").context(ErrorMarker::transaction_failure())?;
			let operation = temp
				.open_dir(OPERATION_DIRECTORY)
				.context(ErrorMarker::transaction_failure())?;
			let stage = operation
				.open_dir("stage")
				.context(ErrorMarker::transaction_failure())?;
			let mut directory = stage.open_dir("mod").context(ErrorMarker::transaction_failure())?;
			let components = path.components().collect::<Vec<_>>();
			let Some((file_name, parents)) = components.split_last() else {
				return Err(report!(ErrorMarker::transaction_failure()));
			};
			for component in parents {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				directory = open_or_create_exact(&directory, component, cancellation)?;
			}
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

			directory
				.create_new_file(file_name)
				.context(ErrorMarker::transaction_failure())
		})();
		let Ok(file) = result else {
			self.poisoned = true;
			return result;
		};

		self.in_progress.insert(key);
		Ok(file)
	}

	pub(crate) fn finish_file(&mut self, path_key: &str) -> Result<(), ErrorMarker> {
		if !self.in_progress.remove(path_key) {
			return Err(report!(ErrorMarker::transaction_failure()));
		}
		Ok(())
	}

	pub(crate) fn finish(&mut self, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if self.finished || self.poisoned || !self.remaining.is_empty() || !self.in_progress.is_empty() {
			return Err(report!(ErrorMarker::transaction_failure()));
		}
		self.finished = true;

		let root = SafeDir::open_absolute(&self.root_path).context(ErrorMarker::transaction_failure())?;
		let temp = root.open_dir("temp").context(ErrorMarker::transaction_failure())?;
		let operation = temp
			.open_dir(OPERATION_DIRECTORY)
			.context(ErrorMarker::transaction_failure())?;
		let stage = operation
			.open_dir("stage")
			.context(ErrorMarker::transaction_failure())?;
		let staged_mod = stage.open_dir("mod").context(ErrorMarker::transaction_failure())?;

		write_metadata(&staged_mod, &self.approved)?;
		validate_staged_provider(&staged_mod, cancellation)?;

		let staged_profile = stage.open_dir("profile").context(ErrorMarker::transaction_failure())?;
		let profile = root.open_dir("profile").context(ErrorMarker::transaction_failure())?;
		let mut profile_files = Vec::new();
		if !self.approved.plan.replacement {
			let current_modlist = read_bounded(
				&profile,
				"modlist.txt",
				MAX_PROFILE_BYTES,
				ErrorMarker::transaction_failure(),
				cancellation,
			)?;
			let intended_modlist = append_disabled_mod(&current_modlist, &self.approved.plan.mod_name)?;
			staged_profile
				.write_new("modlist.txt", &intended_modlist)
				.context(ErrorMarker::transaction_failure())?;
			profile_files.push("modlist.txt".to_owned());
		}
		if self.approved.plan.replacement && self.approved.plan.projected_state.enabled {
			profile_files = stage_plugin_maintenance(
				&root,
				&staged_mod,
				&staged_profile,
				&self.approved.plan.mod_name,
				profile_files,
				cancellation,
			)?;
		}
		profile_files.sort_by_key(|name| usize::from(name == "modlist.txt"));

		validate_prospective_namespace(&root, &staged_mod, &self.approved.plan, cancellation)?;
		sync_tree(&staged_mod, ErrorMarker::transaction_failure(), cancellation)?;
		sync_tree(&staged_profile, ErrorMarker::transaction_failure(), cancellation)?;
		stage.sync().context(ErrorMarker::transaction_failure())?;
		operation.sync().context(ErrorMarker::transaction_failure())?;
		temp.sync().context(ErrorMarker::transaction_failure())?;
		root.sync().context(ErrorMarker::transaction_failure())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let mut published_profile_files = Vec::with_capacity(profile_files.len());
		for name in profile_files {
			let expected = read_bounded(
				&staged_profile,
				&name,
				MAX_PROFILE_BYTES,
				ErrorMarker::transaction_failure(),
				cancellation,
			)?;
			published_profile_files.push(PublishedProfileFile { name, expected });
		}

		drop(profile);
		drop(staged_profile);
		drop(staged_mod);
		drop(stage);
		drop(operation);
		publish_installation(
			&self.root_path,
			&root,
			&temp,
			&self.approved.plan,
			&published_profile_files,
			cancellation,
		)?;
		Ok(())
	}
}

fn validate_intent(installed: &[InstalledMod], plan: &InstallPlan) -> Result<(), ErrorMarker> {
	let existing = installed.iter().find(|item| item.name == plan.mod_name);
	match (plan.replacement, existing) {
		(false, None) => {
			let expected = u32::try_from(installed.len()).context(ErrorMarker::transaction_failure())?;
			if plan.projected_state.mode != InstallMode::NewInstall
				|| plan.projected_state.mod_name != plan.mod_name
				|| plan.projected_state.enabled
				|| plan.projected_state.priority.get() != expected
				|| plan.projected_state.list_position != u64::from(expected)
			{
				return Err(report!(ErrorMarker::transaction_failure()));
			}
		}
		(true, Some(existing)) => {
			if plan.projected_state.mode != InstallMode::Replacement
				|| plan.mod_name.as_str() != existing.name.as_str()
				|| plan.projected_state.mod_name != existing.name
				|| plan.projected_state.list_position != u64::from(existing.priority.get())
				|| plan.projected_state.enabled != existing.enabled
				|| plan.projected_state.priority != existing.priority
			{
				return Err(report!(ErrorMarker::transaction_failure()));
			}
		}
		_ => return Err(report!(ErrorMarker::transaction_failure())),
	}
	Ok(())
}

fn open_or_create_exact(
	directory: &SafeDir,
	name: &str,
	cancellation: &CancellationToken,
) -> Result<SafeDir, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::transaction_failure())?;
	let mut budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
	let mut directory_entries = 0_usize;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let entry = entry.into_report().context(ErrorMarker::transaction_failure())?;
		budget.consume(&mut directory_entries)
			.context(ErrorMarker::transaction_failure())?;
		let existing = entry.file_name();
		let existing = existing
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::transaction_failure()))?;
		if case_fold_key(existing) == case_fold_key(name) {
			if existing != name {
				return Err(report!(ErrorMarker::transaction_failure()));
			}
			return directory.open_dir(name).context(ErrorMarker::transaction_failure());
		}
	}
	directory.create_dir(name).context(ErrorMarker::transaction_failure())
}

#[derive(Serialize)]
struct InstalledMetadata<'a> {
	schema_version: u32,
	source_basename: &'a str,
	archive_sha256: &'a str,
	package_root: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	installer_config_member: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	installer_config_sha256: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	fomod_schema_version: Option<&'a str>,
	choices: Vec<MetadataChoice<'a>>,
	warnings: Vec<&'static str>,
}

#[derive(Serialize)]
struct MetadataChoice<'a> {
	group_id: &'a str,
	option_id: &'a str,
}

fn write_metadata(mod_dir: &SafeDir, approved: &ApprovedInstallation) -> Result<(), ErrorMarker> {
	let source_basename = approved.source_basename.as_str();
	if source_basename.is_empty() {
		return Err(report!(ErrorMarker::transaction_failure()));
	}
	let identity = &approved.plan.archive_identity;
	let (config_member, config_sha256) = match identity {
		ArchiveIdentity::DataArchive { .. } => (None, None),
		ArchiveIdentity::Fomod {
			config_member,
			config_sha256,
			..
		} => (Some(config_member.as_str()), Some(config_sha256.as_str())),
	};
	let choices = approved
		.plan
		.accepted_choices
		.iter()
		.map(|event| MetadataChoice {
			group_id: &event.group_id,
			option_id: &event.option_id,
		})
		.collect();
	let warnings = approved.plan.warnings.iter().map(warning_name).collect();
	let metadata = InstalledMetadata {
		schema_version: 1,
		source_basename,
		archive_sha256: identity.archive_sha256().as_str(),
		package_root: identity.package_root(),
		installer_config_member: config_member,
		installer_config_sha256: config_sha256,
		fomod_schema_version: approved.fomod_schema_version.as_deref(),
		choices,
		warnings,
	};
	let text = to_string_pretty(&metadata).context(ErrorMarker::transaction_failure())?;
	mod_dir.write_new("meta.toml", text.as_bytes())
		.context(ErrorMarker::transaction_failure())
}

fn warning_name(warning: &InstallWarning) -> &'static str {
	match warning {
		InstallWarning::FomodCouldBeUsableSelected { .. } => "fomod_could_be_usable_selected",
		InstallWarning::FomodConflictingFlagValues { .. } => "fomod_conflicting_flag_values",
		InstallWarning::FomodEmptyConditionList { .. } => "fomod_empty_condition_list",
		InstallWarning::FomodMalformedGroupRepaired { .. } => "fomod_malformed_group_repaired",
		InstallWarning::FomodFommDependencyAssumedCompatible { .. } => {
			"fomod_fomm_dependency_assumed_compatible"
		}
		InstallWarning::FomodEmptySourceIgnored { .. } => "fomod_empty_source_ignored",
		InstallWarning::FomodEmptyOptionAccepted { .. } => "fomod_empty_option_accepted",
		InstallWarning::FomodModuleConfigPreferred { .. } => "fomod_module_config_preferred",
		InstallWarning::FomodNonPluginFileDependencyPolicyUsed { .. } => {
			"fomod_non_plugin_file_dependency_policy_used"
		}
		InstallWarning::FomodEqualPriorityTieResolved { .. } => "fomod_equal_priority_tie_resolved",
	}
}

#[derive(Debug)]
struct PublishedProfileFile {
	name: String,
	expected: Vec<u8>,
}

fn publish_installation(
	root_path: &Path,
	root: &SafeDir,
	temp: &SafeDir,
	plan: &InstallPlan,
	profile_files: &[PublishedProfileFile],
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let operation = temp.open_dir(OPERATION_DIRECTORY).context(publication_failed())?;
	let stage = operation.open_dir("stage").context(publication_failed())?;
	let staged_profile = stage.open_dir("profile").context(publication_failed())?;
	let backup = operation.open_dir("backup").context(publication_failed())?;
	let mods = root.open_dir("mods").context(publication_failed())?;
	let profile = root.open_dir("profile").context(publication_failed())?;
	let mod_name = plan.mod_name.as_str();

	let backup_validation = validate_exact_entries(&backup, &[], cancellation);
	if let Err(error) = backup_validation {
		if error.current_context().code() == ErrorCode::OperationCancelled {
			return Err(error);
		}
		return Err(error.context(publication_failed()));
	}
	stage.open_dir("mod").context(publication_failed())?;
	let canonical_mod_exists = mods.exists(mod_name).context(publication_failed())?;
	if canonical_mod_exists != plan.replacement {
		return Err(report!(publication_failed()));
	}
	for file in profile_files {
		profile.open_regular(&file.name).context(publication_failed())?;
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	if plan.replacement {
		mods.rename_durable_to(mod_name, &backup, "mod")
			.context(publication_failed())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
	}

	stage.rename_durable_to("mod", &mods, mod_name)
		.context(publication_failed())?;

	for file in profile_files {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		profile.rename_durable_to(&file.name, &backup, &file.name)
			.context(publication_failed())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		staged_profile
			.rename_durable_to(&file.name, &profile, &file.name)
			.context(publication_failed())?;
	}

	// The mod is final when no profile file changed. Otherwise, the last changed profile
	// file is final. Caller cancellation is intentionally not observed after either rename.
	drop(profile);
	drop(mods);
	drop(backup);
	drop(staged_profile);
	drop(stage);
	drop(operation);
	finish_committed_installation(root_path, root, temp, plan, profile_files)
}

fn finish_committed_installation(
	root_path: &Path,
	root: &SafeDir,
	temp: &SafeDir,
	plan: &InstallPlan,
	profile_files: &[PublishedProfileFile],
) -> Result<(), ErrorMarker> {
	let validation_cancellation = CancellationToken::new();
	let snapshot = load_during_publication(root_path, &validation_cancellation).context(publication_failed())?;
	let installed = snapshot
		.installed_mods
		.iter()
		.find(|installed| installed.name == plan.mod_name)
		.ok_or_else(|| report!(publication_failed()))?;
	if installed.priority != plan.projected_state.priority || installed.enabled != plan.projected_state.enabled {
		return Err(report!(publication_failed()));
	}

	let profile = root.open_dir("profile").context(publication_failed())?;
	for file in profile_files {
		let canonical = read_bounded(
			&profile,
			&file.name,
			MAX_PROFILE_BYTES,
			publication_failed(),
			&validation_cancellation,
		)?;
		if canonical != file.expected {
			return Err(report!(publication_failed()));
		}
	}

	drop(profile);
	cleanup_best_effort(root, temp, OPERATION_DIRECTORY);
	Ok(())
}

fn publication_failed() -> ErrorMarker {
	ErrorMarker::environment_publication_failed(Some("publication"))
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
mod tests {
	use super::InstallationTransaction;
	use super::PublishedProfileFile;
	use super::finish_committed_installation;
	use super::open_or_create_exact;
	use super::publish_installation;
	use crate::EnvironmentAdapter;
	use crate::profile::PROFILE_FILES;
	use crate::safe_fs::SafeDir;
	use application::ErrorCode;
	use application::ErrorMarker;
	use application::installation::AcceptedChoice;
	use application::installation::ApprovedInstallation;
	use application::installation::CandidateDecision;
	use application::installation::EffectiveResult;
	use application::installation::InstallMode;
	use application::installation::InstallPlan;
	use application::installation::PlanCandidateReference;
	use application::installation::PlannedCandidate;
	use application::installation::ProjectedModState;
	use application::installation::WinnerReason;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::InstallationStateAccess;
	use application::ports::ProfileSource;
	use domain::ArchiveIdentity;
	use domain::DataRelativePath;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::InstallCandidate;
	use domain::InstallCandidateOrigin;
	use domain::InstallationPhase;
	use domain::ModName;
	use domain::ModPriority;
	use domain::Sha256Digest;
	use domain::SteamBuildId;
	use rootcause::Result;
	use std::env::current_dir;
	use std::ffi::OsStr;
	use std::fs;
	use std::path::Path;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	fn temp_dir() -> TempDir {
		TempDir::new_in(current_dir().expect("current dir must be available"))
			.expect("temp dir must be created")
	}

	fn environment_root(path: &Path) -> EnvironmentRoot {
		EnvironmentRoot::new(path.to_path_buf()).expect("test environment root must be valid")
	}

	fn initialization_plan(game: &Path) -> InitializationPlan {
		let files = PROFILE_FILES
			.into_iter()
			.map(|name| ProfileSource { name, contents: None })
			.collect();
		InitializationPlan {
			game_binding: GameBinding::new(
				GameInstallationPath::new(game.to_path_buf()).expect("test game path must be valid"),
				SteamBuildId::new(7).expect("test build ID must be valid"),
			),
			profile_sources: InitializationProfileSources {
				files,
				fallout_default_ini: b"[Archive]\r\nsArchiveList=Fallout - Meshes.bsa\r\n".to_vec(),
			},
		}
	}

	fn approved_installation(
		archive: &Path,
		name: &str,
		replacement: bool,
		enabled: bool,
		priority: u32,
		destination: &str,
	) -> ApprovedInstallation {
		let mod_name = ModName::new(name.to_owned()).expect("fixture mod name must be valid");
		ApprovedInstallation {
			source_basename: archive
				.file_name()
				.and_then(OsStr::to_str)
				.expect("fixture archive basename must be Unicode")
				.to_owned(),
			fomod_schema_version: None,
			plan: InstallPlan {
				archive_identity: ArchiveIdentity::DataArchive {
					archive_sha256: Sha256Digest::new("a".repeat(64))
						.expect("fixture hash must be valid"),
					package_root: String::new(),
				},
				mod_name: mod_name.clone(),
				replacement,
				accepted_choices: Vec::new(),
				warnings: Vec::new(),
				candidates: vec![PlannedCandidate {
					candidate: InstallCandidate {
						candidate_id: 0,
						origin: InstallCandidateOrigin::Required,
						phase: InstallationPhase::Required,
						declared_priority: 0,
						descriptor_order: 0,
						source_member: destination.to_owned(),
						destination: DataRelativePath::new(destination.to_owned())
							.expect("fixture destination must be valid"),
					},
					current_winner: EffectiveResult::Absent {
						controlling_tombstone: None,
					},
					proposed_winner: PlanCandidateReference {
						candidate_id: 0,
						source_member: destination.to_owned(),
					},
					decision: CandidateDecision::Winner {
						reason: WinnerReason::OnlyCandidate,
					},
				}],
				projected_state: ProjectedModState {
					mode: if replacement {
						InstallMode::Replacement
					} else {
						InstallMode::NewInstall
					},
					mod_name,
					priority: ModPriority::new(priority),
					list_position: u64::from(priority),
					enabled,
					overlaps: Vec::new(),
				},
			},
		}
	}

	fn initialized_environment(parent: &TempDir) -> EnvironmentRoot {
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.expect("fixture environment must initialize");
		root
	}

	fn stage_file(
		transaction: &mut InstallationTransaction,
		path: &str,
		contents: &[u8],
		cancellation: &CancellationToken,
	) {
		let path = DataRelativePath::new(path.to_owned()).expect("fixture path must be valid");
		let mut file = transaction
			.begin_file(&path, cancellation)
			.expect("fixture file must begin");
		file.write_chunk(contents).expect("fixture file must write");
		file.finish().expect("fixture file must become durable");
		transaction
			.finish_file(path.comparison_key())
			.expect("fixture file must finish");
	}

	fn install(root: &EnvironmentRoot, approved: ApprovedInstallation, path: &str, contents: &[u8]) {
		let cancellation = CancellationToken::new();
		let mut transaction = InstallationTransaction::begin(root.as_path(), approved, &cancellation)
			.expect("fixture transaction must begin");
		stage_file(&mut transaction, path, contents, &cancellation);
		transaction
			.finish(&cancellation)
			.expect("fixture transaction must publish");
	}

	#[expect(clippy::panic, reason = "a successful result must fail this test assertion")]
	fn assert_manual_cleanup_required<T>(result: Result<T, ErrorMarker>) {
		let Err(error) = result else {
			panic!("pending work must require manual cleanup");
		};
		assert_eq!(error.current_context().code(), ErrorCode::ManualCleanupRequired);
	}

	#[test]
	fn pending_operation_refuses_a_later_mutation() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		fs::create_dir(root.as_path().join("temp/operation")).expect("pending operation must be created");
		let archive = parent.path().join("blocked.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");

		assert_manual_cleanup_required(InstallationTransaction::begin(
			root.as_path(),
			approved_installation(&archive, "Blocked", false, false, 0, "blocked.txt"),
			&CancellationToken::new(),
		));
	}

	#[test]
	fn failed_intent_validation_preserves_the_reserved_operation() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("invalid-plan.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let approved = approved_installation(&archive, "Invalid", false, false, 1, "file.txt");

		let error = InstallationTransaction::begin(root.as_path(), approved, &CancellationToken::new())
			.err()
			.expect("invalid intent must fail after reserving publication");

		assert_eq!(error.current_context().code(), ErrorCode::TransactionFailure);
		assert!(root.as_path().join("temp/operation").is_dir());
	}

	#[test]
	fn prior_temp_debris_is_rejected_after_reservation() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("blocked.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let debris = root.as_path().join("temp/unrelated");
		fs::write(&debris, b"keep").expect("prior debris must exist");

		assert_manual_cleanup_required(InstallationTransaction::begin(
			root.as_path(),
			approved_installation(&archive, "Blocked", false, false, 0, "blocked.txt"),
			&CancellationToken::new(),
		));
		assert_eq!(fs::read(debris).expect("prior debris must remain"), b"keep");
		assert!(root.as_path().join("temp/operation").is_dir());
	}

	#[test]
	fn exact_directory_open_rejects_simple_unicode_case_aliases() {
		let parent = temp_dir();
		fs::create_dir(parent.path().join("éς")).expect("existing directory fixture must be created");
		let directory = SafeDir::open_absolute(
			&parent.path()
				.canonicalize()
				.expect("fixture directory must canonicalize"),
		)
		.expect("fixture directory must open");

		assert!(open_or_create_exact(&directory, "ÉΣ", &CancellationToken::new()).is_err());
	}

	#[test]
	fn staging_does_not_mutate_canonical_state() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("staged.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let cancellation = CancellationToken::new();
		let mut transaction = InstallationTransaction::begin(
			root.as_path(),
			approved_installation(&archive, "Staged", false, false, 0, "file.txt"),
			&cancellation,
		)
		.expect("transaction must begin");
		stage_file(&mut transaction, "file.txt", b"staged", &cancellation);

		assert!(!root.as_path().join("mods/Staged").exists());
		assert_eq!(
			fs::read(root.as_path().join("profile/modlist.txt")).expect("modlist must read"),
			b""
		);
		assert!(root.as_path().join("temp/operation/stage/mod/file.txt").is_file());
	}

	#[test]
	fn new_install_publishes_the_mod_then_modlist() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("new.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		install(
			&root,
			approved_installation(&archive, "New", false, false, 0, "file.txt"),
			"file.txt",
			b"contents",
		);

		assert_eq!(
			fs::read(root.as_path().join("mods/New/file.txt")).expect("mod file must read"),
			b"contents"
		);
		assert_eq!(
			fs::read(root.as_path().join("profile/modlist.txt")).expect("modlist must read"),
			b"-New\r\n"
		);
		assert!(fs::read_dir(root.as_path().join("temp"))
			.expect("temp must read")
			.next()
			.is_none());
	}

	#[test]
	fn metadata_keeps_fomod_provenance_without_decorative_installer_fields() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("fomod.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let mut approved = approved_installation(&archive, "Fomod", false, false, 0, "file.txt");
		approved.fomod_schema_version = Some("5.0".to_owned());
		approved.plan.accepted_choices = vec![AcceptedChoice {
			group_id: "core".to_owned(),
			option_id: "standard".to_owned(),
		}];
		approved.plan.archive_identity = ArchiveIdentity::Fomod {
			archive_sha256: Sha256Digest::new("a".repeat(64)).expect("fixture hash must be valid"),
			package_root: String::new(),
			config_member: "fomod/ModuleConfig.xml".to_owned(),
			config_sha256: Sha256Digest::new("b".repeat(64)).expect("fixture hash must be valid"),
		};

		install(&root, approved, "file.txt", b"contents");

		let metadata =
			fs::read_to_string(root.as_path().join("mods/Fomod/meta.toml")).expect("metadata must read");
		assert!(metadata.contains("fomod_schema_version = \"5.0\""));
		assert!(metadata.contains("group_id = \"core\""));
		assert!(metadata.contains("option_id = \"standard\""));
		for trace in [
			"automatic",
			"resolved_flags",
			"winning_sequence",
			"winning_source",
			"sequence =",
			"source =",
			"action =",
		] {
			assert!(!metadata.contains(trace));
		}
		for deferred in ["name", "author", "version", "description", "website", "id"] {
			assert!(!metadata.lines().any(|line| line.starts_with(&format!("{deferred} = "))));
		}
	}

	#[test]
	fn disabled_replacement_preserves_every_profile_file() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("replacement.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		install(
			&root,
			approved_installation(&archive, "Replace", false, false, 0, "Old.ESP"),
			"Old.ESP",
			b"old",
		);
		let before = ["plugins.txt", "loadorder.txt", "modlist.txt"].map(|name| {
			fs::read(root.as_path().join("profile").join(name)).expect("profile file must read")
		});

		install(
			&root,
			approved_installation(&archive, "Replace", true, false, 0, "New.ESP"),
			"New.ESP",
			b"new",
		);

		let after = ["plugins.txt", "loadorder.txt", "modlist.txt"].map(|name| {
			fs::read(root.as_path().join("profile").join(name)).expect("profile file must read")
		});
		assert_eq!(after, before);
		assert!(!root.as_path().join("mods/Replace/Old.ESP").exists());
		assert!(root.as_path().join("mods/Replace/New.ESP").is_file());
	}

	#[test]
	fn enabled_replacement_preserves_modlist_and_updates_only_changed_plugin_files() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("replacement.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		install(
			&root,
			approved_installation(&archive, "Replace", false, false, 0, "Old.ESP"),
			"Old.ESP",
			b"old",
		);
		fs::write(root.as_path().join("profile/modlist.txt"), b"# keep\r\n+Replace\r\n")
			.expect("enabled modlist must write");
		fs::write(root.as_path().join("profile/plugins.txt"), b"# active\r\nOld.ESP\r\n")
			.expect("plugins must write");
		fs::write(root.as_path().join("profile/loadorder.txt"), b"# order\r\nOld.ESP\r\n")
			.expect("load order must write");
		let modlist = fs::read(root.as_path().join("profile/modlist.txt")).expect("modlist must read");

		install(
			&root,
			approved_installation(&archive, "Replace", true, true, 0, "New.ESP"),
			"New.ESP",
			b"new",
		);

		assert_eq!(
			fs::read(root.as_path().join("profile/modlist.txt")).expect("modlist must read"),
			modlist
		);
		assert_eq!(
			fs::read(root.as_path().join("profile/plugins.txt")).expect("plugins must read"),
			b"# active\r\n"
		);
		assert_eq!(
			fs::read(root.as_path().join("profile/loadorder.txt")).expect("load order must read"),
			b"# order\r\nNew.ESP\r\n"
		);
	}

	#[test]
	fn unexpected_backup_entry_is_a_publication_failure_with_its_original_cause() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("new.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let approved = approved_installation(&archive, "New", false, false, 0, "file.txt");
		let cancellation = CancellationToken::new();
		let mut transaction = InstallationTransaction::begin(root.as_path(), approved.clone(), &cancellation)
			.expect("installation must begin");
		stage_file(&mut transaction, "file.txt", b"contents", &cancellation);
		fs::write(root.as_path().join("temp/operation/backup/unexpected"), b"keep")
			.expect("unexpected backup entry must exist");
		let root_dir = SafeDir::open_absolute(root.as_path()).expect("root must open");
		let temp = root_dir.open_dir("temp").expect("temp must open");

		let error = publish_installation(root.as_path(), &root_dir, &temp, &approved.plan, &[], &cancellation)
			.expect_err("unexpected backup entry must stop publication");

		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentPublicationFailed);
		assert!(error.iter_reports().any(|report| {
			report.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::EnvironmentInvalid)
		}));
		assert!(root.as_path().join("temp/operation/backup/unexpected").is_file());
	}

	#[test]
	fn backup_validation_keeps_cancellation_as_the_top_marker() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("new.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let approved = approved_installation(&archive, "New", false, false, 0, "file.txt");
		let cancellation = CancellationToken::new();
		let mut transaction = InstallationTransaction::begin(root.as_path(), approved.clone(), &cancellation)
			.expect("installation must begin");
		stage_file(&mut transaction, "file.txt", b"contents", &cancellation);
		let root_dir = SafeDir::open_absolute(root.as_path()).expect("root must open");
		let temp = root_dir.open_dir("temp").expect("temp must open");
		cancellation.cancel();

		let error = publish_installation(root.as_path(), &root_dir, &temp, &approved.plan, &[], &cancellation)
			.expect_err("cancelled backup validation must stop publication");

		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		assert!(root.as_path().join("temp/operation").is_dir());
	}

	#[test]
	fn a_mid_publication_failure_keeps_stage_and_backups_then_refuses_later_mutation() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("replacement.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		install(
			&root,
			approved_installation(&archive, "Replace", false, false, 0, "Old.txt"),
			"Old.txt",
			b"old",
		);
		let approved = approved_installation(&archive, "Replace", true, false, 0, "New.txt");
		let cancellation = CancellationToken::new();
		let mut transaction = InstallationTransaction::begin(root.as_path(), approved.clone(), &cancellation)
			.expect("replacement must begin");
		stage_file(&mut transaction, "New.txt", b"new", &cancellation);
		let root_dir = SafeDir::open_absolute(root.as_path()).expect("root must open");
		let temp = root_dir.open_dir("temp").expect("temp must open");
		let missing_profile = [PublishedProfileFile {
			name: "modlist.txt".to_owned(),
			expected: b"-Replace\r\n".to_vec(),
		}];

		let result = publish_installation(
			root.as_path(),
			&root_dir,
			&temp,
			&approved.plan,
			&missing_profile,
			&cancellation,
		);
		assert!(result.is_err());
		assert!(root.as_path().join("mods/Replace/New.txt").is_file());
		assert!(root.as_path().join("temp/operation/backup/mod/Old.txt").is_file());
		assert!(root.as_path().join("temp/operation/backup/modlist.txt").is_file());
		assert_manual_cleanup_required(EnvironmentAdapter.load_installation_state(
			&root,
			InstallationStateAccess::Mutation,
			&CancellationToken::new(),
		));
	}

	#[test]
	fn cancellation_preserves_staging_without_canonical_mutation() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("cancelled.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let cancellation = CancellationToken::new();
		let mut transaction = InstallationTransaction::begin(
			root.as_path(),
			approved_installation(&archive, "Cancelled", false, false, 0, "file.txt"),
			&cancellation,
		)
		.expect("transaction must begin");
		stage_file(&mut transaction, "file.txt", b"contents", &cancellation);
		cancellation.cancel();

		let error = transaction
			.finish(&cancellation)
			.expect_err("cancellation must stop publication");
		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		assert!(root.as_path().join("temp/operation/stage/mod/file.txt").is_file());
		assert!(!root.as_path().join("mods/Cancelled").exists());
	}

	#[test]
	fn postcommit_cancellation_is_not_observed() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("committed.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let approved = approved_installation(&archive, "Committed", false, false, 0, "file.txt");
		install(&root, approved.clone(), "file.txt", b"contents");
		fs::create_dir(root.as_path().join("temp/operation")).expect("postcommit operation must exist");
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		assert!(cancellation.is_cancelled());
		let root_dir = SafeDir::open_absolute(root.as_path()).expect("root must open");
		let temp = root_dir.open_dir("temp").expect("temp must open");

		finish_committed_installation(root.as_path(), &root_dir, &temp, &approved.plan, &[])
			.expect("postcommit work must not observe caller cancellation");
		assert!(fs::read_dir(root.as_path().join("temp"))
			.expect("temp must read")
			.next()
			.is_none());
	}

	#[test]
	fn cleanup_failure_after_validation_still_returns_success() {
		let parent = temp_dir();
		let root = initialized_environment(&parent);
		let archive = parent.path().join("committed.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let approved = approved_installation(&archive, "Committed", false, false, 0, "file.txt");
		install(&root, approved.clone(), "file.txt", b"contents");
		fs::write(root.as_path().join("temp/operation"), b"cleanup obstruction")
			.expect("cleanup obstruction must exist");
		let root_dir = SafeDir::open_absolute(root.as_path()).expect("root must open");
		let temp = root_dir.open_dir("temp").expect("temp must open");

		finish_committed_installation(root.as_path(), &root_dir, &temp, &approved.plan, &[])
			.expect("validated commit must succeed despite cleanup failure");
		assert!(root.as_path().join("temp/operation").is_file());
		assert_manual_cleanup_required(EnvironmentAdapter.load_installation_state(
			&root,
			InstallationStateAccess::Mutation,
			&CancellationToken::new(),
		));
	}
}
