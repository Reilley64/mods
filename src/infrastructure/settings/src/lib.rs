mod config_source;
mod layout;
mod manifest_writer;

use application::ErrorMarker;
use application::ports::CheckSettingsReadiness;
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
use config_source::RawManifest;
use domain::EnvironmentName;
use domain::EnvironmentRoot;
use domain::EnvironmentSchemaVersion;
use domain::GameBinding;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
#[cfg(test)]
use std::sync::atomic::Ordering;
use tokio::fs::metadata;
use tokio::fs::read_dir;
use tokio::fs::read_to_string;
use tokio_util::sync::CancellationToken;
use toml::from_str;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsLoadMode {
	Inspection,
	ReadOnly,
	Mutation,
	Execution,
}

#[derive(Debug, Clone)]
pub struct LoadedSettings {
	pub resolved: ResolvedSettings,
	manifest: RawManifest,
}

#[derive(Clone)]
pub struct SettingsAdapter {
	root: EnvironmentRoot,
	environment: Arc<Vec<(OsString, OsString)>>,
	#[cfg(test)]
	source_loads: Arc<AtomicUsize>,
}

impl SettingsAdapter {
	pub fn new(root: EnvironmentRoot) -> Self {
		Self {
			root,
			environment: Arc::new(env::vars_os().collect()),
			#[cfg(test)]
			source_loads: Arc::new(AtomicUsize::new(0)),
		}
	}

	#[cfg(test)]
	fn with_environment(root: EnvironmentRoot, environment: Vec<(OsString, OsString)>) -> Self {
		Self {
			root,
			environment: Arc::new(environment),
			source_loads: Arc::new(AtomicUsize::new(0)),
		}
	}

	async fn check_readiness(&self, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		refuse_unfinished_operation_before_layout(&self.root, SettingsAccess::Mutation).await?;

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		Ok(())
	}

	/// Loads one command snapshot. Downstream operations receive its typed values.
	///
	/// # Errors
	/// Refuses unsafe roots, pending work, invalid settings, and cancellation.
	pub async fn load_command(
		&self,
		mode: SettingsLoadMode,
		cancellation: &CancellationToken,
	) -> Result<LoadedSettings, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let access = if matches!(mode, SettingsLoadMode::ReadOnly | SettingsLoadMode::Inspection) {
			SettingsAccess::ReadOnly
		} else {
			SettingsAccess::Mutation
		};
		refuse_unfinished_operation_before_layout(&self.root, access).await?;
		let source = open_manifest_root(&self.root).await?;
		if matches!(mode, SettingsLoadMode::ReadOnly | SettingsLoadMode::Mutation) {
			layout::validate(self.root.as_path()).await?;
		}
		refuse_unfinished_operation(self.root.as_path(), access).await?;

		#[cfg(test)]
		self.source_loads.fetch_add(1, Ordering::Relaxed);
		let (manifest, effective, shadowed) = config_source::read_sources(
			&source,
			&self.environment,
			ErrorMarker::settings_environment_invalid(),
		)?;
		let resolved = resolved_settings(manifest.clone(), effective, shadowed)?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		Ok(LoadedSettings { resolved, manifest })
	}

	#[cfg(test)]
	async fn load(&self) -> Result<ResolvedSettings, ErrorMarker> {
		self.load_command(SettingsLoadMode::ReadOnly, &CancellationToken::new())
			.await
			.map(|loaded| loaded.resolved)
	}

	fn initialization_override(&self) -> Result<Option<GameInstallationPath>, ErrorMarker> {
		let Some(value) = config_source::initialization_override(&self.environment)? else {
			return Ok(None);
		};
		let path = GameInstallationPath::new(value.into()).context(ErrorMarker::game_install_invalid())?;
		Ok(Some(path))
	}

	fn preview_game_binding(
		&self,
		loaded: &LoadedSettings,
		binding: GameBinding,
	) -> Result<StoredAndEffectiveBinding, ErrorMarker> {
		Ok(self.prepare_game_binding(loaded, binding)?.outcome)
	}

	fn prepare_game_binding(
		&self,
		loaded: &LoadedSettings,
		binding: GameBinding,
	) -> Result<PreparedGameBinding, ErrorMarker> {
		let manifest = &loaded.manifest;
		let game_dir = binding
			.game_directory()
			.as_path()
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::setting_value_invalid()))?;
		let replacement = toml::to_string_pretty(&WritableManifest {
			schema_version: manifest.schema_version,
			name: manifest.name.as_deref(),
			game_dir,
		})
		.context(ErrorMarker::setting_value_invalid())?;

		let shadowed = loaded
			.resolved
			.settings
			.iter()
			.any(|record| record.key == SettingKey::GameDir && record.shadowed);
		let mut replacement_manifest = manifest.clone();
		replacement_manifest.game_dir = game_dir.to_owned();
		let mut replacement_effective = replacement_manifest.clone();
		if shadowed {
			replacement_effective.game_dir = loaded
				.resolved
				.effective_binding
				.game_directory()
				.as_path()
				.to_str()
				.ok_or_else(|| report!(ErrorMarker::setting_value_invalid()))?
				.to_owned();
		}
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

		Ok(PreparedGameBinding { replacement, outcome })
	}

	/// Writes the manifest directly. A concurrent manifest edit may be overwritten.
	async fn store_game_binding(
		&self,
		loaded: &LoadedSettings,
		binding: GameBinding,
		cancellation: CancellationToken,
	) -> Result<StoredAndEffectiveBinding, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		refuse_unfinished_operation_before_layout(&self.root, SettingsAccess::Mutation).await?;
		open_manifest_root(&self.root).await?;
		layout::validate(self.root.as_path()).await?;
		let prepared = self.prepare_game_binding(loaded, binding)?;

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		manifest_writer::replace_validated(
			self.root.as_path(),
			&prepared.replacement,
			&cancellation,
			|candidate| {
				from_str::<RawManifest>(candidate)
					.ok()
					.and_then(|raw| validate_manifest(&raw).ok())
					.is_some()
			},
		)
		.await?;
		Ok(prepared.outcome)
	}

	pub fn readiness_port(&self) -> CheckSettingsReadiness {
		let adapter = self.clone();
		Arc::new(move |cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.check_readiness(&cancellation).await }) as PortFuture<_>
		})
	}

	pub fn preview_port(&self, loaded: LoadedSettings) -> PreviewGameBinding {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let result = (|| {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				let outcome = adapter.preview_game_binding(&loaded, binding)?;

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

	pub fn store_port(&self, loaded: LoadedSettings) -> StoreGameBinding {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let adapter = adapter.clone();
			let loaded = loaded.clone();
			Box::pin(async move { adapter.store_game_binding(&loaded, binding, cancellation).await })
				as PortFuture<_>
		})
	}
}

struct PreparedGameBinding {
	replacement: String,
	outcome: StoredAndEffectiveBinding,
}

#[derive(Serialize)]
struct WritableManifest<'a> {
	schema_version: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<&'a str>,
	game_dir: &'a str,
}

async fn open_manifest_root(root: &EnvironmentRoot) -> Result<String, ErrorMarker> {
	let root_metadata = metadata(root.as_path()).await.map_err(|error| {
		let marker = missing_or_unsafe(error.kind());
		report!(error).context(marker)
	})?;
	if !root_metadata.is_dir() {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}

	let manifest = root.as_path().join("mods.toml");
	let manifest_metadata = metadata(&manifest).await.map_err(|error| {
		let marker = missing_or_unsafe(error.kind());
		report!(error).context(marker)
	})?;
	if !manifest_metadata.is_file() {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}

	let text = read_to_string(&manifest)
		.await
		.context(ErrorMarker::environment_invalid(None))?;

	for name in ["mods", "profile", "overwrite", "temp"] {
		let directory = metadata(root.as_path().join(name)).await.map_err(|error| {
			let marker = if error.kind() == ErrorKind::NotFound {
				ErrorMarker::environment_invalid(None)
			} else {
				ErrorMarker::environment_root_unsafe()
			};
			report!(error).context(marker)
		})?;
		if !directory.is_dir() {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}

	Ok(text)
}

fn missing_or_unsafe(kind: ErrorKind) -> ErrorMarker {
	if kind == ErrorKind::NotFound {
		ErrorMarker::environment_not_initialized()
	} else {
		ErrorMarker::environment_root_unsafe()
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsAccess {
	ReadOnly,
	Mutation,
}

async fn refuse_unfinished_operation_before_layout(
	root: &EnvironmentRoot,
	access: SettingsAccess,
) -> Result<(), ErrorMarker> {
	let temp = root.as_path().join("temp");
	let temp_metadata = match metadata(&temp).await {
		Ok(metadata) => metadata,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
		Err(error) => return Err(report!(error).context(ErrorMarker::environment_root_unsafe())),
	};
	if !temp_metadata.is_dir() {
		return Ok(());
	}
	refuse_unfinished_operation_in_temp(&temp, access).await
}

async fn refuse_unfinished_operation(root: &Path, access: SettingsAccess) -> Result<(), ErrorMarker> {
	refuse_unfinished_operation_in_temp(&root.join("temp"), access).await
}

async fn refuse_unfinished_operation_in_temp(temp: &Path, access: SettingsAccess) -> Result<(), ErrorMarker> {
	let mut entries = read_dir(temp).await.context(ErrorMarker::environment_root_unsafe())?;
	if entries
		.next_entry()
		.await
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
	let path = GameInstallationPath::new(raw.game_dir.clone().into())
		.context(ErrorMarker::environment_invalid(None))?;
	let name =
		raw.name.clone()
			.map(EnvironmentName::new)
			.transpose()
			.context(ErrorMarker::environment_invalid(None))?;
	Ok((GameBinding::new(path), name))
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
		SettingValue::Path(manifest_binding.game_directory().as_path().to_path_buf()),
	];
	let effective_values = [
		SettingValue::UnsignedInteger(u64::from(effective.schema_version)),
		effective.name.map_or(SettingValue::Unset, SettingValue::String),
		SettingValue::Path(effective_binding.game_directory().as_path().to_path_buf()),
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
	use super::SettingsLoadMode;
	use application::ErrorCode;
	use application::settings::SettingKey;
	use application::settings::SettingSource;
	use application::settings::SettingValue;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ModName;
	use rootcause::Result;
	use std::ffi::OsString;
	use std::fs;
	use std::io::Error as IoError;
	use std::path::Path;
	use std::sync::atomic::Ordering;
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
				"[General]\nbUseMyGamesDirectory=1\nSLocalSavePath=Saves\\\n",
				"[Archive]\nbInvalidateOlderFiles=1\n",
				"SInvalidationFile=\n",
				"sArchiveList=Fallout - Invalidation.bsa\n",
			),
		)?;
		for file in ["plugins.txt", "modlist.txt"] {
			fs::write(temp.path().join("profile").join(file), b"")?;
		}
		fs::write(temp.path().join("cache/Fallout - Invalidation.bsa"), empty_bsa_bytes())?;
		fs::write(
			temp.path().join("mods.toml"),
			format!(
				concat!(
					"schema_version = 1\nname = \"Mojave\"\n",
					"game_dir = \"{MANIFEST_GAME_DIR}\"\n",
				),
				MANIFEST_GAME_DIR = MANIFEST_GAME_DIR,
			),
		)?;
		Ok((temp, root))
	}

	#[tokio::test]
	async fn execution_binding_does_not_require_plugin_lists_or_inspect_saves() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::remove_file(temp.path().join("profile/plugins.txt"))?;
		let adapter = SettingsAdapter::with_environment(root, Vec::new());
		let binding = adapter
			.load_command(SettingsLoadMode::Execution, &CancellationToken::new())
			.await?
			.resolved
			.effective_binding;
		assert_eq!(binding.game_directory().as_path(), Path::new(MANIFEST_GAME_DIR));
		assert!(!temp.path().join("profile/plugins.txt").exists());
		fs::create_dir(temp.path().join("temp/pending"))?;
		let result = adapter
			.load_command(SettingsLoadMode::Execution, &CancellationToken::new())
			.await;
		assert!(
			matches!(result, Err(error) if error.current_context().code() == ErrorCode::ManualCleanupRequired)
		);
		Ok(())
	}

	#[tokio::test]
	async fn load_returns_exact_registry_order_and_provenance() -> Result<()> {
		let (_temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(root, Vec::new());
		let resolved = adapter.load().await?;
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

	#[tokio::test]
	async fn environment_override_shadows_only_game_directory() -> Result<()> {
		let (_temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![(OsString::from("mods_game_dir"), OsString::from(OVERRIDE_GAME_DIR))],
		);
		let resolved = adapter.load().await?;
		let record = &resolved.settings[2];
		assert_eq!(record.value, SettingValue::Path(OVERRIDE_GAME_DIR.into()));
		assert_eq!(record.manifest_value, SettingValue::Path(MANIFEST_GAME_DIR.into()));
		assert!(record.shadowed);
		Ok(())
	}

	#[tokio::test]
	async fn command_load_refuses_unfinished_pre_manifest_work_without_mutating() -> Result<()> {
		let temp = TempDir::new()?;
		let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?)?;
		let operation = temp.path().join("temp/operation");
		fs::create_dir_all(&operation)?;
		let marker = operation.join("mods.toml");
		fs::write(&marker, "pending")?;
		let before = fs::read(&marker)?;

		let result = SettingsAdapter::with_environment(root, Vec::new())
			.load_command(SettingsLoadMode::ReadOnly, &CancellationToken::new())
			.await;

		assert!(matches!(
			result,
			Err(report)
				if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read(marker)?, before);
		assert!(!temp.path().join("mods.toml").exists());
		Ok(())
	}

	#[tokio::test]
	async fn unfinished_work_blocks_read_without_mutating() -> Result<()> {
		let (temp, root) = fixture()?;
		let operation = temp.path().join("temp/operation");
		fs::create_dir(&operation)?;
		fs::write(operation.join("operation.toml"), "schema_version = 1")?;
		let before = fs::read(temp.path().join("mods.toml"))?;
		let error = SettingsAdapter::with_environment(root, Vec::new()).load().await;
		assert!(matches!(
			error,
			Err(report)
				if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read(temp.path().join("mods.toml")).ok(), Some(before));
		Ok(())
	}

	#[tokio::test]
	async fn invalid_flat_manifests_fail_unchanged() -> Result<()> {
		let invalid = [
			format!("game_dir = \"{MANIFEST_GAME_DIR}\"\n"),
			format!("schema_version = 2\ngame_dir = \"{MANIFEST_GAME_DIR}\"\n"),
			format!("schema_version = 1\ngame_dir = \"{MANIFEST_GAME_DIR}\"\nsteam_app_id = 22380\n"),
			format!("schema_version = 1\ngame_dir = \"{MANIFEST_GAME_DIR}\"\nobserved_build_id = 42\n"),
			"schema_version = 1\ngame_dir = \"relative\"\n".to_owned(),
			format!("schema_version = 1\ngame_dir = \"{MANIFEST_GAME_DIR}\"\nunknown = true\n"),
		];
		for contents in invalid {
			let (temp, root) = fixture()?;
			fs::write(temp.path().join("mods.toml"), contents)?;
			let before = fs::read(temp.path().join("mods.toml"))?;
			assert!(SettingsAdapter::with_environment(root, Vec::new())
				.load()
				.await
				.is_err());
			assert_eq!(fs::read(temp.path().join("mods.toml"))?, before);
		}
		Ok(())
	}

	#[tokio::test]
	async fn cancelled_store_preserves_manifest_and_starts_no_operation() -> Result<()> {
		let (temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(root.clone(), Vec::new());
		let loaded = adapter
			.load_command(SettingsLoadMode::Mutation, &CancellationToken::new())
			.await?;
		let before = fs::read(temp.path().join("mods.toml"))?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let binding = GameBinding::new(GameInstallationPath::new(STORED_GAME_DIR.into())?);

		let result = SettingsAdapter::with_environment(root, Vec::new())
			.store_game_binding(&loaded, binding, cancellation)
			.await;

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::OperationCancelled
		));
		assert_eq!(fs::read(temp.path().join("mods.toml"))?, before);
		assert!(fs::read_dir(temp.path().join("temp"))?.next().is_none());
		Ok(())
	}

	#[tokio::test]
	async fn command_snapshot_is_loaded_once_and_reused_for_preview_and_store() -> Result<()> {
		let (_temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![("MODS_GAME_DIR".into(), OVERRIDE_GAME_DIR.into())],
		);
		let loaded = adapter
			.load_command(SettingsLoadMode::Mutation, &CancellationToken::new())
			.await?;
		let stored = GameBinding::new(GameInstallationPath::new(STORED_GAME_DIR.into())?);

		let preview = adapter.preview_game_binding(&loaded, stored.clone())?;
		let result = adapter
			.store_game_binding(&loaded, stored, CancellationToken::new())
			.await?;

		assert_eq!(adapter.source_loads.load(Ordering::Relaxed), 1);
		assert_eq!(preview.effective, result.effective);
		assert_eq!(
			result.effective.game_directory().as_path(),
			Path::new(OVERRIDE_GAME_DIR)
		);
		assert!(result.shadowed);
		Ok(())
	}

	#[tokio::test]
	async fn preview_resolves_shadowing_without_mutating_the_manifest() -> Result<()> {
		let (temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![(OsString::from("MODS_GAME_DIR"), OsString::from(OVERRIDE_GAME_DIR))],
		);
		let stored = GameBinding::new(GameInstallationPath::new(STORED_GAME_DIR.into())?);
		let before = fs::read(temp.path().join("mods.toml"))?;

		let loaded = adapter
			.load_command(SettingsLoadMode::Mutation, &CancellationToken::new())
			.await?;
		let outcome = adapter.preview_game_binding(&loaded, stored.clone())?;

		assert_eq!(outcome.stored, stored);
		assert_eq!(
			outcome.effective.game_directory().as_path(),
			Path::new(OVERRIDE_GAME_DIR)
		);
		assert!(outcome.shadowed);
		assert_eq!(fs::read(temp.path().join("mods.toml"))?, before);
		assert!(fs::read_dir(temp.path().join("temp"))?.next().is_none());
		Ok(())
	}

	#[tokio::test]
	async fn store_replaces_path_under_shadowing() -> Result<()> {
		let (temp, root) = fixture()?;
		let adapter = SettingsAdapter::with_environment(
			root,
			vec![(OsString::from("MODS_GAME_DIR"), OsString::from(OVERRIDE_GAME_DIR))],
		);
		let stored = GameBinding::new(GameInstallationPath::new(STORED_GAME_DIR.into())?);
		let loaded = adapter
			.load_command(SettingsLoadMode::Mutation, &CancellationToken::new())
			.await?;
		let outcome = adapter
			.store_game_binding(&loaded, stored, CancellationToken::new())
			.await?;
		assert!(outcome.shadowed);
		assert_eq!(
			outcome.effective.game_directory().as_path(),
			Path::new(OVERRIDE_GAME_DIR)
		);
		let text = fs::read_to_string(temp.path().join("mods.toml"))?;
		assert!(text.contains(&format!("game_dir = \"{STORED_GAME_DIR}\"")));
		assert!(!text.contains("observed_build_id"));
		assert!(!text.contains("steam_app_id"));
		Ok(())
	}

	#[tokio::test]
	async fn missing_or_partial_root_is_reported_as_folder_uninitialized() -> Result<()> {
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
				.await
				.err()
				.ok_or_else(|| IoError::other("partial root unexpectedly loaded"))?;
			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentNotInitialized);
			assert_eq!(error.current_context().message(), "folder uninitialized");
		}
		Ok(())
	}

	#[tokio::test]
	async fn established_root_with_missing_canonical_directory_is_invalid() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::remove_dir_all(temp.path().join("profile"))?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.await
			.err()
			.ok_or_else(|| IoError::other("malformed root unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		Ok(())
	}

	#[tokio::test]
	async fn canonical_esl_plugin_state_is_accepted() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("profile/plugins.txt"), "Example.EsL\r\n")?;

		SettingsAdapter::with_environment(root, Vec::new()).load().await?;
		Ok(())
	}

	#[tokio::test]
	async fn a_former_load_order_file_is_ignored() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("profile/loadorder.txt"), "not a plugin\n")?;

		SettingsAdapter::with_environment(root, Vec::new()).load().await?;
		Ok(())
	}

	#[tokio::test]
	async fn installed_mod_without_metadata_is_accepted() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::create_dir(temp.path().join("mods/Example Mod"))?;
		fs::write(temp.path().join("profile/modlist.txt"), "+Example Mod\n")?;

		SettingsAdapter::with_environment(root, Vec::new()).load().await?;
		Ok(())
	}

	#[tokio::test]
	async fn installed_mod_with_invalid_metadata_is_rejected() -> Result<()> {
		for metadata in ["not = [", "schema_version = 2\n"] {
			let (temp, root) = fixture()?;
			fs::create_dir(temp.path().join("mods/Example Mod"))?;
			fs::write(temp.path().join("mods/Example Mod/meta.toml"), metadata)?;
			fs::write(temp.path().join("profile/modlist.txt"), "+Example Mod\n")?;

			let error = SettingsAdapter::with_environment(root, Vec::new())
				.load()
				.await
				.err()
				.ok_or_else(|| IoError::other("present invalid metadata unexpectedly loaded"))?;
			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		}
		Ok(())
	}

	#[tokio::test]
	async fn installed_mods_overwrite_saves_and_rebuildable_cache_are_accepted() -> Result<()> {
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

		SettingsAdapter::with_environment(root.clone(), Vec::new())
			.load()
			.await?;

		fs::create_dir(temp.path().join("cache"))?;
		SettingsAdapter::with_environment(root, Vec::new()).load().await?;
		Ok(())
	}

	#[tokio::test]
	async fn a_listed_mod_needs_a_directory_of_exactly_the_listed_spelling() -> Result<()> {
		for (modlist, listed) in [("+éς mod\r\n", "éς mod"), ("+Missing\r\n", "Missing")] {
			let (temp, root) = fixture()?;
			fs::create_dir(temp.path().join("mods/ÉΣ Mod"))?;
			fs::write(temp.path().join("profile/modlist.txt"), modlist)?;

			let error = SettingsAdapter::with_environment(root, Vec::new())
				.load()
				.await
				.err()
				.ok_or_else(|| {
					IoError::other("listed mod without its directory unexpectedly loaded")
				})?;

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
			assert_eq!(error.current_context().mod_name().map(ModName::as_str), Some(listed));
		}
		Ok(())
	}

	#[tokio::test]
	async fn unlisted_mod_folders_and_stray_files_are_ignored() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::create_dir(temp.path().join("mods/Unlisted"))?;
		fs::write(temp.path().join("mods/Unlisted/meta.toml"), "not = [")?;
		fs::write(temp.path().join("mods/stray.txt"), b"stray")?;

		SettingsAdapter::with_environment(root, Vec::new()).load().await?;
		Ok(())
	}

	#[tokio::test]
	async fn fallout_prefs_archive_values_are_accepted() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(
			temp.path().join("profile/FalloutPrefs.ini"),
			"[Archive]\nsArchiveList=Prefs.bsa\n",
		)?;

		SettingsAdapter::with_environment(root, Vec::new()).load().await?;
		Ok(())
	}

	#[tokio::test]
	async fn user_authored_archive_keys_are_preserved() -> Result<()> {
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

			assert!(SettingsAdapter::with_environment(root, Vec::new()).load().await.is_ok());
		}
		Ok(())
	}

	#[tokio::test]
	async fn misplaced_routing_keys_are_invalid() -> Result<()> {
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

			assert!(SettingsAdapter::with_environment(root, Vec::new())
				.load()
				.await
				.is_err());
		}
		Ok(())
	}

	#[tokio::test]
	async fn routing_keys_outside_canonical_fallout_ini_are_invalid() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(
			temp.path().join("profile/Fallout.ini"),
			concat!(
				"[General]\n",
				"SLocalSavePath=Saves\\\n",
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

		let error = SettingsAdapter::with_environment(root, Vec::new()).load().await;
		assert!(matches!(
			error,
			Err(report) if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		Ok(())
	}

	#[tokio::test]
	async fn established_root_with_malformed_profile_is_invalid() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("profile/Fallout.ini"), b"[Archive]\n")?;
		let error = SettingsAdapter::with_environment(root, Vec::new())
			.load()
			.await
			.err()
			.ok_or_else(|| IoError::other("malformed profile unexpectedly loaded"))?;
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		Ok(())
	}

	#[tokio::test]
	async fn disposable_cache_and_logs_never_change_settings_behavior() -> Result<()> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("cache/Fallout - Invalidation.bsa"), b"bad")?;
		fs::write(temp.path().join("cache/unknown.bin"), b"unknown")?;
		fs::create_dir(temp.path().join("logs"))?;
		fs::write(temp.path().join("logs/unknown.jsonl"), b"not a session")?;

		SettingsAdapter::with_environment(root, Vec::new()).load().await?;
		Ok(())
	}
}
