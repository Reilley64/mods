mod profile;
mod recovery;
mod safe_fs;

use application::ErrorCode;
use application::ErrorMarker;
use application::ports::AssessInitializationTarget;
use application::ports::InitializationPlan;
use application::ports::InitializationTargetAssessment;
use application::ports::PortFuture;
use application::ports::ProfileFileRecord;
use application::ports::PublishEnvironment;
use application::ports::RecoverEnvironment;
use application::ports::RecoveryOutcome;
use domain::EnvironmentName;
use domain::EnvironmentRoot;
use domain::GameInstallationPath;
use domain::SteamAppId;
use domain::SteamBuildId;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use rootcause::report_collection::ReportCollection;
use safe_fs::SafeDir;
use safe_fs::is_reparse;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::Path;
use std::str::from_utf8;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct EnvironmentAdapter;

impl EnvironmentAdapter {
	pub fn recover(
		&self,
		root: &EnvironmentRoot,
		cancellation: &CancellationToken,
	) -> Result<RecoveryOutcome, ErrorMarker> {
		recovery::recover(root.as_path(), cancellation)
	}

	pub(crate) fn assess(
		&self,
		root: &EnvironmentRoot,
		cancellation: &CancellationToken,
	) -> Result<InitializationTargetAssessment, ErrorMarker> {
		check_cancelled(cancellation)?;
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

	pub(crate) fn publish(
		&self,
		root: &EnvironmentRoot,
		plan: InitializationPlan,
		cancellation: &CancellationToken,
	) -> Result<Vec<ProfileFileRecord>, ErrorMarker> {
		self.publish_with_failure(root, plan, &mut NoPublicationFailure, cancellation)
	}

	fn publish_with_failure(
		&self,
		root: &EnvironmentRoot,
		plan: InitializationPlan,
		failure: &mut dyn PublicationFailure,
		cancellation: &CancellationToken,
	) -> Result<Vec<ProfileFileRecord>, ErrorMarker> {
		check_cancelled(cancellation)?;
		let root_dir =
			SafeDir::create_absolute(root.as_path()).context(ErrorMarker::environment_root_unsafe())?;
		assess_open(&root_dir, cancellation)?;
		let temp = root_dir
			.ensure_dir("temp")
			.context(ErrorMarker::environment_root_unsafe())?;

		check_cancelled(cancellation)?;
		let operation_id = Uuid::new_v4();
		let operation_name = operation_id.to_string();
		let operation_dir = temp
			.create_dir(&operation_name)
			.context(ErrorMarker::environment_root_unsafe())?;
		check_cancelled(cancellation)?;
		let stage = operation_dir
			.create_dir("stage")
			.context(ErrorMarker::environment_root_unsafe())?;
		check_cancelled(cancellation)?;

		let staged: Result<Vec<ProfileFileRecord>, ErrorMarker> = (|| {
			let mods = stage
				.create_dir("mods")
				.context(ErrorMarker::environment_root_unsafe())?;
			check_cancelled(cancellation)?;
			let profile_dir = stage
				.create_dir("profile")
				.context(ErrorMarker::environment_root_unsafe())?;
			check_cancelled(cancellation)?;
			let overwrite = stage
				.create_dir("overwrite")
				.context(ErrorMarker::environment_root_unsafe())?;
			check_cancelled(cancellation)?;
			let cache = stage
				.create_dir("cache")
				.context(ErrorMarker::environment_root_unsafe())?;
			check_cancelled(cancellation)?;
			let records = profile::stage_profile(&profile_dir, &plan.profile_sources, cancellation)?;
			cache.write_new("Fallout - Invalidation.bsa", &empty_bsa_bytes())
				.context(ErrorMarker::environment_root_unsafe())?;
			check_cancelled(cancellation)?;
			write_manifest(&stage, &plan)?;
			check_cancelled(cancellation)?;
			validate_stage(&stage, LayoutLocation::Stage).map_err(map_initialization_error)?;
			check_cancelled(cancellation)?;
			for directory in [&mods, &profile_dir, &overwrite, &cache] {
				sync_tree(directory).context(ErrorMarker::environment_root_unsafe())?;
				check_cancelled(cancellation)?;
			}
			stage.sync().context(ErrorMarker::environment_root_unsafe())?;
			failure.check(PublicationPoint::StageDurable)
				.context(ErrorMarker::environment_root_unsafe())?;
			check_cancelled(cancellation)?;
			Ok(records)
		})();
		let records = match staged {
			Ok(records) => records,
			Err(error) => {
				drop(stage);
				drop(operation_dir);
				if error.current_context().code() == ErrorCode::OperationCancelled {
					return Err(error);
				}
				return Err(cleanup_preserving_error(error, &temp, &operation_name));
			}
		};
		if cancellation.is_cancelled() {
			drop(stage);
			drop(operation_dir);
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		if let Err(error) = check_cancelled(cancellation) {
			drop(stage);
			drop(operation_dir);
			return Err(error);
		}
		if let Err(error) = recovery::write_record(&operation_dir, operation_id, failure, cancellation) {
			drop(stage);
			drop(operation_dir);
			if error.current_context().code() == ErrorCode::OperationCancelled {
				return Err(error);
			}
			return Err(cleanup_preserving_error(error, &temp, &operation_name));
		}
		drop(stage);
		drop(operation_dir);

		let first_attempt = failure
			.check(PublicationPoint::MarkerPublished)
			.context(ErrorMarker::environment_recovery_failed(Some("publication")))
			.and_then(|()| check_cancelled(cancellation))
			.and_then(|()| {
				recovery::complete_initialization(
					&root_dir,
					&temp,
					&operation_name,
					failure,
					Some(cancellation),
				)
			});
		match first_attempt {
			Ok(()) => Ok(records),
			Err(first_error) if first_error.current_context().code() == ErrorCode::OperationCancelled => {
				Err(first_error)
			}
			Err(first_error) if cancellation.is_cancelled() => {
				Err(combine_errors(report!(ErrorMarker::operation_cancelled()), first_error))
			}
			Err(first_error) => {
				let mut no_failure = NoPublicationFailure;
				match recovery::complete_initialization(
					&root_dir,
					&temp,
					&operation_name,
					&mut no_failure,
					Some(cancellation),
				) {
					Ok(()) => Ok(records),
					Err(retry_error)
						if retry_error.current_context().code()
							== ErrorCode::OperationCancelled =>
					{
						Err(combine_errors(retry_error, first_error))
					}
					Err(retry_error) => {
						let publication_error = combine_errors(first_error, retry_error);
						match recovery::rollback_initialization(
							&root_dir,
							&temp,
							&operation_name,
							Some(cancellation),
						) {
							Ok(()) => Err(publication_error),
							Err(rollback_error) => {
								Err(combine_errors(publication_error, rollback_error))
							}
						}
					}
				}
			}
		}
	}

	pub fn recover_port(&self) -> RecoverEnvironment {
		let adapter = self.clone();
		Arc::new(move |root, cancellation| {
			let result = adapter.recover(&root, &cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn assess_port(&self) -> AssessInitializationTarget {
		let adapter = self.clone();
		Arc::new(move |root, cancellation| {
			let result = adapter.assess(&root, &cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn publish_port(&self) -> PublishEnvironment {
		let adapter = self.clone();
		Arc::new(move |root, plan, cancellation| {
			let result = adapter.publish(&root, plan, &cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
	schema_version: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	steam_app_id: u32,
	game_dir: String,
	observed_build_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublicationPoint {
	StageDurable,
	MarkerTempDurable,
	MarkerPublished,
	ModsPublished,
	ProfilePublished,
	OverwritePublished,
	CachePublished,
	ManifestPublished,
	BeforeFinalValidation,
}

impl fmt::Display for PublicationPoint {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "{self:?}")
	}
}

pub(crate) trait PublicationFailure {
	fn check(&mut self, point: PublicationPoint) -> Result<(), io::Error>;
}

pub(crate) struct NoPublicationFailure;

impl PublicationFailure for NoPublicationFailure {
	fn check(&mut self, _point: PublicationPoint) -> Result<(), io::Error> {
		Ok(())
	}
}

#[derive(Clone, Copy)]
pub(crate) enum LayoutLocation {
	Stage,
	Root,
}

fn assess_open(
	root: &SafeDir,
	cancellation: &CancellationToken,
) -> Result<InitializationTargetAssessment, ErrorMarker> {
	check_cancelled(cancellation)?;
	let entries = root.entries().context(ErrorMarker::environment_root_unsafe())?;
	if entries.iter().any(|name| name == OsStr::new("mods.toml")) {
		return Err(report!(ErrorMarker::environment_already_initialized()));
	}
	for name in entries {
		check_cancelled(cancellation)?;
		match name.to_str() {
			Some("logs") => {
				root.open_dir("logs").context(ErrorMarker::environment_root_unsafe())?;
			}
			_ => {
				return Err(report!(ErrorMarker::environment_root_not_empty()));
			}
		}
	}
	check_cancelled(cancellation)?;
	Ok(InitializationTargetAssessment::Available)
}

fn map_initialization_error(report: Report<ErrorMarker>) -> Report<ErrorMarker> {
	if report.current_context().code() == ErrorCode::EnvironmentInvalid {
		report.context(ErrorMarker::game_install_invalid())
	} else {
		report
	}
}

fn write_manifest(stage: &SafeDir, plan: &InitializationPlan) -> Result<(), ErrorMarker> {
	let game_dir = plan
		.game_binding
		.game_directory()
		.as_path()
		.to_str()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	let contents = toml::to_string_pretty(&Manifest {
		schema_version: 1,
		name: None,
		steam_app_id: plan.game_binding.steam_app_id().get(),
		game_dir: game_dir.to_owned(),
		observed_build_id: plan.game_binding.observed_build_id().get(),
	})
	.context(ErrorMarker::environment_invalid(None))?;
	stage.write_new("mods.toml", contents.as_bytes())
		.context(ErrorMarker::environment_root_unsafe())
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

pub(crate) fn validate_stage(directory: &SafeDir, location: LayoutLocation) -> Result<(), ErrorMarker> {
	let allowed = match location {
		LayoutLocation::Stage => &["mods", "profile", "overwrite", "cache", "mods.toml"][..],
		LayoutLocation::Root => &["mods", "profile", "overwrite", "cache", "mods.toml", "temp", "logs"][..],
	};
	validate_exact_entries(directory, allowed)?;
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
	validate_exact_entries(&mods, &[])?;
	validate_exact_entries(&overwrite, &[])?;
	validate_exact_entries(&cache, &["Fallout - Invalidation.bsa"])?;
	profile::validate_profile(&profile_dir)?;
	let manifest = validate_manifest_file(directory)?;
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
	validate_bsa_file(&cache)
}

pub(crate) fn validate_recovery(root: &SafeDir, stage: &SafeDir) -> Result<(), ErrorMarker> {
	validate_exact_entries(stage, &["mods", "profile", "overwrite", "cache", "mods.toml"])?;
	for name in ["mods", "profile", "overwrite", "cache", "mods.toml"] {
		if !stage.exists(name).context(ErrorMarker::environment_invalid(None))?
			&& !root.exists(name).context(ErrorMarker::environment_invalid(None))?
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	let mods = choose_dir(root, stage, "mods")?;
	let profile_dir = choose_dir(root, stage, "profile")?;
	let overwrite = choose_dir(root, stage, "overwrite")?;
	let cache = choose_dir(root, stage, "cache")?;
	validate_exact_entries(&mods, &[])?;
	validate_exact_entries(&overwrite, &[])?;
	validate_exact_entries(&cache, &["Fallout - Invalidation.bsa"])?;
	profile::validate_profile(&profile_dir)?;
	let manifest_dir = if stage
		.exists("mods.toml")
		.context(ErrorMarker::environment_invalid(None))?
	{
		stage
	} else {
		root
	};
	let manifest = validate_manifest_file(manifest_dir)?;
	let game = SafeDir::open_absolute(Path::new(&manifest.game_dir))
		.context(ErrorMarker::environment_invalid(None))?;
	if root.is_ancestor_of(&game)
		.context(ErrorMarker::environment_invalid(None))?
		|| game.is_ancestor_of(root)
			.context(ErrorMarker::environment_invalid(None))?
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	validate_bsa_file(&cache)
}

fn choose_dir(root: &SafeDir, stage: &SafeDir, name: &str) -> Result<SafeDir, ErrorMarker> {
	if stage.exists(name).context(ErrorMarker::environment_invalid(None))? {
		stage.open_dir(name).context(ErrorMarker::environment_invalid(None))
	} else {
		root.open_dir(name).context(ErrorMarker::environment_invalid(None))
	}
}

fn validate_manifest_file(directory: &SafeDir) -> Result<Manifest, ErrorMarker> {
	let contents = directory
		.read_regular("mods.toml")
		.context(ErrorMarker::environment_invalid(None))?;
	let text = from_utf8(&contents).context(ErrorMarker::environment_invalid(None))?;
	let manifest: Manifest = toml::from_str(text).context(ErrorMarker::environment_invalid(None))?;
	if manifest.schema_version != 1
		|| SteamAppId::new(manifest.steam_app_id).is_err()
		|| SteamBuildId::new(manifest.observed_build_id).is_err()
		|| GameInstallationPath::new(manifest.game_dir.clone().into()).is_err()
		|| manifest
			.name
			.as_deref()
			.is_some_and(|name| EnvironmentName::new(name.to_owned()).is_err())
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(manifest)
}

fn validate_exact_entries(directory: &SafeDir, allowed: &[&str]) -> Result<(), ErrorMarker> {
	let mut remaining = allowed.iter().copied().collect::<HashSet<_>>();
	for name in directory.entries().context(ErrorMarker::environment_invalid(None))? {
		let text = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if !remaining.remove(text) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let metadata = directory
			.symlink_metadata(&name)
			.context(ErrorMarker::environment_invalid(None))?;
		if is_reparse(&metadata) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

fn validate_bsa_file(cache: &SafeDir) -> Result<(), ErrorMarker> {
	let bsa = cache
		.read_regular("Fallout - Invalidation.bsa")
		.context(ErrorMarker::environment_invalid(None))?;
	if bsa != empty_bsa_bytes() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

fn sync_tree(directory: &SafeDir) -> Result<(), io::Error> {
	for name in directory.entries()? {
		let metadata = directory.symlink_metadata(&name)?;
		if is_reparse(&metadata) {
			return Err(report!(io::Error::other("refusing to sync a symbolic link",)));
		}
		if metadata.is_dir() {
			sync_tree(&directory.open_dir(&name)?)?;
		} else if metadata.is_file() {
			directory.sync_file(&name)?;
		} else {
			return Err(report!(io::Error::other("refusing to sync a special file")));
		}
	}
	directory.sync()
}

pub(crate) fn check_cancelled(cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		Err(report!(ErrorMarker::operation_cancelled()))
	} else {
		Ok(())
	}
}

pub(crate) fn combine_errors(primary: Report<ErrorMarker>, secondary: Report<ErrorMarker>) -> Report<ErrorMarker> {
	let context = primary.current_context().clone();
	let mut reports = ReportCollection::new();
	reports.push(primary.into_dynamic().into_cloneable());
	reports.push(secondary.into_dynamic().into_cloneable());
	reports.context(context)
}

fn cleanup_preserving_error(error: Report<ErrorMarker>, temp: &SafeDir, operation_name: &str) -> Report<ErrorMarker> {
	match cleanup_before_marker(temp, operation_name) {
		Ok(()) => error,
		Err(cleanup_error) => combine_errors(error, cleanup_error),
	}
}

fn cleanup_before_marker(temp: &SafeDir, operation_name: &str) -> Result<(), ErrorMarker> {
	if temp.exists(operation_name)
		.context(ErrorMarker::environment_root_unsafe())?
	{
		temp.remove_dir_all(operation_name)
			.context(ErrorMarker::environment_root_unsafe())?;
		temp.sync().context(ErrorMarker::environment_root_unsafe())?;
	}
	Ok(())
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
mod tests {
	use super::*;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileFileDisposition;
	use application::ports::ProfileSource;
	use domain::GameBinding;
	use domain::SteamBuildId;
	use std::env::current_dir;
	use std::fs;
	use std::io;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	#[cfg(windows)]
	use std::os::windows::fs::symlink_dir;
	use std::path::PathBuf;
	use tempfile::TempDir;

	fn temp_dir() -> TempDir {
		TempDir::new_in(current_dir().expect("current dir must be available"))
			.expect("temp dir must be created")
	}

	fn environment_root(path: &Path) -> EnvironmentRoot {
		EnvironmentRoot::new(path.to_path_buf()).expect("test environment root must be valid")
	}

	fn plan(game: &Path) -> InitializationPlan {
		let files = profile::PROFILE_FILES
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

	fn error_at(point: PublicationPoint) -> io::Error {
		io::Error::other(format!("injected failure at {point}"))
	}

	struct FailOnce {
		wanted: PublicationPoint,
		failed: bool,
	}

	impl PublicationFailure for FailOnce {
		fn check(&mut self, actual: PublicationPoint) -> Result<(), io::Error> {
			if actual == self.wanted && !self.failed {
				self.failed = true;
				Err(report!(error_at(actual)))
			} else {
				Ok(())
			}
		}
	}

	struct CancelAt {
		wanted: PublicationPoint,
		cancellation: CancellationToken,
	}

	impl PublicationFailure for CancelAt {
		fn check(&mut self, actual: PublicationPoint) -> Result<(), io::Error> {
			if actual == self.wanted {
				self.cancellation.cancel();
			}
			Ok(())
		}
	}

	struct CorruptAfterMods {
		root: PathBuf,
		corrupted: bool,
	}

	impl PublicationFailure for CorruptAfterMods {
		fn check(&mut self, actual: PublicationPoint) -> Result<(), io::Error> {
			if actual == PublicationPoint::ModsPublished && !self.corrupted {
				self.corrupted = true;
				let operation = fs::read_dir(self.root.join("temp"))?
					.next()
					.ok_or_else(|| io::Error::other("operation missing"))??
					.path();
				fs::remove_dir_all(operation.join("stage/profile"))?;
				return Err(report!(error_at(actual)));
			}
			Ok(())
		}
	}

	fn assert_no_canonical_artifacts(root: &Path) {
		for name in ["mods", "profile", "overwrite", "cache", "mods.toml"] {
			assert!(!root.join(name).exists(), "unexpected artifact {name}");
		}
	}

	fn assert_cancelled<T>(result: Result<T, ErrorMarker>) {
		assert!(result.is_err());
		let report = result.err().expect("cancelled operation must return an error");
		assert_eq!(report.current_context().code(), ErrorCode::OperationCancelled);
	}

	#[test]
	fn initialization_publishes_complete_layout_and_manifest_last() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		let records = EnvironmentAdapter
			.publish(&root, plan(&game), &CancellationToken::new())
			.expect("publication must succeed");
		assert!(root.as_path().join("mods.toml").is_file());
		for directory in ["mods", "profile", "profile/saves", "overwrite", "cache", "temp"] {
			assert!(root.as_path().join(directory).is_dir());
		}
		assert_eq!(
			records.iter().map(|record| record.disposition).collect::<Vec<_>>(),
			vec![
				ProfileFileDisposition::SeededFromGame,
				ProfileFileDisposition::Absent,
				ProfileFileDisposition::Absent,
				ProfileFileDisposition::Absent,
				ProfileFileDisposition::Absent,
				ProfileFileDisposition::CreatedEmpty,
				ProfileFileDisposition::CreatedEmpty,
				ProfileFileDisposition::Absent,
			]
		);
	}

	#[test]
	fn nested_environment_root_is_created_through_verified_parent_handles() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("mods/environments/default"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		EnvironmentAdapter
			.publish(&root, plan(&game), &CancellationToken::new())
			.expect("nested environment must publish");
		assert!(root.as_path().join("mods.toml").is_file());
	}

	#[test]
	fn assessment_allows_only_logs_before_manifest() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		fs::create_dir_all(root.as_path().join("logs")).expect("logs must be created");
		assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_ok());

		fs::create_dir(root.as_path().join("temp")).expect("temp must be created");
		assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_err());
	}

	#[test]
	fn cancelled_recovery_preserves_abandoned_temporary_state() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let operation = root.as_path().join("temp").join(Uuid::new_v4().to_string());
		fs::create_dir_all(operation.join("stage")).expect("orphan stage must be created");
		fs::write(operation.join("operation.toml.tmp"), b"torn = [").expect("temporary marker must be written");

		let cancellation = CancellationToken::new();
		cancellation.cancel();
		assert_cancelled(EnvironmentAdapter.recover(&root, &cancellation));
		assert!(operation.join("stage").is_dir());
		assert!(operation.join("operation.toml.tmp").is_file());
	}

	#[test]
	fn cancellation_before_work_returns_typed_error_without_creating_state() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");

		let cancellation = CancellationToken::new();
		cancellation.cancel();
		assert_cancelled(EnvironmentAdapter.publish(&root, plan(&game), &cancellation));
		assert!(!root.as_path().exists());
	}

	#[test]
	fn cancellation_preserves_pre_and_post_marker_partial_state() {
		for wanted in [
			PublicationPoint::StageDurable,
			PublicationPoint::MarkerTempDurable,
			PublicationPoint::MarkerPublished,
			PublicationPoint::ModsPublished,
		] {
			let parent = temp_dir();
			let root = environment_root(&parent.path().join("environment"));
			let game = parent.path().join("game");
			fs::create_dir(&game).expect("game dir must be created");
			let cancellation = CancellationToken::new();
			let result = EnvironmentAdapter.publish_with_failure(
				&root,
				plan(&game),
				&mut CancelAt {
					wanted,
					cancellation: cancellation.clone(),
				},
				&cancellation,
			);

			assert_cancelled(result);
			assert!(!root.as_path().join("mods.toml").exists());
			let operation_dir = fs::read_dir(root.as_path().join("temp"))
				.expect("temporary state must remain")
				.next()
				.expect("operation must remain")
				.expect("operation entry must be readable")
				.path();
			assert!(operation_dir.join("stage").is_dir());
			assert_eq!(
				operation_dir.join("operation.toml").exists(),
				matches!(
					wanted,
					PublicationPoint::MarkerPublished | PublicationPoint::ModsPublished
				)
			);
			assert_eq!(
				root.as_path().join("mods").is_dir(),
				wanted == PublicationPoint::ModsPublished
			);
		}
	}

	#[test]
	fn cancellation_after_manifest_publication_returns_success() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		let cancellation = CancellationToken::new();

		EnvironmentAdapter
			.publish_with_failure(
				&root,
				plan(&game),
				&mut CancelAt {
					wanted: PublicationPoint::ManifestPublished,
					cancellation: cancellation.clone(),
				},
				&cancellation,
			)
			.expect("final published manifest makes cancellation too late");
		assert!(root.as_path().join("mods.toml").is_file());
	}

	#[test]
	fn pre_marker_failures_require_recovery_before_assessment() {
		for wanted in [PublicationPoint::StageDurable, PublicationPoint::MarkerTempDurable] {
			let parent = temp_dir();
			let root = environment_root(&parent.path().join("environment"));
			let game = parent.path().join("game");
			fs::create_dir(&game).expect("game dir must be created");
			assert!(EnvironmentAdapter
				.publish_with_failure(
					&root,
					plan(&game),
					&mut FailOnce { wanted, failed: false },
					&CancellationToken::new(),
				)
				.is_err());
			assert_no_canonical_artifacts(root.as_path());
			assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_err());
			assert_eq!(
				EnvironmentAdapter
					.recover(&root, &CancellationToken::new())
					.expect("empty temporary directory must recover"),
				RecoveryOutcome::NothingToRecover
			);
			assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_ok());
		}
	}

	#[test]
	fn failures_during_each_canonical_move_settle_to_a_valid_commit() {
		for wanted in [
			PublicationPoint::MarkerPublished,
			PublicationPoint::ModsPublished,
			PublicationPoint::ProfilePublished,
			PublicationPoint::OverwritePublished,
			PublicationPoint::CachePublished,
			PublicationPoint::ManifestPublished,
			PublicationPoint::BeforeFinalValidation,
		] {
			let parent = temp_dir();
			let root = environment_root(&parent.path().join("environment"));
			let game = parent.path().join("game");
			fs::create_dir(&game).expect("game dir must be created");
			EnvironmentAdapter
				.publish_with_failure(
					&root,
					plan(&game),
					&mut FailOnce { wanted, failed: false },
					&CancellationToken::new(),
				)
				.expect("same invocation must finish the durable publication");
			assert!(root.as_path().join("mods.toml").is_file());
			assert_eq!(
				EnvironmentAdapter
					.recover(&root, &CancellationToken::new())
					.expect("completed environment recovery must be a no-op"),
				RecoveryOutcome::NothingToRecover
			);
		}
	}

	#[test]
	fn an_unfinishable_durable_publication_rolls_back_and_preserves_logs() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		fs::create_dir_all(root.as_path().join("logs")).expect("logs must be created");
		fs::write(root.as_path().join("logs/session.log"), b"keep").expect("log must be written");
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		let root_path = root.as_path().to_path_buf();
		let result = EnvironmentAdapter.publish_with_failure(
			&root,
			plan(&game),
			&mut CorruptAfterMods {
				root: root_path,
				corrupted: false,
			},
			&CancellationToken::new(),
		);
		assert!(result.is_err());
		assert_no_canonical_artifacts(root.as_path());
		assert_eq!(
			fs::read(root.as_path().join("logs/session.log")).expect("log must survive"),
			b"keep"
		);
		assert_eq!(
			EnvironmentAdapter
				.recover(&root, &CancellationToken::new())
				.expect("rollback must be terminal"),
			RecoveryOutcome::NothingToRecover
		);
		assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_ok());
	}

	#[test]
	fn recovery_resumes_a_partially_moved_durable_publication() {
		let parent = temp_dir();
		let root_value = environment_root(&parent.path().join("environment"));
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game dir must be created");
		let root = SafeDir::create_absolute(root_value.as_path()).expect("root must open");
		let temp = root.create_dir("temp").expect("temp must be created");
		let operation_id = Uuid::new_v4();
		let operation_name = operation_id.to_string();
		let operation = temp.create_dir(&operation_name).expect("operation must be created");
		let stage = operation.create_dir("stage").expect("stage must be created");
		stage.create_dir("mods").expect("mods must be created");
		let profile_dir = stage.create_dir("profile").expect("profile must be created");
		stage.create_dir("overwrite").expect("overwrite must be created");
		let cache = stage.create_dir("cache").expect("cache must be created");
		profile::stage_profile(&profile_dir, &plan(&game).profile_sources, &CancellationToken::new())
			.expect("profile must stage");
		cache.write_new("Fallout - Invalidation.bsa", &empty_bsa_bytes())
			.expect("BSA must stage");
		write_manifest(&stage, &plan(&game)).expect("manifest must stage");
		recovery::write_record(
			&operation,
			operation_id,
			&mut NoPublicationFailure,
			&CancellationToken::new(),
		)
		.expect("marker must become durable");
		stage.rename_to("mods", &root, "mods").expect("first move must succeed");
		drop(profile_dir);
		drop(cache);
		drop(stage);
		drop(operation);
		drop(temp);
		drop(root);

		assert_eq!(
			EnvironmentAdapter
				.recover(&root_value, &CancellationToken::new())
				.expect("recovery must complete"),
			RecoveryOutcome::Committed
		);
		assert!(root_value.as_path().join("mods.toml").is_file());
	}

	#[test]
	fn recovery_discards_pre_marker_and_torn_temp_orphans() {
		for temp_marker in [false, true] {
			let parent = temp_dir();
			let root = environment_root(&parent.path().join("environment"));
			let operation = root.as_path().join("temp").join(Uuid::new_v4().to_string());
			fs::create_dir_all(operation.join("stage")).expect("orphan stage must be created");
			if temp_marker {
				fs::write(operation.join("operation.toml.tmp"), b"torn = [")
					.expect("torn temporary marker must be written");
			}
			assert_eq!(
				EnvironmentAdapter
					.recover(&root, &CancellationToken::new())
					.expect("orphan recovery must succeed"),
				RecoveryOutcome::NothingToRecover
			);
			assert!(!operation.exists());
			assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_ok());
		}
	}

	#[test]
	fn recovery_rejects_a_malformed_durable_marker_without_changing_it() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let operation = root.as_path().join("temp").join(Uuid::new_v4().to_string());
		let stage = operation.join("stage");
		fs::create_dir_all(&stage).expect("orphan stage must be created");
		let marker = operation.join("operation.toml");
		fs::write(&marker, b"torn = [").expect("malformed marker must be written");

		assert!(EnvironmentAdapter.recover(&root, &CancellationToken::new()).is_err());
		assert_eq!(fs::read(marker).expect("marker must remain"), b"torn = [");
		assert!(stage.is_dir());
	}

	#[test]
	fn recovery_rejects_an_unknown_durable_schema_without_changing_it() {
		let parent = temp_dir();
		let root = environment_root(&parent.path().join("environment"));
		let operation_id = Uuid::new_v4();
		let operation = root.as_path().join("temp").join(operation_id.to_string());
		let stage = operation.join("stage");
		fs::create_dir_all(&stage).expect("orphan stage must be created");
		let contents = format!(
			concat!(
				"schema_version = 2\n",
				"operation = \"initialize\"\n",
				"operation_id = \"{}\"\n",
				"phase = \"publishing\"\n",
			),
			operation_id,
		);
		let marker = operation.join("operation.toml");
		fs::write(&marker, &contents).expect("unknown marker must be written");

		assert!(EnvironmentAdapter.recover(&root, &CancellationToken::new()).is_err());
		assert_eq!(fs::read_to_string(marker).expect("marker must remain"), contents);
		assert!(stage.is_dir());
	}

	#[cfg(unix)]
	#[test]
	fn final_root_and_managed_entry_symlinks_are_rejected_without_following() {
		let parent = temp_dir();
		let external = parent.path().join("external");
		fs::create_dir(&external).expect("external must be created");
		let root_path = parent.path().join("environment");
		symlink(&external, &root_path).expect("root symlink must be created");
		let root = environment_root(&root_path);
		assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_err());
		assert!(fs::read_dir(&external)
			.expect("external must remain readable")
			.next()
			.is_none());

		let ancestor_link = parent.path().join("ancestor-link");
		symlink(&external, &ancestor_link).expect("ancestor symlink must be created");
		let nested_root = environment_root(&ancestor_link.join("nested"));
		assert!(EnvironmentAdapter
			.publish(
				&nested_root,
				plan(&parent.path().join("game-not-needed")),
				&CancellationToken::new(),
			)
			.is_err());
		assert!(!external.join("nested").exists());

		fs::remove_file(&root_path).expect("root symlink must be removed");
		fs::create_dir(&root_path).expect("root must be created");
		symlink(&external, root_path.join("temp")).expect("temp symlink must be created");
		let game = parent.path().join("game");
		fs::create_dir(&game).expect("game must be created");
		assert!(EnvironmentAdapter
			.publish(&root, plan(&game), &CancellationToken::new())
			.is_err());
		assert!(fs::read_dir(&external)
			.expect("external must remain readable")
			.next()
			.is_none());
	}

	#[cfg(windows)]
	#[test]
	#[expect(clippy::panic, reason = "unexpected Windows fixture errors must fail the test")]
	fn windows_root_directory_symlink_is_rejected_without_following() {
		let parent = temp_dir();
		let external = parent.path().join("external");
		fs::create_dir(&external).expect("external must be created");
		let root_path = parent.path().join("environment");
		match symlink_dir(&external, &root_path) {
			Ok(()) => {
				let root = environment_root(&root_path);
				assert!(EnvironmentAdapter.assess(&root, &CancellationToken::new()).is_err());
				assert!(fs::read_dir(&external)
					.expect("external must remain readable")
					.next()
					.is_none());
			}
			Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
			Err(error) => panic!("unexpected symlink creation error: {error}"),
		}
	}

	#[test]
	fn empty_bsa_is_the_complete_known_tes4_header() {
		let bytes = empty_bsa_bytes();
		assert_eq!(bytes.len(), 36);
		assert_eq!(&bytes[..4], b"BSA\0");
		assert_eq!(
			u32::from_le_bytes(bytes[4..8].try_into().expect("version must fit")),
			0x68
		);
		assert_eq!(
			u32::from_le_bytes(bytes[8..12].try_into().expect("offset must fit")),
			36
		);
		assert_eq!(
			u32::from_le_bytes(bytes[12..16].try_into().expect("flags must fit")),
			0x3
		);
	}

	#[test]
	fn report_keeps_the_original_io_error_below_the_safe_marker() {
		let parent = temp_dir();
		let root_path = parent.path().join("not-a-directory");
		fs::write(&root_path, b"file").expect("fixture file must be created");
		let report = EnvironmentAdapter
			.assess(&environment_root(&root_path), &CancellationToken::new())
			.expect_err("assessment must fail");
		assert_eq!(report.current_context().code(), ErrorCode::EnvironmentRootUnsafe);
		assert!(report
			.iter_reports()
			.any(|node| { node.downcast_current_context::<io::Error>().is_some() }));
	}
}
