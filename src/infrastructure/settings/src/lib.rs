#![cfg_attr(test, feature(fn_traits))]

mod config_source;
mod fs_access;
mod layout;
mod manifest_writer;

use application::ErrorMarker;
use application::ports::CheckSettingsReadiness;
use application::ports::LoadSettings;
use application::ports::PortFuture;
use application::ports::PreviewGameBinding;
use application::ports::ReadInitializationGameOverride;
use application::ports::StoreGameBinding;
use application::ports::StoredAndEffectiveBinding;
use application::settings::ResolvedSettings;
use application::settings::SettingKey;
use application::settings::SettingRecord;
use application::settings::SettingSource;
use application::settings::SettingValue;
use cap_std::fs::Dir;
use config_source::RawManifest;
use domain::EnvironmentName;
use domain::EnvironmentRoot;
use domain::EnvironmentSchemaVersion;
use domain::GameBinding;
use domain::GameInstallationPath;
use domain::SteamAppId;
use domain::SteamBuildId;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use toml::from_str;

#[derive(Clone)]
pub struct SettingsAdapter {
	root: EnvironmentRoot,
	environment: Arc<Vec<(OsString, OsString)>>,
}

impl SettingsAdapter {
	pub fn new(root: EnvironmentRoot) -> Self {
		Self {
			root,
			environment: Arc::new(env::vars_os().collect()),
		}
	}

	#[cfg(test)]
	fn with_environment(root: EnvironmentRoot, environment: Vec<(OsString, OsString)>) -> Self {
		Self {
			root,
			environment: Arc::new(environment),
		}
	}

	fn check_readiness(&self, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		refuse_unfinished_operation_before_layout(&self.root, SettingsAccess::Mutation)?;

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		Ok(())
	}

	/// Reads the effective binding while execution owns Profile State validation.
	///
	/// # Errors
	/// Refuses pending work, invalid manifests or overrides, unsafe roots, and cancellation.
	pub fn load_execution_binding(&self, cancellation: &CancellationToken) -> Result<GameBinding, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		refuse_unfinished_operation_before_layout(&self.root, SettingsAccess::Mutation)?;
		let (root, text) = open_manifest_root(&self.root)?;
		refuse_unfinished_operation(&root, SettingsAccess::Mutation)?;

		let (manifest, effective, shadowed) = config_source::read_sources(
			&text,
			&self.environment,
			ErrorMarker::settings_environment_invalid(),
		)?;
		let settings = resolved_settings(manifest, effective, shadowed)?;

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		Ok(settings.effective_binding)
	}

	fn load(&self) -> Result<ResolvedSettings, ErrorMarker> {
		refuse_unfinished_operation_before_layout(&self.root, SettingsAccess::ReadOnly)?;
		let (root, text) = open_bound_root(&self.root)?;
		refuse_unfinished_operation(&root, SettingsAccess::ReadOnly)?;
		let (manifest, effective, shadowed) = config_source::read_sources(
			&text,
			&self.environment,
			ErrorMarker::settings_environment_invalid(),
		)?;
		resolved_settings(manifest, effective, shadowed)
	}

	fn initialization_override(&self) -> Result<Option<GameInstallationPath>, ErrorMarker> {
		let Some(value) = config_source::initialization_override(&self.environment)? else {
			return Ok(None);
		};
		let path = GameInstallationPath::new(value.into()).context(ErrorMarker::game_install_invalid())?;
		Ok(Some(path))
	}

	fn preview_game_binding(&self, binding: GameBinding) -> Result<StoredAndEffectiveBinding, ErrorMarker> {
		Ok(self.prepare_game_binding(binding)?.outcome)
	}

	fn prepare_game_binding(&self, binding: GameBinding) -> Result<PreparedGameBinding, ErrorMarker> {
		refuse_unfinished_operation_before_layout(&self.root, SettingsAccess::Mutation)?;
		let (root, text) = open_bound_root(&self.root)?;
		refuse_unfinished_operation(&root, SettingsAccess::Mutation)?;

		let (manifest, _, _) = config_source::read_sources(
			&text,
			&self.environment,
			ErrorMarker::settings_environment_invalid(),
		)?;
		validate_manifest(&manifest)?;

		let game_dir = binding
			.game_directory()
			.as_path()
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::setting_value_invalid()))?;
		let replacement = toml::to_string_pretty(&WritableManifest {
			schema_version: manifest.schema_version,
			name: manifest.name.as_deref(),
			steam_app_id: manifest.steam_app_id,
			game_dir,
			observed_build_id: binding.observed_build_id().get(),
		})
		.context(ErrorMarker::setting_value_invalid())?;

		let (replacement_manifest, replacement_effective, shadowed) = config_source::read_sources(
			&replacement,
			&self.environment,
			ErrorMarker::settings_environment_invalid(),
		)?;
		let resolved = resolved_settings(replacement_manifest, replacement_effective, shadowed)?;
		let game_record = resolved
			.settings
			.iter()
			.find(|record| record.key == SettingKey::GameDir)
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		let outcome = StoredAndEffectiveBinding {
			stored: binding,
			effective: resolved.effective_binding,
			source: game_record.source.clone(),
			shadowed: game_record.shadowed,
		};

		Ok(PreparedGameBinding {
			root,
			replacement,
			outcome,
		})
	}

	fn store_game_binding(
		&self,
		binding: GameBinding,
		cancellation: CancellationToken,
	) -> Result<StoredAndEffectiveBinding, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let prepared = self.prepare_game_binding(binding)?;

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		manifest_writer::replace_validated(
			&prepared.root,
			&prepared.replacement,
			&cancellation,
			|candidate| {
				from_str::<RawManifest>(candidate)
					.ok()
					.and_then(|raw| validate_manifest(&raw).ok())
					.is_some()
			},
		)?;
		Ok(prepared.outcome)
	}

	pub fn readiness_port(&self) -> CheckSettingsReadiness {
		let adapter = self.clone();
		Arc::new(move |cancellation| {
			let result = adapter.check_readiness(&cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn load_port(&self) -> LoadSettings {
		let adapter = self.clone();
		Arc::new(move || {
			let result = adapter.load();
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn preview_port(&self) -> PreviewGameBinding {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let result = (|| {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				let outcome = adapter.preview_game_binding(binding)?;

				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				Ok(outcome)
			})();
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn initialization_override_port(&self) -> ReadInitializationGameOverride {
		let adapter = self.clone();
		Arc::new(move || {
			let result = adapter.initialization_override();
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn store_port(&self) -> StoreGameBinding {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let result = adapter.store_game_binding(binding, cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}
}

struct PreparedGameBinding {
	root: Dir,
	replacement: String,
	outcome: StoredAndEffectiveBinding,
}

#[derive(Serialize)]
struct WritableManifest<'a> {
	schema_version: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<&'a str>,
	steam_app_id: u32,
	game_dir: &'a str,
	observed_build_id: u64,
}

fn open_bound_root(root: &EnvironmentRoot) -> Result<(Dir, String), ErrorMarker> {
	let (directory, text) = open_manifest_root(root)?;
	layout::validate(&directory)?;
	Ok((directory, text))
}

fn open_manifest_root(root: &EnvironmentRoot) -> Result<(Dir, String), ErrorMarker> {
	let directory = fs_access::open_ambient_dir(root.as_path()).map_err(|error| {
		let marker = if error.current_context().kind() == ErrorKind::NotFound {
			ErrorMarker::environment_not_initialized()
		} else {
			ErrorMarker::environment_root_unsafe()
		};
		error.context(marker)
	})?;

	let manifest_metadata = directory.symlink_metadata("mods.toml").map_err(|error| {
		let marker = if error.kind() == ErrorKind::NotFound {
			ErrorMarker::environment_not_initialized()
		} else {
			ErrorMarker::environment_root_unsafe()
		};
		report!(error).context(marker)
	})?;
	if !manifest_metadata.is_file() || fs_access::is_reparse(&manifest_metadata) {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}
	let mut manifest = fs_access::open_regular(&directory, Path::new("mods.toml"))
		.context(ErrorMarker::environment_root_unsafe())?;
	let mut text = String::new();
	manifest.read_to_string(&mut text)
		.context(ErrorMarker::environment_invalid(None))?;

	for name in ["mods", "profile", "overwrite", "temp"] {
		let metadata = directory.symlink_metadata(name).map_err(|error| {
			let marker = if error.kind() == ErrorKind::NotFound {
				ErrorMarker::environment_invalid(None)
			} else {
				ErrorMarker::environment_root_unsafe()
			};
			report!(error).context(marker)
		})?;
		if fs_access::is_reparse(&metadata) {
			return Err(report!(ErrorMarker::environment_root_unsafe()));
		}
		if !metadata.is_dir() {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		fs_access::open_dir(&directory, Path::new(name)).context(ErrorMarker::environment_root_unsafe())?;
	}
	Ok((directory, text))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsAccess {
	ReadOnly,
	Mutation,
}

fn refuse_unfinished_operation_before_layout(
	root: &EnvironmentRoot,
	access: SettingsAccess,
) -> Result<(), ErrorMarker> {
	let directory = match fs_access::open_ambient_dir(root.as_path()) {
		Ok(directory) => directory,
		Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(()),
		Err(error) => return Err(error.context(ErrorMarker::environment_root_unsafe())),
	};
	let temp_metadata = match directory.symlink_metadata("temp") {
		Ok(metadata) => metadata,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
		Err(error) => return Err(report!(error).context(ErrorMarker::environment_root_unsafe())),
	};
	if fs_access::is_reparse(&temp_metadata) {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}
	if !temp_metadata.is_dir() {
		return Ok(());
	}
	let temp = fs_access::open_dir(&directory, Path::new("temp")).context(ErrorMarker::environment_root_unsafe())?;
	refuse_unfinished_operation_in_temp(&temp, access)
}

fn refuse_unfinished_operation(root: &Dir, access: SettingsAccess) -> Result<(), ErrorMarker> {
	let temp = fs_access::open_dir(root, Path::new("temp")).context(ErrorMarker::environment_root_unsafe())?;
	refuse_unfinished_operation_in_temp(&temp, access)
}

fn refuse_unfinished_operation_in_temp(temp: &Dir, access: SettingsAccess) -> Result<(), ErrorMarker> {
	let mut entries = temp.entries().context(ErrorMarker::environment_root_unsafe())?;
	if entries
		.next()
		.transpose()
		.context(ErrorMarker::environment_root_unsafe())?
		.is_some()
	{
		let marker = match access {
			SettingsAccess::ReadOnly => ErrorMarker::environment_invalid(None),
			SettingsAccess::Mutation => ErrorMarker::manual_cleanup_required(),
		};
		return Err(report!(marker));
	}
	Ok(())
}
fn validate_manifest(raw: &RawManifest) -> Result<(GameBinding, Option<EnvironmentName>), ErrorMarker> {
	EnvironmentSchemaVersion::new(raw.schema_version).context(ErrorMarker::environment_schema_unsupported())?;
	SteamAppId::new(raw.steam_app_id).context(ErrorMarker::environment_invalid(None))?;
	let build = SteamBuildId::new(raw.observed_build_id).context(ErrorMarker::environment_invalid(None))?;
	let path = GameInstallationPath::new(raw.game_dir.clone().into())
		.context(ErrorMarker::environment_invalid(None))?;
	let name =
		raw.name.clone()
			.map(EnvironmentName::new)
			.transpose()
			.context(ErrorMarker::environment_invalid(None))?;
	Ok((GameBinding::new(path, build), name))
}

fn resolved_settings(
	manifest: RawManifest,
	effective: RawManifest,
	shadowed: bool,
) -> Result<ResolvedSettings, ErrorMarker> {
	let (manifest_binding, _) = validate_manifest(&manifest)?;
	let (effective_binding, _) = validate_manifest(&effective)?;
	let manifest_values = [
		SettingValue::UnsignedInteger(u64::from(manifest.schema_version)),
		manifest.name.clone().map_or(SettingValue::Unset, SettingValue::String),
		SettingValue::UnsignedInteger(u64::from(manifest.steam_app_id)),
		SettingValue::Path(manifest_binding.game_directory().as_path().to_path_buf()),
		SettingValue::UnsignedInteger(manifest.observed_build_id),
	];
	let effective_values = [
		SettingValue::UnsignedInteger(u64::from(effective.schema_version)),
		effective.name.map_or(SettingValue::Unset, SettingValue::String),
		SettingValue::UnsignedInteger(u64::from(effective.steam_app_id)),
		SettingValue::Path(effective_binding.game_directory().as_path().to_path_buf()),
		SettingValue::UnsignedInteger(effective.observed_build_id),
	];
	let settings = SettingKey::ALL
		.into_iter()
		.zip(effective_values)
		.zip(manifest_values)
		.map(|((key, value), manifest_value)| {
			let is_game_dir = key == SettingKey::GameDir;
			SettingRecord {
				key,
				value,
				source: if is_game_dir && shadowed {
					SettingSource::Environment {
						variable: "MODS_GAME_DIR",
					}
				} else {
					SettingSource::Manifest
				},
				manifest_value,
				manifest_path: key.manifest_path(),
				shadowed: is_game_dir && shadowed,
				writable: is_game_dir,
			}
		})
		.collect();
	Ok(ResolvedSettings {
		settings,
		effective_binding,
		manifest_binding,
	})
}

#[cfg(test)]
mod tests {
	use super::SettingsAdapter;
	use super::fs_access;
	use super::manifest_writer;
	use application::ErrorCode;
	use application::ErrorMarker;
	use application::settings::SettingKey;
	use application::settings::SettingSource;
	use application::settings::SettingValue;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use rootcause::Result;
	use rootcause::report;
	use std::ffi::OsString;
	use std::fs;
	use std::io::Error as IoError;
	use std::io::Result as IoResult;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	#[cfg(windows)]
	use std::os::windows::fs::symlink_dir as windows_symlink_dir;
	#[cfg(windows)]
	use std::os::windows::fs::symlink_file as windows_symlink_file;
	use std::path::Path;
	use std::task::Context;
	use std::task::Poll;
	use std::task::Waker;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[cfg(not(windows))]
	const MANIFEST_GAME_DIR: &str = "/games/fnv";
	#[cfg(windows)]
	const MANIFEST_GAME_DIR: &str = "C:/games/fnv";
	#[cfg(not(windows))]
	const OVERRIDE_GAME_DIR: &str = "/portable/fnv";
	#[cfg(windows)]
	const OVERRIDE_GAME_DIR: &str = "C:/portable/fnv";
	#[cfg(not(windows))]
	const STORED_GAME_DIR: &str = "/other/fnv";
	#[cfg(windows)]
	const STORED_GAME_DIR: &str = "C:/other/fnv";

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

	fn fixture() -> Result<(TempDir, EnvironmentRoot)> {
		let temp = TempDir::new()?;
		let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?)?;
		for directory in ["mods", "profile", "profile/saves", "overwrite", "cache", "temp"] {
			fs::create_dir(temp.path().join(directory))?;
		}
		fs::write(
			temp.path().join("profile/Fallout.ini"),
			concat!(
				"[General]\nbUseMyGamesDirectory=1\nSLocalSavePath=__mods_saves\\\n",
				"[Archive]\nbInvalidateOlderFiles=1\n",
				"SInvalidationFile=\n",
				"sArchiveList=Fallout - Invalidation.bsa\n",
			),
		)?;
		for file in ["plugins.txt", "loadorder.txt", "modlist.txt"] {
			fs::write(temp.path().join("profile").join(file), b"")?;
		}
		fs::write(temp.path().join("cache/Fallout - Invalidation.bsa"), empty_bsa_bytes())?;
		fs::write(
			temp.path().join("mods.toml"),
			format!(
				concat!(
					"schema_version = 1\nname = \"Mojave\"\nsteam_app_id = 22380\n",
					"game_dir = \"{MANIFEST_GAME_DIR}\"\nobserved_build_id = 42\n",
				),
				MANIFEST_GAME_DIR = MANIFEST_GAME_DIR,
			),
		)?;
		Ok((temp, root))
	}

	#[test]
	fn execution_binding_does_not_require_plugin_lists_or_inspect_saves() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::remove_file(temp.path().join("profile/plugins.txt"))?;
		fs::remove_file(temp.path().join("profile/loadorder.txt"))?;
		let adapter = SettingsAdapter::with_environment(root, Vec::new());
		let binding = adapter.load_execution_binding(&CancellationToken::new())?;
		assert_eq!(binding.game_directory().as_path(), Path::new(MANIFEST_GAME_DIR));
		assert!(!temp.path().join("profile/plugins.txt").exists());
		fs::create_dir(temp.path().join("temp/pending"))?;
		let result = adapter.load_execution_binding(&CancellationToken::new());
		assert!(
			matches!(result, Err(error) if error.current_context().code() == ErrorCode::ManualCleanupRequired)
		);
		Ok(())
	}

	#[test]
	fn load_returns_exact_registry_order_and_provenance() -> Result<()> {
		let (_temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(root, Vec::new());
		let resolved = adapter.load()?;
		assert_eq!(
			resolved.settings.iter().map(|record| record.key).collect::<Vec<_>>(),
			SettingKey::ALL
		);
		assert!(resolved
			.settings
			.iter()
			.all(|record| record.source == SettingSource::Manifest));
		Ok(())
	}

	#[test]
	fn environment_override_shadows_only_game_directory() -> Result<()> {
		let (_temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![(OsString::from("mods_game_dir"), OsString::from(OVERRIDE_GAME_DIR))],
		);
		let resolved = adapter.load()?;
		let record = &resolved.settings[3];
		assert_eq!(record.value, SettingValue::Path(OVERRIDE_GAME_DIR.into()));
		assert_eq!(record.manifest_value, SettingValue::Path(MANIFEST_GAME_DIR.into()));
		assert!(record.shadowed);
		Ok(())
	}

	#[test]
	fn load_port_refuses_unfinished_pre_manifest_work_without_mutating() -> Result<()> {
		let temp = TempDir::new()?;
		let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?)?;
		let operation = temp.path().join("temp/operation");
		fs::create_dir_all(&operation)?;
		let marker = operation.join("mods.toml");
		fs::write(&marker, "pending")?;
		let before = fs::read(&marker)?;

		let load = SettingsAdapter::with_environment(root, Vec::new()).load_port();
		let mut future = load.call(());
		let context = &mut Context::from_waker(Waker::noop());
		let Poll::Ready(result) = future.as_mut().poll(context) else {
			return Err(report!(ErrorMarker::environment_invalid(None)).into());
		};

		assert!(matches!(
			result,
			Err(report)
				if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read(marker)?, before);
		assert!(!temp.path().join("mods.toml").exists());
		Ok(())
	}

	#[test]
	fn unfinished_work_blocks_read_without_mutating() -> Result<()> {
		let (temp, root) = fixture()?;
		let operation = temp.path().join("temp/operation");
		fs::create_dir(&operation)?;
		fs::write(operation.join("operation.toml"), "schema_version = 1")?;
		let before = fs::read(temp.path().join("mods.toml"))?;
		let error = SettingsAdapter::with_environment(root, Vec::new()).load();
		assert!(matches!(
			error,
			Err(report)
				if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read(temp.path().join("mods.toml")).ok(), Some(before));
		Ok(())
	}

	#[test]
	fn invalid_flat_manifests_fail_unchanged() -> Result<()> {
		let invalid = [
			format!("steam_app_id = 22380\ngame_dir = \"{MANIFEST_GAME_DIR}\"\nobserved_build_id = 42\n"),
			format!(
				concat!(
					"schema_version = 2\nsteam_app_id = 22380\n",
					"game_dir = \"{MANIFEST_GAME_DIR}\"\nobserved_build_id = 42\n",
				),
				MANIFEST_GAME_DIR = MANIFEST_GAME_DIR,
			),
			format!(
				concat!(
					"schema_version = 1\nsteam_app_id = 1\n",
					"game_dir = \"{MANIFEST_GAME_DIR}\"\nobserved_build_id = 42\n",
				),
				MANIFEST_GAME_DIR = MANIFEST_GAME_DIR,
			),
			format!(
				concat!(
					"schema_version = 1\nsteam_app_id = 22380\n",
					"game_dir = \"{MANIFEST_GAME_DIR}\"\nobserved_build_id = 0\n",
				),
				MANIFEST_GAME_DIR = MANIFEST_GAME_DIR,
			),
			"schema_version = 1\nsteam_app_id = 22380\ngame_dir = \"relative\"\nobserved_build_id = 42\n"
				.to_owned(),
			format!(
				concat!(
					"schema_version = 1\nsteam_app_id = 22380\n",
					"game_dir = \"{MANIFEST_GAME_DIR}\"\nobserved_build_id = 42\nunknown = true\n",
				),
				MANIFEST_GAME_DIR = MANIFEST_GAME_DIR,
			),
		];
		for contents in invalid {
			let (temp, root) = fixture()?;
			fs::write(temp.path().join("mods.toml"), contents)?;
			let before = fs::read(temp.path().join("mods.toml"))?;
			assert!(SettingsAdapter::with_environment(root, Vec::new()).load().is_err());
			assert_eq!(fs::read(temp.path().join("mods.toml"))?, before);
		}
		Ok(())
	}

	#[test]
	fn failed_manifest_stage_blocks_a_later_store_and_preserves_all_artifacts() -> Result<()> {
		let (temp, root) = fixture()?;
		let directory = fs_access::open_ambient_dir(root.as_path())?;
		let first =
			manifest_writer::replace_validated(&directory, "candidate", &CancellationToken::new(), |_| {
				false
			});
		assert!(first.is_err());

		let manifest_before = fs::read(temp.path().join("mods.toml"))?;
		let operations_before = fs::read_dir(temp.path().join("temp"))?.collect::<IoResult<Vec<_>>>()?;
		assert_eq!(operations_before.len(), 1);
		let operation_path = operations_before[0].path();
		let staged_path = operation_path.join("mods.toml");
		let staged_before = fs::read(&staged_path)?;
		let binding = GameBinding::new(
			GameInstallationPath::new(STORED_GAME_DIR.into())?,
			SteamBuildId::new(99)?,
		);

		let second = SettingsAdapter::with_environment(root, Vec::new())
			.store_game_binding(binding, CancellationToken::new());

		assert!(matches!(
			second,
			Err(report) if report.current_context().code() == ErrorCode::ManualCleanupRequired
		));
		assert_eq!(fs::read(temp.path().join("mods.toml"))?, manifest_before);
		assert_eq!(fs::read(&staged_path)?, staged_before);
		let operations_after = fs::read_dir(temp.path().join("temp"))?.collect::<IoResult<Vec<_>>>()?;
		assert_eq!(operations_after.len(), 1);
		assert_eq!(operations_after[0].file_name(), operations_before[0].file_name());
		Ok(())
	}

	#[test]
	fn cancelled_store_preserves_manifest_and_starts_no_operation() -> Result<()> {
		let (temp, root) = fixture()?;
		let before = fs::read(temp.path().join("mods.toml"))?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let binding = GameBinding::new(
			GameInstallationPath::new(STORED_GAME_DIR.into())?,
			SteamBuildId::new(99)?,
		);

		let result =
			SettingsAdapter::with_environment(root, Vec::new()).store_game_binding(binding, cancellation);

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::OperationCancelled
		));
		assert_eq!(fs::read(temp.path().join("mods.toml"))?, before);
		assert!(fs::read_dir(temp.path().join("temp"))?.next().is_none());
		Ok(())
	}

	#[test]
	fn preview_resolves_shadowing_without_mutating_the_manifest() -> Result<()> {
		let (temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![(OsString::from("MODS_GAME_DIR"), OsString::from(OVERRIDE_GAME_DIR))],
		);
		let stored = GameBinding::new(
			GameInstallationPath::new(STORED_GAME_DIR.into())?,
			SteamBuildId::new(99)?,
		);
		let before = fs::read(temp.path().join("mods.toml"))?;

		let outcome = adapter.preview_game_binding(stored.clone())?;

		assert_eq!(outcome.stored, stored);
		assert_eq!(
			outcome.effective.game_directory().as_path(),
			Path::new(OVERRIDE_GAME_DIR)
		);
		assert_eq!(outcome.effective.observed_build_id(), SteamBuildId::new(99)?);
		assert!(outcome.shadowed);
		assert_eq!(fs::read(temp.path().join("mods.toml"))?, before);
		assert!(fs::read_dir(temp.path().join("temp"))?.next().is_none());
		Ok(())
	}

	#[test]
	fn store_replaces_path_and_observed_build_together_under_shadowing() -> Result<()> {
		let (temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![(OsString::from("MODS_GAME_DIR"), OsString::from(OVERRIDE_GAME_DIR))],
		);
		let stored = GameBinding::new(
			GameInstallationPath::new(STORED_GAME_DIR.into())?,
			SteamBuildId::new(99)?,
		);
		let outcome = adapter.store_game_binding(stored, CancellationToken::new())?;
		assert!(outcome.shadowed);
		assert_eq!(
			outcome.effective.game_directory().as_path(),
			Path::new(OVERRIDE_GAME_DIR)
		);
		let text = fs::read_to_string(temp.path().join("mods.toml"))?;
		assert!(text.contains(&format!("game_dir = \"{STORED_GAME_DIR}\"")));
		assert!(text.contains("observed_build_id = 99"));
		Ok(())
	}

	#[test]
	fn missing_or_partial_root_is_reported_as_folder_uninitialized() -> Result<()> {
		let temp = TempDir::new()?;
		let base = fs::canonicalize(temp.path())?;
		let missing = EnvironmentRoot::new(base.join("missing"))?;
		for root in [missing, {
			let partial_path = base.join("partial");
			fs::create_dir(&partial_path)?;
			fs::create_dir(partial_path.join("temp"))?;
			EnvironmentRoot::new(partial_path)?
		}] {
			let error = SettingsAdapter::with_environment(root, Vec::new())
				.load()
				.err()
				.ok_or_else(|| IoError::other("partial root unexpectedly loaded"))?;
			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentNotInitialized);
			assert_eq!(error.current_context().message(), "folder uninitialized");
		}
		Ok(())
	}

	#[test]
	fn established_root_with_missing_canonical_directory_is_invalid() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::remove_dir_all(temp.path().join("profile"))?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.err()
			.ok_or_else(|| IoError::other("malformed root unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		Ok(())
	}

	#[test]
	fn canonical_esl_plugin_state_is_accepted() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("profile/plugins.txt"), "Example.EsL\r\n")?;
		fs::write(temp.path().join("profile/loadorder.txt"), "Example.eSL\r\n")?;

		SettingsAdapter::with_environment(root, Vec::new()).load()?;
		Ok(())
	}

	#[test]
	fn installed_mods_overwrite_saves_and_rebuildable_cache_are_accepted() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::create_dir_all(temp.path().join("mods/Example Mod/meshes"))?;
		fs::write(
			temp.path().join("mods/Example Mod/meta.toml"),
			"schema_version = 1\nsource = \"fixture\"\n",
		)?;
		fs::write(temp.path().join("mods/Example Mod/meshes/example.nif"), b"mesh")?;
		fs::write(
			temp.path().join("profile/modlist.txt"),
			"\u{feff}# manually ordered\r\n\r\n+Example Mod\r\n",
		)?;
		fs::create_dir_all(temp.path().join("overwrite/textures"))?;
		fs::write(temp.path().join("overwrite/textures/generated.dds"), b"texture")?;
		fs::write(temp.path().join("profile/saves/slot.fos"), b"save")?;
		fs::remove_dir_all(temp.path().join("cache"))?;

		SettingsAdapter::with_environment(root.clone(), Vec::new()).load()?;

		fs::create_dir(temp.path().join("cache"))?;
		SettingsAdapter::with_environment(root, Vec::new()).load()?;
		Ok(())
	}

	#[test]
	fn installed_mod_and_modlist_names_use_simple_unicode_case_folded_keys() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::create_dir(temp.path().join("mods/ÉΣ Mod"))?;
		fs::write(temp.path().join("mods/ÉΣ Mod/meta.toml"), "schema_version = 1\n")?;
		fs::write(temp.path().join("profile/modlist.txt"), "+éς mod\r\n")?;

		SettingsAdapter::with_environment(root, Vec::new()).load()?;
		Ok(())
	}

	#[test]
	fn installed_mod_and_modlist_must_describe_the_same_names() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::create_dir(temp.path().join("mods/Unlisted"))?;
		fs::write(temp.path().join("mods/Unlisted/meta.toml"), "schema_version = 1\n")?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.err()
			.ok_or_else(|| IoError::other("unlisted mod unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn nested_provider_symlink_is_rejected_as_unsafe() -> Result<()> {
		let (temp, root) = fixture()?;
		let outside = TempDir::new()?;
		let target = outside.path().join("outside.dds");
		fs::write(&target, b"outside")?;
		symlink_file(&target, &temp.path().join("overwrite/linked.dds"))?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.err()
			.ok_or_else(|| IoError::other("nested symlink unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentRootUnsafe);
		Ok(())
	}

	#[test]
	fn fallout_prefs_archive_values_are_accepted() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(
			temp.path().join("profile/FalloutPrefs.ini"),
			"[Archive]\nsArchiveList=Prefs.bsa\n",
		)?;

		SettingsAdapter::with_environment(root, Vec::new()).load()?;
		Ok(())
	}

	#[test]
	fn misplaced_managed_archive_keys_are_invalid() -> Result<()> {
		for (name, contents) in [
			(
				"Fallout.ini",
				concat!(
					"[Display]\n",
					"bInvalidateOlderFiles=0\n",
					"SInvalidationFile=misplaced\n",
					"sArchiveList=Misplaced.bsa\n",
				),
			),
			(
				"FalloutCustom.ini",
				concat!(
					"[General]\n",
					"bInvalidateOlderFiles=0\n",
					"SInvalidationFile=misplaced\n",
					"sArchiveList=Misplaced.bsa\n",
				),
			),
		] {
			let (temp, root) = fixture()?;
			let path = temp.path().join("profile").join(name);
			if name == "Fallout.ini" {
				fs::write(&path, format!("{}{}", fs::read_to_string(&path)?, contents))?;
			} else {
				fs::write(path, contents)?;
			}

			assert!(SettingsAdapter::with_environment(root, Vec::new()).load().is_err());
		}
		Ok(())
	}

	#[test]
	fn misplaced_routing_keys_are_invalid() -> Result<()> {
		for (name, contents) in [
			("Fallout.ini", "[Display]\nbUseMyGamesDirectory=0\n"),
			("FalloutPrefs.ini", "[Display]\nSLocalSavePath=elsewhere\n"),
			("FalloutCustom.ini", "[Display]\nbUseMyGamesDirectory=0\n"),
		] {
			let (temp, root) = fixture()?;
			let path = temp.path().join("profile").join(name);
			if name == "Fallout.ini" {
				fs::write(&path, format!("{}{}", fs::read_to_string(&path)?, contents))?;
			} else {
				fs::write(path, contents)?;
			}

			assert!(SettingsAdapter::with_environment(root, Vec::new()).load().is_err());
		}
		Ok(())
	}

	#[test]
	fn routing_keys_outside_canonical_fallout_ini_are_invalid() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(
			temp.path().join("profile/Fallout.ini"),
			concat!(
				"[General]\n",
				"SLocalSavePath=__mods_saves\\\n",
				"bUseMyGamesDirectory=0\n",
				"busemygamesdirectory=1\n",
				"[Archive]\n",
				"bInvalidateOlderFiles=1\n",
				"SInvalidationFile=\n",
				"sArchiveList=Fallout - Invalidation.bsa\n",
			),
		)?;
		for name in ["FalloutPrefs.ini", "FalloutCustom.ini"] {
			fs::write(
				temp.path().join("profile").join(name),
				"[General]\nbUseMyGamesDirectory=0\nbusemygamesdirectory=1\n",
			)?;
		}

		let error = SettingsAdapter::with_environment(root, Vec::new()).load();
		assert!(matches!(
			error,
			Err(report) if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		Ok(())
	}

	#[test]
	fn established_root_with_malformed_profile_is_invalid() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("profile/Fallout.ini"), b"[Archive]\n")?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.err()
			.ok_or_else(|| IoError::other("malformed profile unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		Ok(())
	}

	#[test]
	fn disposable_cache_and_logs_never_change_settings_behavior() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("cache/Fallout - Invalidation.bsa"), b"bad")?;
		fs::write(temp.path().join("cache/unknown.bin"), b"unknown")?;
		fs::create_dir(temp.path().join("logs"))?;
		fs::write(temp.path().join("logs/unknown.jsonl"), b"not a session")?;

		SettingsAdapter::with_environment(root, Vec::new()).load()?;
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn unsafe_disposable_entries_never_change_settings_behavior() -> Result<()> {
		let (temp, root) = fixture()?;
		let outside = TempDir::new()?;
		let target = outside.path().join("outside");
		fs::write(&target, b"outside")?;
		fs::create_dir(temp.path().join("logs"))?;
		symlink_file(&target, &temp.path().join("cache/linked"))?;
		symlink_file(&target, &temp.path().join("logs/linked"))?;

		SettingsAdapter::with_environment(root, Vec::new()).load()?;
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn manifest_symlink_is_rejected_without_reading_its_target() -> Result<()> {
		let (temp, root) = fixture()?;
		let target = temp.path().join("outside.toml");
		fs::rename(temp.path().join("mods.toml"), &target)?;
		symlink_file(&target, &temp.path().join("mods.toml"))?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.err()
			.ok_or_else(|| IoError::other("manifest symlink unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentRootUnsafe);
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn symlinked_root_ancestor_is_rejected() -> Result<()> {
		let (target, _) = fixture()?;
		let holder = TempDir::new()?;
		let linked_parent = holder.path().join("linked-parent");
		let target_parent = target
			.path()
			.parent()
			.ok_or_else(|| IoError::other("missing target parent"))?;
		symlink_dir(target_parent, &linked_parent)?;
		let selected = linked_parent.join(target
			.path()
			.file_name()
			.ok_or_else(|| IoError::other("missing target name"))?);
		let root = EnvironmentRoot::new(selected)?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.err()
			.ok_or_else(|| IoError::other("symlinked ancestor unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentRootUnsafe);
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn root_directory_symlink_is_rejected() -> Result<()> {
		let (target, _) = fixture()?;
		let holder = TempDir::new()?;
		let linked = holder.path().join("linked-root");
		symlink_dir(target.path(), &linked)?;
		let root = EnvironmentRoot::new(linked)?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.err()
			.ok_or_else(|| IoError::other("root symlink unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentRootUnsafe);
		Ok(())
	}

	#[cfg(unix)]
	fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
		symlink(source, destination)
	}

	#[cfg(windows)]
	fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
		windows_symlink_file(source, destination)
	}

	#[cfg(unix)]
	fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
		symlink(source, destination)
	}

	#[cfg(windows)]
	fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
		windows_symlink_dir(source, destination)
	}
}
