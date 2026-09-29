use crate::profile::update_plugin_lists;
use crate::snapshot::insert_disabled_mod;
use crate::snapshot::load;
use crate::snapshot::validate_prospective_namespace;
use crate::snapshot::validate_staged_provider;
use crate::snapshot::visible_plugins;
use application::ErrorMarker;
use application::installation::ApprovedInstallation;
use application::installation::CandidateDecision;
use application::installation::InstallMode;
use application::installation::InstallPlan;
use application::installation::InstallWarning;
use application::ports::InstallationStateAccess;
use domain::ArchiveIdentity;
use domain::DataRelativePath;
use domain::GameBinding;
use domain::InstalledMod;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::fs::create_dir;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio::fs::remove_dir_all;
use tokio::fs::write;
use tokio_util::sync::CancellationToken;
use toml::to_string_pretty;

/// Writes one approved installation directly into `mods/<name>` and the profile.
///
/// There is no staging and no rollback. A failure or cancellation after `begin` can leave a partial
/// mod directory or profile edit; the planned git rollback covers recovery.
pub(crate) struct InstallationTransaction {
	root_path: PathBuf,
	binding: GameBinding,
	approved: ApprovedInstallation,
	plugins_before: Option<HashMap<String, String>>,
	remaining: HashSet<String>,
	in_progress: HashSet<String>,
	poisoned: bool,
	finished: bool,
}

impl InstallationTransaction {
	pub(crate) async fn begin(
		root_path: &Path,
		binding: &GameBinding,
		approved: ApprovedInstallation,
		cancellation: &CancellationToken,
	) -> Result<Self, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
		}

		let current = load(root_path, binding, InstallationStateAccess::Mutation, cancellation).await?;
		validate_intent(&current.installed_mods, &approved.plan)?;

		let plugins_before = if approved.plan.replacement && approved.plan.projected_state.enabled {
			Some(visible_plugins(root_path, binding, cancellation).await?)
		} else {
			None
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
		}

		let mod_directory = root_path.join("mods").join(approved.plan.mod_name.as_str());
		if approved.plan.replacement {
			remove_dir_all(&mod_directory)
				.await
				.context(ErrorMarker::transaction_failure().with_phase("publication"))?;
		}
		create_dir(&mod_directory)
			.await
			.context(ErrorMarker::transaction_failure().with_phase("publication"))?;

		let remaining = approved
			.plan
			.candidates
			.iter()
			.filter(|candidate| matches!(candidate.decision, CandidateDecision::Winner { .. }))
			.map(|candidate| candidate.candidate.destination.comparison_key().to_owned())
			.collect();
		Ok(Self {
			root_path: root_path.to_owned(),
			binding: binding.clone(),
			approved,
			plugins_before,
			remaining,
			in_progress: HashSet::new(),
			poisoned: false,
			finished: false,
		})
	}

	fn mod_directory(&self) -> PathBuf {
		self.root_path.join("mods").join(self.approved.plan.mod_name.as_str())
	}

	pub(crate) async fn begin_file(
		&mut self,
		path: &DataRelativePath,
		cancellation: &CancellationToken,
	) -> Result<File, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
		}

		let key = path.comparison_key().to_owned();
		if self.finished
			|| self.poisoned || path.as_str().eq_ignore_ascii_case("meta.toml")
			|| path.as_str().eq_ignore_ascii_case("Fallout - Invalidation.bsa")
			|| !self.remaining.remove(&key)
		{
			return Err(report!(ErrorMarker::transaction_failure().with_phase("publication")));
		}

		let result = create_mod_file(&self.mod_directory(), path, cancellation).await;
		let Ok(file) = result else {
			self.poisoned = true;
			return result;
		};

		self.in_progress.insert(key);
		Ok(file)
	}

	pub(crate) fn finish_file(&mut self, path_key: &str) -> Result<(), ErrorMarker> {
		if !self.in_progress.remove(path_key) {
			return Err(report!(ErrorMarker::transaction_failure().with_phase("publication")));
		}
		Ok(())
	}

	pub(crate) async fn finish(&mut self, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
		}
		if self.finished || self.poisoned || !self.remaining.is_empty() || !self.in_progress.is_empty() {
			return Err(report!(ErrorMarker::transaction_failure().with_phase("publication")));
		}
		self.finished = true;

		let mod_directory = self.mod_directory();
		write_metadata(&mod_directory, &self.approved).await?;

		validate_staged_provider(&mod_directory, cancellation).await?;
		validate_prospective_namespace(
			&self.root_path,
			&self.binding,
			&mod_directory,
			&self.approved.plan,
			cancellation,
		)
		.await?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
		}

		let modlist = self.root_path.join("profile/modlist.txt");
		if !self.approved.plan.replacement {
			let current_modlist = read(&modlist)
				.await
				.context(ErrorMarker::transaction_failure().with_phase("publication"))?;
			let intended_modlist = insert_disabled_mod(&current_modlist, &self.approved.plan.mod_name)?;
			write(&modlist, &intended_modlist)
				.await
				.context(ErrorMarker::transaction_failure().with_phase("publication"))?;
		}

		if let Some(before) = &self.plugins_before {
			update_plugin_lists(&self.root_path, &self.binding, before, cancellation).await?;
		}

		// The last write is done, so caller cancellation is no longer observed.
		finish_committed_installation(&self.root_path, &self.binding, &self.approved.plan).await
	}
}

async fn create_mod_file(
	mod_directory: &Path,
	path: &DataRelativePath,
	cancellation: &CancellationToken,
) -> Result<File, ErrorMarker> {
	let components = path.components().collect::<Vec<_>>();
	let (file_name, parents) = components
		.split_last()
		.ok_or_else(|| report!(ErrorMarker::transaction_failure().with_phase("publication")))?;

	let mut directory = mod_directory.to_path_buf();
	for component in parents {
		directory = open_or_create_exact(&directory, component, cancellation).await?;
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
	}

	File::create(directory.join(file_name))
		.await
		.context(ErrorMarker::transaction_failure().with_phase("publication"))
}

fn validate_intent(installed: &[InstalledMod], plan: &InstallPlan) -> Result<(), ErrorMarker> {
	let existing = installed.iter().find(|item| item.name == plan.mod_name);
	match (plan.replacement, existing) {
		(false, None) => {
			let expected = u32::try_from(installed.len())
				.context(ErrorMarker::transaction_failure().with_phase("publication"))?;
			if plan.projected_state.mode != InstallMode::NewInstall
				|| plan.projected_state.mod_name != plan.mod_name
				|| plan.projected_state.enabled
				|| plan.projected_state.priority.get() != expected
				|| plan.projected_state.list_position != 0
			{
				return Err(report!(ErrorMarker::transaction_failure().with_phase("publication")));
			}
		}
		(true, Some(existing)) => {
			// MO2 order lists the highest priority first, so the entry position
			// counts down from the last priority.
			let list_position = u64::try_from(installed.len())
				.ok()
				.and_then(|count| count.checked_sub(1))
				.and_then(|last| last.checked_sub(u64::from(existing.priority.get())))
				.ok_or_else(|| report!(ErrorMarker::transaction_failure().with_phase("publication")))?;
			if plan.projected_state.mode != InstallMode::Replacement
				|| plan.mod_name.as_str() != existing.name.as_str()
				|| plan.projected_state.mod_name != existing.name
				|| plan.projected_state.list_position != list_position
				|| plan.projected_state.enabled != existing.enabled
				|| plan.projected_state.priority != existing.priority
			{
				return Err(report!(ErrorMarker::transaction_failure().with_phase("publication")));
			}
		}
		_ => return Err(report!(ErrorMarker::transaction_failure().with_phase("publication"))),
	}
	Ok(())
}

/// Returns the child directory, reusing an entry whose name differs only by case only when the spelling matches.
async fn open_or_create_exact(
	directory: &Path,
	name: &str,
	cancellation: &CancellationToken,
) -> Result<PathBuf, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
	}

	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::transaction_failure().with_phase("publication"))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::transaction_failure().with_phase("publication"))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("publication")));
		}

		let existing = entry.file_name();
		let existing = existing
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::transaction_failure().with_phase("publication")))?;
		if case_fold_key(existing) != case_fold_key(name) {
			continue;
		}
		if existing != name
			|| !entry
				.file_type()
				.await
				.context(ErrorMarker::transaction_failure().with_phase("publication"))?
				.is_dir()
		{
			return Err(report!(ErrorMarker::transaction_failure().with_phase("publication")));
		}
		return Ok(entry.path());
	}

	let child = directory.join(name);
	create_dir(&child)
		.await
		.context(ErrorMarker::transaction_failure().with_phase("publication"))?;
	Ok(child)
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

async fn write_metadata(mod_dir: &Path, approved: &ApprovedInstallation) -> Result<(), ErrorMarker> {
	let source_basename = approved.source_basename.as_str();
	if source_basename.is_empty() {
		return Err(report!(ErrorMarker::transaction_failure().with_phase("publication")));
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
	let text = to_string_pretty(&metadata).context(ErrorMarker::transaction_failure().with_phase("publication"))?;
	write(mod_dir.join("meta.toml"), text)
		.await
		.context(ErrorMarker::transaction_failure().with_phase("publication"))
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

/// Checks that the written installation reads back with the planned priority and state.
async fn finish_committed_installation(
	root: &Path,
	binding: &GameBinding,
	plan: &InstallPlan,
) -> Result<(), ErrorMarker> {
	let snapshot = load(
		root,
		binding,
		InstallationStateAccess::Mutation,
		&CancellationToken::new(),
	)
	.await
	.context(ErrorMarker::transaction_failure().with_phase("publication"))?;
	snapshot.installed_mods
		.iter()
		.find(|installed| installed.name == plan.mod_name)
		.filter(|installed| {
			installed.priority == plan.projected_state.priority
				&& installed.enabled == plan.projected_state.enabled
		})
		.map(drop)
		.ok_or_else(|| report!(ErrorMarker::transaction_failure().with_phase("publication")))
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
mod tests {
	use super::InstallationTransaction;
	use super::open_or_create_exact;
	use crate::EnvironmentAdapter;
	use crate::profile::PROFILE_FILES;
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
	use rootcause::Result;
	use std::env::current_dir;
	use std::ffi::OsStr;
	use std::fs;
	use std::path::Path;
	use tempfile::TempDir;
	use tokio::io::AsyncWriteExt;
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
				automatic_events: Vec::new(),
				resolved_flags: Vec::new(),
				warnings: Vec::new(),
				candidates: vec![PlannedCandidate {
					origin_condition_evaluation: None,
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
					list_position: 0,
					enabled,
					overlaps: Vec::new(),
				},
			},
		}
	}

	async fn initialized_environment(parent: &TempDir) -> EnvironmentRoot {
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.await
			.expect("fixture environment must initialize");
		root
	}

	async fn stage_file(
		transaction: &mut InstallationTransaction,
		path: &str,
		contents: &[u8],
		cancellation: &CancellationToken,
	) {
		let path = DataRelativePath::new(path.to_owned()).expect("fixture path must be valid");
		let mut file = transaction
			.begin_file(&path, cancellation)
			.await
			.expect("fixture file must begin");
		file.write_all(contents).await.expect("fixture file must write");
		file.flush().await.expect("fixture file must flush");
		transaction
			.finish_file(path.comparison_key())
			.expect("fixture file must finish");
	}

	async fn install(root: &EnvironmentRoot, approved: ApprovedInstallation, path: &str, contents: &[u8]) {
		let cancellation = CancellationToken::new();
		let mut transaction = InstallationTransaction::begin(
			root.as_path(),
			&initialization_plan(&root.as_path().parent().expect("fixture parent").join("game"))
				.game_binding,
			approved,
			&cancellation,
		)
		.await
		.expect("fixture transaction must begin");
		stage_file(&mut transaction, path, contents, &cancellation).await;
		transaction
			.finish(&cancellation)
			.await
			.expect("fixture transaction must publish");
	}

	#[expect(clippy::panic, reason = "a successful result must fail this test assertion")]
	fn assert_manual_cleanup_required<T>(result: Result<T, ErrorMarker>) {
		let Err(error) = result else {
			panic!("pending work must require manual cleanup");
		};
		assert_eq!(error.current_context().code(), ErrorCode::ManualCleanupRequired);
	}

	#[tokio::test]
	async fn failed_intent_validation_writes_nothing() {
		let parent = temp_dir();
		let root = initialized_environment(&parent).await;
		let archive = parent.path().join("invalid-plan.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let approved = approved_installation(&archive, "Invalid", false, false, 1, "file.txt");

		let error = InstallationTransaction::begin(
			root.as_path(),
			&initialization_plan(&root.as_path().parent().expect("fixture parent").join("game"))
				.game_binding,
			approved,
			&CancellationToken::new(),
		)
		.await
		.err()
		.expect("invalid intent must fail");

		assert_eq!(error.current_context().code(), ErrorCode::TransactionFailure);
		assert!(!root.as_path().join("mods/Invalid").exists());
	}

	#[tokio::test]
	async fn prior_temp_debris_is_rejected() {
		let parent = temp_dir();
		let root = initialized_environment(&parent).await;
		let archive = parent.path().join("blocked.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let debris = root.as_path().join("temp/unrelated");
		fs::write(&debris, b"keep").expect("prior debris must exist");

		assert_manual_cleanup_required(
			InstallationTransaction::begin(
				root.as_path(),
				&initialization_plan(&root.as_path().parent().expect("fixture parent").join("game"))
					.game_binding,
				approved_installation(&archive, "Blocked", false, false, 0, "blocked.txt"),
				&CancellationToken::new(),
			)
			.await,
		);
		assert_eq!(fs::read(debris).expect("prior debris must remain"), b"keep");
	}

	#[tokio::test]
	async fn exact_directory_open_rejects_simple_unicode_case_aliases() {
		let parent = temp_dir();
		fs::create_dir(parent.path().join("éς")).expect("existing directory fixture must be created");
		let directory = parent
			.path()
			.canonicalize()
			.expect("fixture directory must canonicalize");

		assert!(open_or_create_exact(&directory, "ÉΣ", &CancellationToken::new())
			.await
			.is_err());
	}

	#[tokio::test]
	async fn new_install_publishes_the_mod_then_modlist() {
		let parent = temp_dir();
		let root = initialized_environment(&parent).await;
		let archive = parent.path().join("new.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		install(
			&root,
			approved_installation(&archive, "New", false, false, 0, "file.txt"),
			"file.txt",
			b"contents",
		)
		.await;

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

	#[tokio::test]
	async fn new_install_takes_the_top_of_an_mo2_ordered_modlist() {
		let parent = temp_dir();
		let root = initialized_environment(&parent).await;
		fs::create_dir(root.as_path().join("mods/Base")).expect("existing mod must exist");
		fs::write(root.as_path().join("profile/modlist.txt"), b"# header\r\n+Base\r\n")
			.expect("modlist must write");
		let archive = parent.path().join("new.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");

		install(
			&root,
			approved_installation(&archive, "New", false, false, 1, "file.txt"),
			"file.txt",
			b"contents",
		)
		.await;

		assert_eq!(
			fs::read(root.as_path().join("profile/modlist.txt")).expect("modlist must read"),
			b"# header\r\n-New\r\n+Base\r\n"
		);
	}

	#[tokio::test]
	async fn metadata_keeps_fomod_provenance_without_decorative_installer_fields() {
		let parent = temp_dir();
		let root = initialized_environment(&parent).await;
		let archive = parent.path().join("fomod.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		let mut approved = approved_installation(&archive, "Fomod", false, false, 0, "file.txt");
		approved.fomod_schema_version = Some("5.0".to_owned());
		approved.plan.accepted_choices = vec![AcceptedChoice {
			sequence: 0,
			group_id: "core".to_owned(),
			option_id: "standard".to_owned(),
		}];
		approved.plan.archive_identity = ArchiveIdentity::Fomod {
			archive_sha256: Sha256Digest::new("a".repeat(64)).expect("fixture hash must be valid"),
			package_root: String::new(),
			config_member: "fomod/ModuleConfig.xml".to_owned(),
			config_sha256: Sha256Digest::new("b".repeat(64)).expect("fixture hash must be valid"),
		};

		install(&root, approved, "file.txt", b"contents").await;

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

	#[tokio::test]
	async fn disabled_replacement_preserves_every_profile_file() {
		let parent = temp_dir();
		let root = initialized_environment(&parent).await;
		let archive = parent.path().join("replacement.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		install(
			&root,
			approved_installation(&archive, "Replace", false, false, 0, "Old.ESP"),
			"Old.ESP",
			b"old",
		)
		.await;
		let before = ["plugins.txt", "loadorder.txt", "modlist.txt"].map(|name| {
			fs::read(root.as_path().join("profile").join(name)).expect("profile file must read")
		});

		install(
			&root,
			approved_installation(&archive, "Replace", true, false, 0, "New.ESP"),
			"New.ESP",
			b"new",
		)
		.await;

		let after = ["plugins.txt", "loadorder.txt", "modlist.txt"].map(|name| {
			fs::read(root.as_path().join("profile").join(name)).expect("profile file must read")
		});
		assert_eq!(after, before);
		assert!(!root.as_path().join("mods/Replace/Old.ESP").exists());
		assert!(root.as_path().join("mods/Replace/New.ESP").is_file());
	}

	#[tokio::test]
	async fn enabled_replacement_preserves_modlist_and_updates_only_changed_plugin_files() {
		let parent = temp_dir();
		let root = initialized_environment(&parent).await;
		let archive = parent.path().join("replacement.zip");
		fs::write(&archive, b"archive").expect("archive fixture must exist");
		install(
			&root,
			approved_installation(&archive, "Replace", false, false, 0, "Old.ESP"),
			"Old.ESP",
			b"old",
		)
		.await;
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
		)
		.await;

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
}
