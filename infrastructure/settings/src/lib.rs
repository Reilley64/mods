#![cfg_attr(test, feature(fn_traits))]

mod config_source;
mod fs_access;
mod layout;
mod manifest_writer;

use application::ErrorMarker;
use application::ports::LoadSettings;
use application::ports::PortFuture;
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
use serde::Deserialize;
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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

	fn load(&self) -> Result<ResolvedSettings, ErrorMarker> {
		if has_pending_recovery_before_layout(&self.root)? {
			return Err(report!(ErrorMarker::environment_invalid(Some("recovery"))));
		}
		let (root, text) = open_bound_root(&self.root)?;
		if has_pending_recovery(&root)? {
			return Err(report!(ErrorMarker::environment_invalid(Some("recovery"))));
		}
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

	fn store_game_binding(
		&self,
		binding: GameBinding,
		cancellation: CancellationToken,
	) -> Result<StoredAndEffectiveBinding, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let (root, text) = open_bound_root(&self.root)?;
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
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let game_record = resolved
			.settings
			.iter()
			.find(|record| record.key == SettingKey::GameDir)
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		let source = game_record.source.clone();
		let shadowed = game_record.shadowed;
		let effective = resolved.effective_binding;
		manifest_writer::replace_validated(&root, &replacement, &cancellation, |candidate| {
			toml::from_str::<RawManifest>(candidate)
				.ok()
				.and_then(|raw| validate_manifest(&raw).ok())
				.is_some()
		})?;
		Ok(StoredAndEffectiveBinding {
			stored: binding,
			effective,
			source,
			shadowed,
		})
	}

	pub fn load_port(&self) -> LoadSettings {
		let adapter = self.clone();
		Arc::new(move || {
			let result = adapter.load();
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

#[derive(Serialize)]
struct WritableManifest<'a> {
	schema_version: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<&'a str>,
	steam_app_id: u32,
	game_dir: &'a str,
	observed_build_id: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableOperationRecord {
	schema_version: u32,
	operation: DurableOperationKind,
	operation_id: String,
	phase: DurableOperationPhase,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DurableOperationKind {
	Initialize,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DurableOperationPhase {
	Publishing,
}

fn open_bound_root(root: &EnvironmentRoot) -> Result<(Dir, String), ErrorMarker> {
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
	layout::validate(&directory)?;
	Ok((directory, text))
}

fn has_pending_recovery_before_layout(root: &EnvironmentRoot) -> Result<bool, ErrorMarker> {
	let directory = match fs_access::open_ambient_dir(root.as_path()) {
		Ok(directory) => directory,
		Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(false),
		Err(error) => return Err(error.context(ErrorMarker::environment_root_unsafe())),
	};
	let temp_metadata = match directory.symlink_metadata("temp") {
		Ok(metadata) => metadata,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
		Err(error) => return Err(report!(error).context(ErrorMarker::environment_root_unsafe())),
	};
	if fs_access::is_reparse(&temp_metadata) {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}
	if !temp_metadata.is_dir() {
		return Ok(false);
	}
	let temp = fs_access::open_dir(&directory, Path::new("temp")).context(ErrorMarker::environment_root_unsafe())?;
	has_pending_recovery_in_temp(&temp)
}

fn has_pending_recovery(root: &Dir) -> Result<bool, ErrorMarker> {
	let temp = fs_access::open_dir(root, Path::new("temp"))
		.context(ErrorMarker::environment_invalid(Some("recovery")))?;
	has_pending_recovery_in_temp(&temp)
}

fn has_pending_recovery_in_temp(temp: &Dir) -> Result<bool, ErrorMarker> {
	let entries = temp
		.entries()
		.context(ErrorMarker::environment_invalid(Some("recovery")))?;
	for entry in entries {
		let entry = entry.context(ErrorMarker::environment_invalid(Some("recovery")))?;
		let name = entry.file_name();
		let name_text = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(Some("recovery"))))?;
		let operation_id = uuid::Uuid::parse_str(name_text)
			.map_err(|error| report!(error).context(ErrorMarker::environment_invalid(Some("recovery"))))?;
		let metadata = temp
			.symlink_metadata(&name)
			.context(ErrorMarker::environment_invalid(Some("recovery")))?;
		if !metadata.is_dir() || fs_access::is_reparse(&metadata) {
			return Err(report!(ErrorMarker::environment_invalid(Some("recovery"))));
		}
		let operation_dir = fs_access::open_dir(temp, Path::new(&name))
			.context(ErrorMarker::environment_invalid(Some("recovery")))?;
		match operation_dir.symlink_metadata("operation.toml") {
			Ok(metadata) => {
				if !metadata.is_file() || fs_access::is_reparse(&metadata) {
					return Err(report!(ErrorMarker::environment_invalid(Some("recovery"))));
				}
				let mut marker = fs_access::open_regular(&operation_dir, Path::new("operation.toml"))
					.context(ErrorMarker::environment_invalid(Some("recovery")))?;
				let mut contents = String::new();
				marker.read_to_string(&mut contents)
					.context(ErrorMarker::environment_invalid(Some("recovery")))?;
				let record = toml::from_str::<DurableOperationRecord>(&contents)
					.context(ErrorMarker::environment_invalid(Some("recovery")))?;
				if record.schema_version != 1
					|| record.operation != DurableOperationKind::Initialize
					|| uuid::Uuid::parse_str(&record.operation_id).ok() != Some(operation_id)
					|| record.phase != DurableOperationPhase::Publishing
				{
					return Err(report!(ErrorMarker::environment_invalid(Some("recovery"))));
				}
				return Ok(true);
			}
			Err(error) if error.kind() == ErrorKind::NotFound => {}
			Err(error) => {
				return Err(report!(error).context(ErrorMarker::environment_invalid(Some("recovery"))));
			}
		}
	}
	Ok(false)
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
	use super::*;
	use application::ErrorCode;
	use std::fs;
	use std::io::Error as IoError;
	use std::io::Result as IoResult;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	#[cfg(windows)]
	use std::os::windows::fs::symlink_dir as windows_symlink_dir;
	#[cfg(windows)]
	use std::os::windows::fs::symlink_file as windows_symlink_file;
	use std::task::Context;
	use std::task::Poll;
	use std::task::Waker;
	use tempfile::TempDir;

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
			concat!(
				"schema_version = 1\nname = \"Mojave\"\nsteam_app_id = 22380\n",
				"game_dir = \"/games/fnv\"\nobserved_build_id = 42\n",
			),
		)?;
		Ok((temp, root))
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
			vec![(OsString::from("mods_game_dir"), OsString::from("/portable/fnv"))],
		);
		let resolved = adapter.load()?;
		let record = &resolved.settings[3];
		assert_eq!(record.value, SettingValue::Path("/portable/fnv".into()));
		assert_eq!(record.manifest_value, SettingValue::Path("/games/fnv".into()));
		assert!(record.shadowed);
		Ok(())
	}

	#[test]
	fn load_port_blocks_pre_manifest_initialization_with_a_durable_operation_marker() -> Result<()> {
		let temp = TempDir::new()?;
		let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?)?;
		let operation_id = uuid::Uuid::new_v4();
		let operation = temp.path().join("temp").join(operation_id.to_string());
		fs::create_dir_all(&operation)?;
		let marker = operation.join("operation.toml");
		fs::write(
			&marker,
			format!(
				concat!(
					"schema_version = 1\n",
					"operation = \"initialize\"\n",
					"operation_id = \"{operation_id}\"\n",
					"phase = \"publishing\"\n",
				),
				operation_id = operation_id,
			),
		)?;
		let before = fs::read(&marker)?;

		let load = SettingsAdapter::with_environment(root, Vec::new()).load_port();
		let mut future = load.call(());
		let context = &mut Context::from_waker(Waker::noop());
		let result = match future.as_mut().poll(context) {
			Poll::Ready(result) => result,
			Poll::Pending => return Err(report!(ErrorMarker::environment_invalid(None)).into()),
		};

		assert!(matches!(
			result,
			Err(report)
				if report.current_context().code() == ErrorCode::EnvironmentInvalid
					&& report.current_context().phase() == Some("recovery")
		));
		assert_eq!(fs::read(marker)?, before);
		assert!(!temp.path().join("mods.toml").exists());
		Ok(())
	}

	#[test]
	fn pending_recovery_blocks_read_without_mutating() -> Result<()> {
		let (temp, root) = fixture()?;
		let operation = temp.path().join("temp").join(uuid::Uuid::new_v4().to_string());
		fs::create_dir(&operation)?;
		fs::write(operation.join("operation.toml"), "schema_version = 1")?;
		let before = fs::read(temp.path().join("mods.toml"))?;
		let error = SettingsAdapter::with_environment(root, Vec::new()).load();
		assert!(matches!(
			error,
			Err(report)
				if report.current_context().code() == ErrorCode::EnvironmentInvalid
					&& report.current_context().phase() == Some("recovery")
		));
		assert_eq!(fs::read(temp.path().join("mods.toml")).ok(), Some(before));
		Ok(())
	}

	#[test]
	fn invalid_flat_manifests_fail_unchanged() -> Result<()> {
		let invalid = [
			"steam_app_id = 22380\ngame_dir = \"/games/fnv\"\nobserved_build_id = 42\n",
			"schema_version = 2\nsteam_app_id = 22380\ngame_dir = \"/games/fnv\"\nobserved_build_id = 42\n",
			"schema_version = 1\nsteam_app_id = 1\ngame_dir = \"/games/fnv\"\nobserved_build_id = 42\n",
			"schema_version = 1\nsteam_app_id = 22380\ngame_dir = \"/games/fnv\"\nobserved_build_id = 0\n",
			"schema_version = 1\nsteam_app_id = 22380\ngame_dir = \"relative\"\nobserved_build_id = 42\n",
			concat!(
				"schema_version = 1\nsteam_app_id = 22380\n",
				"game_dir = \"/games/fnv\"\nobserved_build_id = 42\nunknown = true\n",
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
	fn cancelled_store_preserves_manifest_and_starts_no_operation() -> Result<()> {
		let (temp, root) = fixture()?;
		let before = fs::read(temp.path().join("mods.toml"))?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let binding = GameBinding::new(GameInstallationPath::new("/other/fnv".into())?, SteamBuildId::new(99)?);

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
	fn store_replaces_path_and_observed_build_together_under_shadowing() -> Result<()> {
		let (temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![(OsString::from("MODS_GAME_DIR"), OsString::from("/portable/fnv"))],
		);
		let stored = GameBinding::new(GameInstallationPath::new("/other/fnv".into())?, SteamBuildId::new(99)?);
		let outcome = adapter.store_game_binding(stored, CancellationToken::new())?;
		assert!(outcome.shadowed);
		assert_eq!(outcome.effective.game_directory().as_path(), Path::new("/portable/fnv"));
		let text = fs::read_to_string(temp.path().join("mods.toml"))?;
		assert!(text.contains("game_dir = \"/other/fnv\""));
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
