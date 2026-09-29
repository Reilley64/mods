#![cfg_attr(test, feature(fn_traits))]

mod active_code_page;
mod conflict_scan;
mod derived_profile;
mod execution_preparation;
mod export;
pub use derived_profile::ExecutionInis;
mod files;
mod hashing;
mod manifest;
mod profile;
mod profile_activation;
mod snapshot;
mod transactions;

use crate::conflict_scan::read_content as read_conflict_content;
use crate::conflict_scan::scan as scan_conflicts;
use crate::files::validate_exact_entries;
use crate::manifest::validate_manifest_file;
use crate::manifest::write_manifest;
use crate::profile::validate_profile;
use crate::profile::write_initial_profile;
use crate::snapshot::assess_installation as assess_installation_snapshot;
use crate::snapshot::load as load_snapshot;
use crate::transactions::InstallationTransaction;
use application::ErrorCode;
use application::ErrorMarker;
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
use domain::GameBinding;
pub use execution_preparation::ExecutionProfileText;
pub use execution_preparation::ExecutionProvider;
pub use execution_preparation::ExecutionVisibleFile;
pub use execution_preparation::LaunchVisibleFile;
pub use execution_preparation::PreparedExecution;
pub use execution_preparation::PreparedLaunch;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::Arc;
use tokio::fs::create_dir;
use tokio::fs::create_dir_all;
use tokio::fs::metadata;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio::fs::write;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default)]
pub struct EnvironmentAdapter;

impl EnvironmentAdapter {
	async fn assess(
		&self,
		root: &EnvironmentRoot,
		cancellation: &CancellationToken,
	) -> Result<InitializationTargetAssessment, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		match metadata(root.as_path()).await {
			Ok(metadata) if metadata.is_dir() => assess_open(root.as_path(), cancellation).await,
			Ok(_) => Err(report!(ErrorMarker::environment_root_unsafe())),
			Err(error) if error.kind() == io::ErrorKind::NotFound => {
				Ok(InitializationTargetAssessment::Available)
			}
			Err(error) => Err(report!(error).context(ErrorMarker::environment_root_unsafe())),
		}
	}

	async fn publish(
		&self,
		root: &EnvironmentRoot,
		plan: InitializationPlan,
		cancellation: &CancellationToken,
	) -> Result<Vec<ProfileFileRecord>, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let root_dir = root.as_path();
		create_dir_all(root_dir)
			.await
			.context(ErrorMarker::environment_root_unsafe())?;
		assess_open(root_dir, cancellation).await?;

		create_dir_all(root_dir.join("temp"))
			.await
			.context(ErrorMarker::environment_root_unsafe())?;
		for directory in ["mods", "profile", "overwrite", "cache"] {
			create_dir(root_dir.join(directory))
				.await
				.context(ErrorMarker::environment_root_unsafe())?;
		}
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let records =
			write_initial_profile(&root_dir.join("profile"), &plan.profile_sources, cancellation).await?;
		write(root_dir.join("cache/Fallout - Invalidation.bsa"), empty_bsa_bytes())
			.await
			.context(ErrorMarker::environment_root_unsafe())?;

		// `mods.toml` goes last, so a partial layout is never mistaken for an initialized environment.
		write_manifest(root_dir, &plan).await?;

		validate_layout(root_dir, &CancellationToken::new())
			.await
			.map_err(|report| {
				if report.current_context().code() == ErrorCode::EnvironmentInvalid {
					report.context(ErrorMarker::game_install_invalid())
				} else {
					report
				}
			})?;
		Ok(records)
	}

	async fn load_installation_state(
		&self,
		root: &EnvironmentRoot,
		binding: &GameBinding,
		access: InstallationStateAccess,
		cancellation: &CancellationToken,
	) -> Result<InstallationState, ErrorMarker> {
		let snapshot = load_snapshot(root.as_path(), binding, access, cancellation).await?;
		Ok(InstallationState {
			game_binding: snapshot.game_binding,
			installed_mods: snapshot.installed_mods,
			current_winners: snapshot.current_winners,
			file_dependencies: snapshot.file_dependencies,
		})
	}

	async fn assess_installation(
		&self,
		root: &EnvironmentRoot,
		binding: &GameBinding,
		plan: &InstallPlan,
		cancellation: &CancellationToken,
	) -> Result<InstallationAssessment, ErrorMarker> {
		assess_installation_snapshot(root.as_path(), binding, plan, cancellation).await
	}

	async fn begin_installation(
		&self,
		root: &EnvironmentRoot,
		binding: &GameBinding,
		approved: ApprovedInstallation,
		cancellation: &CancellationToken,
	) -> Result<InstallationChange, ErrorMarker> {
		let transaction =
			InstallationTransaction::begin(root.as_path(), binding, approved, cancellation).await?;
		let transaction = Arc::new(Mutex::new(transaction));
		let begin_transaction = Arc::clone(&transaction);
		let begin_file = Arc::new(move |path: DataRelativePath, cancellation: CancellationToken| {
			let begin_transaction = Arc::clone(&begin_transaction);
			Box::pin(async move {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("extraction")
					));
				}
				let mut transaction = begin_transaction.lock().await;
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("extraction")
					));
				}

				let file = transaction.begin_file(&path, &cancellation).await;
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("extraction")
					));
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
								return Err(report!(ErrorMarker::operation_cancelled(
								)
								.with_phase("extraction")));
							}
							let mut file = write_file.lock().await;
							if cancellation.is_cancelled() {
								return Err(report!(ErrorMarker::operation_cancelled(
								)
								.with_phase("extraction")));
							}
							if file.1 {
								return Err(report!(ErrorMarker::transaction_failure(
								)
								.with_phase("publication")));
							}
							let written = file.0.write_all(&contents).await.context(
								ErrorMarker::transaction_failure()
									.with_phase("publication"),
							);
							if cancellation.is_cancelled() {
								return Err(report!(ErrorMarker::operation_cancelled(
								)
								.with_phase("extraction")));
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
							return Err(report!(ErrorMarker::operation_cancelled()
								.with_phase("extraction")));
						}
						let mut file = finish_file.lock().await;
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()
								.with_phase("extraction")));
						}
						if file.1 {
							return Err(report!(ErrorMarker::transaction_failure()
								.with_phase("publication")));
						}

						let finished = file.0.flush().await;
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()
								.with_phase("extraction")));
						}
						finished.context(
							ErrorMarker::transaction_failure().with_phase("publication"),
						)?;

						let mut transaction = finish_transaction.lock().await;
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()
								.with_phase("extraction")));
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
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("publication")
					));
				}
				let mut transaction = finish_transaction.lock().await;
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("publication")
					));
				}

				transaction.finish(&cancellation).await
			}) as PortFuture<_>
		});
		Ok(InstallationChange { begin_file, finish })
	}

	pub fn scan_environment_conflicts_port(
		&self,
		root: EnvironmentRoot,
		binding: GameBinding,
	) -> ScanEnvironmentConflicts {
		Arc::new(move |cancellation| {
			let root = root.clone();
			let binding = binding.clone();
			Box::pin(async move { scan_conflicts(root.as_path(), &binding, &cancellation).await })
				as PortFuture<_>
		})
	}

	pub fn read_conflict_content_port(&self, root: EnvironmentRoot, binding: GameBinding) -> ReadConflictContent {
		Arc::new(move |id, cancellation| {
			let root = root.clone();
			let binding = binding.clone();
			Box::pin(
				async move { read_conflict_content(root.as_path(), &binding, &id, &cancellation).await },
			) as PortFuture<_>
		})
	}

	pub fn load_installation_state_port(
		&self,
		root: EnvironmentRoot,
		binding: GameBinding,
	) -> LoadInstallationState {
		let adapter = self.clone();
		Arc::new(move |access, cancellation| {
			let adapter = adapter.clone();
			let root = root.clone();
			let binding = binding.clone();
			Box::pin(async move {
				adapter.load_installation_state(&root, &binding, access, &cancellation)
					.await
			}) as PortFuture<_>
		})
	}

	pub fn assess_installation_port(&self, root: EnvironmentRoot, binding: GameBinding) -> AssessInstallation {
		let adapter = self.clone();
		Arc::new(move |plan, cancellation| {
			let adapter = adapter.clone();
			let root = root.clone();
			let binding = binding.clone();
			Box::pin(
				async move { adapter.assess_installation(&root, &binding, &plan, &cancellation).await },
			) as PortFuture<_>
		})
	}

	pub fn begin_installation_port(&self, root: EnvironmentRoot, binding: GameBinding) -> BeginInstallation {
		let adapter = self.clone();
		Arc::new(move |approved, cancellation| {
			let adapter = adapter.clone();
			let root = root.clone();
			let binding = binding.clone();
			Box::pin(async move {
				adapter.begin_installation(&root, &binding, approved, &cancellation)
					.await
			}) as PortFuture<_>
		})
	}

	pub fn assess_port(&self) -> AssessInitializationTarget {
		let adapter = self.clone();
		Arc::new(move |root, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.assess(&root, &cancellation).await }) as PortFuture<_>
		})
	}

	pub fn publish_port(&self) -> PublishEnvironment {
		let adapter = self.clone();
		Arc::new(move |root, plan, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.publish(&root, plan, &cancellation).await }) as PortFuture<_>
		})
	}
}

async fn assess_open(
	root: &Path,
	cancellation: &CancellationToken,
) -> Result<InitializationTargetAssessment, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	match read_dir(root.join("temp")).await {
		Ok(mut pending) => {
			if pending
				.next_entry()
				.await
				.context(ErrorMarker::environment_root_unsafe())?
				.is_some()
			{
				return Err(report!(ErrorMarker::manual_cleanup_required()));
			}
		}
		Err(error) if error.kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(report!(error).context(ErrorMarker::environment_root_unsafe())),
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mut entries = read_dir(root).await.context(ErrorMarker::environment_root_unsafe())?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_root_unsafe())?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let name = entry.file_name();
		if name == OsStr::new("mods.toml") {
			return Err(report!(ErrorMarker::environment_already_initialized()));
		}
		if name == OsStr::new("temp") {
			continue;
		}
		if name != OsStr::new("logs")
			|| !metadata(entry.path())
				.await
				.context(ErrorMarker::environment_root_unsafe())?
				.is_dir()
		{
			return Err(report!(ErrorMarker::environment_root_not_empty()));
		}
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

/// Checks a freshly initialized environment: the exact layout, empty providers, the profile, the manifest,
/// and the generated archive.
async fn validate_layout(directory: &Path, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	validate_exact_entries(
		directory,
		&["mods", "profile", "overwrite", "cache", "mods.toml", "temp", "logs"],
		cancellation,
	)
	.await?;

	validate_exact_entries(&directory.join("mods"), &[], cancellation).await?;
	validate_exact_entries(&directory.join("overwrite"), &[], cancellation).await?;
	validate_exact_entries(&directory.join("cache"), &["Fallout - Invalidation.bsa"], cancellation).await?;

	validate_profile(&directory.join("profile"), cancellation).await?;

	let manifest = validate_manifest_file(directory, cancellation).await?;
	if !metadata(&manifest.game_dir)
		.await
		.context(ErrorMarker::environment_invalid(None))?
		.is_dir()
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	validate_bsa_file(&directory.join("cache"), cancellation).await
}

async fn validate_bsa_file(cache: &Path, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let bytes = read(cache.join("Fallout - Invalidation.bsa"))
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	if bytes != empty_bsa_bytes() {
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
	use crate::profile::PROFILE_FILES;
	use application::ErrorCode;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::InitializationTargetAssessment;
	use application::ports::ProfileSource;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
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
			),
			profile_sources: InitializationProfileSources {
				files,
				fallout_default_ini: b"[Archive]\r\nsArchiveList=Fallout - Meshes.bsa\r\n".to_vec(),
			},
		}
	}

	#[tokio::test]
	async fn pending_operation_takes_precedence_and_refuses_initialization() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		fs::create_dir_all(root.as_path().join("temp/operation")).expect("pending operation must be created");
		fs::write(root.as_path().join("mods.toml"), b"already initialized")
			.expect("manifest marker must write");

		let error = EnvironmentAdapter
			.assess(&root, &CancellationToken::new())
			.await
			.expect_err("pending work must require cleanup");
		assert_eq!(error.current_context().code(), ErrorCode::ManualCleanupRequired);
	}

	#[tokio::test]
	async fn initialization_publishes_and_validates_the_canonical_environment() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");

		let records = EnvironmentAdapter
			.publish(&root, plan(&game), &CancellationToken::new())
			.await
			.expect("initialization must publish");

		assert_eq!(records.len(), PROFILE_FILES.len());
		assert!(root.as_path().join("mods.toml").is_file());
		assert!(root.as_path().join("profile/modlist.txt").is_file());
		assert!(fs::read_dir(root.as_path().join("temp"))
			.expect("temp must read")
			.next()
			.is_none());
	}

	#[tokio::test]
	async fn logs_only_root_is_available_and_preserved_by_initialization() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		fs::create_dir_all(root.as_path().join("logs")).expect("logs must be created");
		let log = root.as_path().join("logs/session.jsonl");
		fs::write(&log, b"existing diagnostic fixture\n").expect("log must write");

		let assessment = EnvironmentAdapter
			.assess(&root, &CancellationToken::new())
			.await
			.expect("logs-only root must be eligible");
		assert!(matches!(assessment, InitializationTargetAssessment::Available));
		EnvironmentAdapter
			.publish(&root, plan(&game), &CancellationToken::new())
			.await
			.expect("logs-only initialization must publish");

		assert!(root.as_path().join("mods.toml").is_file());
		assert_eq!(
			fs::read(&log).expect("log must remain readable"),
			b"existing diagnostic fixture\n"
		);
	}
}
