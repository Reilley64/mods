mod active_code_page;
mod conflict_scan;
mod hashing;
mod manifest;
mod profile;
mod profile_activation;
mod publication;
mod safe_fs;
mod snapshot;
mod transactions;

use crate::conflict_scan::read_content as read_conflict_content;
use crate::conflict_scan::scan as scan_conflicts;
use crate::manifest::validate_manifest_file;
use crate::manifest::write_manifest;
use crate::profile::stage_profile;
use crate::profile::validate_profile;
use crate::publication::OPERATION_DIRECTORY;
use crate::publication::publish_initialization;
use crate::safe_fs::sync_tree;
use crate::safe_fs::validate_exact_entries;
use crate::snapshot::assess_installation as assess_installation_snapshot;
use crate::snapshot::load as load_snapshot;
use crate::transactions::InstallationTransaction;
use application::ErrorCode;
use application::ErrorMarker;
use application::conflicts::ConflictContentRead;
use application::conflicts::EnvironmentConflictScan;
use application::conflicts::IndexedConflictFileId;
use application::installation::ApprovedInstallation;
use application::installation::InstallPlan;
use application::installation::InstallationAssessment;
use application::installation::InstallationState;
use application::ports::AssessInitializationTarget;
use application::ports::AssessInstallation;
use application::ports::BeginInstallation;
use application::ports::InitializationPlan;
use application::ports::InitializationTargetAssessment;
use application::ports::InstallationChange;
use application::ports::InstallationFile;
use application::ports::InstallationStateAccess;
use application::ports::LoadInstallationState;
use application::ports::PortFuture;
use application::ports::ProfileFileRecord;
use application::ports::PublishEnvironment;
use application::ports::ReadConflictContent;
use application::ports::ScanEnvironmentConflicts;
use domain::DataRelativePath;
use domain::EnvironmentRoot;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use safe_fs::SafeDir;
use std::ffi::OsStr;
use std::future::ready;
use std::io;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub struct EnvironmentAdapter;

impl EnvironmentAdapter {
	fn assess(
		&self,
		root: &EnvironmentRoot,
		cancellation: &CancellationToken,
	) -> Result<InitializationTargetAssessment, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let directory = match SafeDir::open_absolute(root.as_path()) {
			Ok(directory) => directory,
			Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {
				return Ok(InitializationTargetAssessment::Available);
			}
			Err(error) => {
				return Err(error.context(ErrorMarker::environment_root_unsafe()));
			}
		};
		assess_open(&directory, cancellation)
	}

	fn publish(
		&self,
		root: &EnvironmentRoot,
		plan: InitializationPlan,
		cancellation: &CancellationToken,
	) -> Result<Vec<ProfileFileRecord>, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let root_dir =
			SafeDir::create_absolute(root.as_path()).context(ErrorMarker::environment_root_unsafe())?;
		assess_open(&root_dir, cancellation)?;
		let temp = root_dir
			.ensure_dir("temp")
			.context(ErrorMarker::environment_root_unsafe())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let operation = match temp.create_dir(OPERATION_DIRECTORY) {
			Ok(operation) => operation,
			Err(error) if error.current_context().kind() == io::ErrorKind::AlreadyExists => {
				return Err(error.context(ErrorMarker::manual_cleanup_required()));
			}
			Err(error) => return Err(error.context(ErrorMarker::environment_root_unsafe())),
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let stage = operation
			.create_dir("stage")
			.context(ErrorMarker::environment_root_unsafe())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let mods = stage
			.create_dir("mods")
			.context(ErrorMarker::environment_root_unsafe())?;
		let profile = stage
			.create_dir("profile")
			.context(ErrorMarker::environment_root_unsafe())?;
		let overwrite = stage
			.create_dir("overwrite")
			.context(ErrorMarker::environment_root_unsafe())?;
		let cache = stage
			.create_dir("cache")
			.context(ErrorMarker::environment_root_unsafe())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let records = stage_profile(&profile, &plan.profile_sources, cancellation)?;
		cache.write_new("Fallout - Invalidation.bsa", &empty_bsa_bytes())
			.context(ErrorMarker::environment_root_unsafe())?;
		write_manifest(&stage, &plan)?;
		validate_stage(&stage, LayoutLocation::Stage, cancellation).map_err(|report| {
			if report.current_context().code() == ErrorCode::EnvironmentInvalid {
				report.context(ErrorMarker::game_install_invalid())
			} else {
				report
			}
		})?;

		for directory in [&mods, &profile, &overwrite, &cache] {
			sync_tree(directory, ErrorMarker::environment_root_unsafe(), cancellation)?;
		}
		stage.sync().context(ErrorMarker::environment_root_unsafe())?;
		operation.sync().context(ErrorMarker::environment_root_unsafe())?;
		temp.sync().context(ErrorMarker::environment_root_unsafe())?;
		root_dir.sync().context(ErrorMarker::environment_root_unsafe())?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		drop(cache);
		drop(overwrite);
		drop(profile);
		drop(mods);
		drop(stage);
		drop(operation);
		publish_initialization(&root_dir, &temp, cancellation)?;
		Ok(records)
	}

	fn load_installation_state(
		&self,
		root: &EnvironmentRoot,
		access: InstallationStateAccess,
		cancellation: &CancellationToken,
	) -> Result<InstallationState, ErrorMarker> {
		let snapshot = load_snapshot(root.as_path(), access, cancellation)?;
		Ok(InstallationState {
			game_binding: snapshot.game_binding,
			installed_mods: snapshot.installed_mods,
			current_winners: snapshot.current_winners,
			file_dependencies: snapshot.file_dependencies,
		})
	}

	fn assess_installation(
		&self,
		root: &EnvironmentRoot,
		plan: &InstallPlan,
		cancellation: &CancellationToken,
	) -> Result<InstallationAssessment, ErrorMarker> {
		assess_installation_snapshot(root.as_path(), plan, cancellation)
	}

	fn begin_installation(
		&self,
		root: &EnvironmentRoot,
		approved: ApprovedInstallation,
		cancellation: &CancellationToken,
	) -> Result<InstallationChange, ErrorMarker> {
		let transaction = InstallationTransaction::begin(root.as_path(), approved, cancellation)?;
		let transaction = Arc::new(Mutex::new(transaction));
		let begin_transaction = Arc::clone(&transaction);
		let begin_file = Arc::new(move |path: DataRelativePath, cancellation: CancellationToken| {
			let begin_transaction = Arc::clone(&begin_transaction);
			Box::pin(async move {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				let mut transaction = begin_transaction.lock().await;
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				let file = transaction.begin_file(&path, &cancellation);
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				let file = file?;
				drop(transaction);

				let path_key = path.comparison_key().to_owned();
				let file = Arc::new(Mutex::new((file, false)));
				let write_file = Arc::clone(&file);
				let write_chunk =
					Arc::new(move |contents: Vec<u8>, cancellation: CancellationToken| {
						let write_file = Arc::clone(&write_file);
						Box::pin(async move {
							if cancellation.is_cancelled() {
								return Err(
									report!(ErrorMarker::operation_cancelled()),
								);
							}
							let mut file = write_file.lock().await;
							if cancellation.is_cancelled() {
								return Err(
									report!(ErrorMarker::operation_cancelled()),
								);
							}
							if file.1 {
								return Err(
									report!(ErrorMarker::transaction_failure()),
								);
							}
							let written =
								file.0.write_chunk(&contents)
									.context(ErrorMarker::transaction_failure());
							if cancellation.is_cancelled() {
								return Err(
									report!(ErrorMarker::operation_cancelled()),
								);
							}
							written
						}) as PortFuture<_>
					});
				let finish_file = Arc::clone(&file);
				let finish_transaction = Arc::clone(&begin_transaction);
				let finish = Arc::new(move |cancellation: CancellationToken| {
					let finish_file = Arc::clone(&finish_file);
					let finish_transaction = Arc::clone(&finish_transaction);
					let path_key = path_key.clone();
					Box::pin(async move {
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()));
						}
						let mut file = finish_file.lock().await;
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()));
						}
						if file.1 {
							return Err(report!(ErrorMarker::transaction_failure()));
						}
						let finished = file.0.finish();
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()));
						}
						finished.context(ErrorMarker::transaction_failure())?;

						let mut transaction = finish_transaction.lock().await;
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()));
						}
						transaction.finish_file(&path_key)?;
						file.1 = true;
						Ok(())
					}) as PortFuture<_>
				});
				Ok(InstallationFile { write_chunk, finish })
			}) as PortFuture<_>
		});
		let finish_transaction = Arc::clone(&transaction);
		let finish = Arc::new(move |cancellation: CancellationToken| {
			let finish_transaction = Arc::clone(&finish_transaction);
			Box::pin(async move {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				let mut transaction = finish_transaction.lock().await;
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				transaction.finish(&cancellation)
			}) as PortFuture<_>
		});
		Ok(InstallationChange { begin_file, finish })
	}

	fn scan_environment_conflicts(
		&self,
		root: &EnvironmentRoot,
		cancellation: &CancellationToken,
	) -> Result<EnvironmentConflictScan, ErrorMarker> {
		scan_conflicts(root.as_path(), cancellation)
	}

	fn read_conflict_content(
		&self,
		root: &EnvironmentRoot,
		id: IndexedConflictFileId,
		cancellation: &CancellationToken,
	) -> Result<ConflictContentRead, ErrorMarker> {
		read_conflict_content(root.as_path(), &id, cancellation)
	}

	pub fn scan_environment_conflicts_port(&self, root: EnvironmentRoot) -> ScanEnvironmentConflicts {
		let adapter = self.clone();
		Arc::new(move |cancellation| {
			let result = adapter.scan_environment_conflicts(&root, &cancellation);
			Box::pin(ready(result)) as PortFuture<_>
		})
	}

	pub fn read_conflict_content_port(&self, root: EnvironmentRoot) -> ReadConflictContent {
		let adapter = self.clone();
		Arc::new(move |id, cancellation| {
			let result = adapter.read_conflict_content(&root, id, &cancellation);
			Box::pin(ready(result)) as PortFuture<_>
		})
	}

	pub fn load_installation_state_port(&self, root: EnvironmentRoot) -> LoadInstallationState {
		let adapter = self.clone();
		Arc::new(move |access, cancellation| {
			let result = adapter.load_installation_state(&root, access, &cancellation);
			Box::pin(ready(result)) as PortFuture<_>
		})
	}

	pub fn assess_installation_port(&self, root: EnvironmentRoot) -> AssessInstallation {
		let adapter = self.clone();
		Arc::new(move |plan, cancellation| {
			let result = adapter.assess_installation(&root, &plan, &cancellation);
			Box::pin(ready(result)) as PortFuture<_>
		})
	}

	pub fn begin_installation_port(&self, root: EnvironmentRoot) -> BeginInstallation {
		let adapter = self.clone();
		Arc::new(move |approved, cancellation| {
			let result = adapter.begin_installation(&root, approved, &cancellation);
			Box::pin(ready(result)) as PortFuture<_>
		})
	}

	pub fn assess_port(&self) -> AssessInitializationTarget {
		let adapter = self.clone();
		Arc::new(move |root, cancellation| {
			let result = adapter.assess(&root, &cancellation);
			Box::pin(ready(result)) as PortFuture<_>
		})
	}

	pub fn publish_port(&self) -> PublishEnvironment {
		let adapter = self.clone();
		Arc::new(move |root, plan, cancellation| {
			let result = adapter.publish(&root, plan, &cancellation);
			Box::pin(ready(result)) as PortFuture<_>
		})
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutLocation {
	Stage,
	Root,
}

fn assess_open(
	root: &SafeDir,
	cancellation: &CancellationToken,
) -> Result<InitializationTargetAssessment, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	if root.exists("temp").context(ErrorMarker::environment_root_unsafe())? {
		let temp = root.open_dir("temp").context(ErrorMarker::environment_root_unsafe())?;
		let opened = temp.entries();
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let mut pending = opened.context(ErrorMarker::environment_root_unsafe())?;
		if let Some(entry) = pending.next() {
			entry.into_report().context(ErrorMarker::environment_root_unsafe())?;
			return Err(report!(ErrorMarker::manual_cleanup_required()));
		}
	}

	let opened = root.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_root_unsafe())?;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		let entry = entry.into_report().context(ErrorMarker::environment_root_unsafe())?;
		let name = entry.file_name();
		if name == OsStr::new("mods.toml") {
			return Err(report!(ErrorMarker::environment_already_initialized()));
		}
		if name == OsStr::new("temp") {
			continue;
		}
		if name != OsStr::new("logs") {
			return Err(report!(ErrorMarker::environment_root_not_empty()));
		}
		root.open_dir("logs").context(ErrorMarker::environment_root_unsafe())?;
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	Ok(InitializationTargetAssessment::Available)
}

fn empty_bsa_bytes() -> Vec<u8> {
	let mut bytes = Vec::with_capacity(36);
	bytes.extend_from_slice(b"BSA\0");
	bytes.extend_from_slice(&0x68_u32.to_le_bytes());
	bytes.extend_from_slice(&36_u32.to_le_bytes());
	bytes.extend_from_slice(&0x3_u32.to_le_bytes());
	for _ in 0..5 {
		bytes.extend_from_slice(&0_u32.to_le_bytes());
	}
	bytes
}

pub(crate) fn validate_stage(
	directory: &SafeDir,
	location: LayoutLocation,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let allowed = if location == LayoutLocation::Stage {
		&["mods", "profile", "overwrite", "cache", "mods.toml"][..]
	} else {
		&["mods", "profile", "overwrite", "cache", "mods.toml", "temp", "logs"][..]
	};
	validate_exact_entries(directory, allowed, cancellation)?;
	let mods = directory
		.open_dir("mods")
		.context(ErrorMarker::environment_invalid(None))?;
	let profile_dir = directory
		.open_dir("profile")
		.context(ErrorMarker::environment_invalid(None))?;
	let overwrite = directory
		.open_dir("overwrite")
		.context(ErrorMarker::environment_invalid(None))?;
	let cache = directory
		.open_dir("cache")
		.context(ErrorMarker::environment_invalid(None))?;
	validate_exact_entries(&mods, &[], cancellation)?;
	validate_exact_entries(&overwrite, &[], cancellation)?;
	validate_exact_entries(&cache, &["Fallout - Invalidation.bsa"], cancellation)?;
	validate_profile(&profile_dir, cancellation)?;
	let manifest = validate_manifest_file(directory, cancellation)?;
	let game = SafeDir::open_absolute(Path::new(&manifest.game_dir))
		.context(ErrorMarker::environment_invalid(None))?;
	if directory
		.is_ancestor_of(&game)
		.context(ErrorMarker::environment_invalid(None))?
		|| game.is_ancestor_of(directory)
			.context(ErrorMarker::environment_invalid(None))?
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	validate_bsa_file(&cache, cancellation)
}

fn validate_bsa_file(cache: &SafeDir, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = cache.open_regular("Fallout - Invalidation.bsa");
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut bsa = opened.context(ErrorMarker::environment_invalid(None))?;
	let expected = empty_bsa_bytes();
	let mut bytes = vec![0_u8; expected.len() + 1];
	let mut length = 0;
	while length < bytes.len() {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let read = bsa.read_chunk(&mut bytes[length..]);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let read = read.context(ErrorMarker::environment_invalid(None))?;
		if read == 0 {
			break;
		}
		length += read;
	}
	if bytes[..length] != expected {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
mod tests {
	use super::EnvironmentAdapter;
	use super::empty_bsa_bytes;
	use crate::manifest::write_manifest;
	use crate::profile::PROFILE_FILES;
	use crate::profile::stage_profile;
	use crate::publication::OPERATION_DIRECTORY;
	use crate::publication::publish_initialization;
	use crate::safe_fs::SafeDir;
	use application::ErrorCode;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileSource;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use std::env::current_dir;
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

	fn plan(game: &Path) -> InitializationPlan {
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

	fn staged_initialization(parent: &TempDir) -> (EnvironmentRoot, SafeDir, SafeDir, InitializationPlan) {
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		let plan = plan(&game);
		let root_dir = SafeDir::create_absolute(root.as_path()).expect("root must be created");
		let temp = root_dir.create_dir("temp").expect("temp must be created");
		let operation = temp.create_dir(OPERATION_DIRECTORY).expect("operation must be created");
		let stage = operation.create_dir("stage").expect("stage must be created");
		stage.create_dir("mods").expect("mods must be created");
		let profile = stage.create_dir("profile").expect("profile must be created");
		stage.create_dir("overwrite").expect("overwrite must be created");
		let cache = stage.create_dir("cache").expect("cache must be created");
		stage_profile(&profile, &plan.profile_sources, &CancellationToken::new()).expect("profile must stage");
		cache.write_new("Fallout - Invalidation.bsa", &empty_bsa_bytes())
			.expect("BSA must stage");
		write_manifest(&stage, &plan).expect("manifest must stage");
		drop(cache);
		drop(profile);
		drop(stage);
		drop(operation);
		(root, root_dir, temp, plan)
	}

	fn assert_no_canonical_state(root: &Path) {
		for name in ["mods", "profile", "overwrite", "cache", "mods.toml"] {
			assert!(!root.join(name).exists(), "unexpected canonical artifact {name}");
		}
	}

	#[test]
	fn pending_operation_takes_precedence_and_refuses_initialization() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		fs::create_dir_all(root.as_path().join("temp/operation")).expect("pending operation must be created");
		fs::write(root.as_path().join("mods.toml"), b"already initialized")
			.expect("manifest marker must write");

		let error = EnvironmentAdapter
			.assess(&root, &CancellationToken::new())
			.expect_err("pending work must require cleanup");
		assert_eq!(error.current_context().code(), ErrorCode::ManualCleanupRequired);
	}

	#[test]
	fn staged_initialization_has_no_canonical_mutation() {
		let parent = temp_dir();
		let (root, _, _, _) = staged_initialization(&parent);

		assert_no_canonical_state(root.as_path());
		assert!(root.as_path().join("temp/operation/stage/mods.toml").is_file());
	}

	#[test]
	fn initialization_publishes_and_validates_the_canonical_environment() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");

		let records = EnvironmentAdapter
			.publish(&root, plan(&game), &CancellationToken::new())
			.expect("initialization must publish");

		assert_eq!(records.len(), PROFILE_FILES.len());
		assert!(root.as_path().join("mods.toml").is_file());
		assert!(root.as_path().join("profile/modlist.txt").is_file());
		assert!(fs::read_dir(root.as_path().join("temp"))
			.expect("temp must read")
			.next()
			.is_none());
	}

	#[test]
	fn cancellation_before_publication_preserves_the_complete_stage() {
		let parent = temp_dir();
		let (root, root_dir, temp, _) = staged_initialization(&parent);
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let error = publish_initialization(&root_dir, &temp, &cancellation)
			.expect_err("cancellation must stop publication");

		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		assert_no_canonical_state(root.as_path());
		assert!(root.as_path().join("temp/operation/stage/mods.toml").is_file());
	}
}
