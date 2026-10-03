use crate::hashing::sha256;
use application::ErrorMarker;
use application::conflicts::ConflictContentRead;
use application::conflicts::EnvironmentConflictScan;
use application::conflicts::IndexedConflictFile;
use application::conflicts::IndexedConflictFileId;
use application::conflicts::ScannedConflictProvider;
use domain::ConflictProblem;
use domain::ConflictProblemKind;
use domain::DataRelativePath;
use domain::GameBinding;
use domain::InstalledMod;
use domain::ModName;
use domain::ModPriority;
use domain::ParticipationReason;
use domain::ProblemScope;
use domain::ProviderClass;
use domain::ProviderIdentity;
use domain::ProviderReference;
use domain::Tombstone;
use domain::TombstoneScope;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::str::from_utf8;
use tokio::fs::File;
use tokio::fs::metadata;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio::fs::try_exists;
use tokio_util::sync::CancellationToken;
use toml::Value;
use toml::from_str;

const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];
#[cfg(windows)]
const WINDOWS_ERROR_SHARING_VIOLATION: i32 = 32;
#[cfg(windows)]
const WINDOWS_ERROR_LOCK_VIOLATION: i32 = 33;

pub(crate) async fn scan(
	root_path: &Path,
	binding: &GameBinding,
	cancellation: &CancellationToken,
) -> Result<EnvironmentConflictScan, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
	}

	if !try_exists(root_path.join("mods.toml"))
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?
	{
		return Err(report!(ErrorMarker::environment_not_initialized()));
	}

	let profile = required_directory(root_path, "profile").await?;
	let mods = required_directory(root_path, "mods").await?;
	let overwrite = required_directory(root_path, "overwrite").await?;
	required_directory(root_path, "cache").await?;
	let temp = required_directory(root_path, "temp").await?;
	if directory_has_entries(&temp, cancellation).await? {
		return Err(report!(ErrorMarker::environment_invalid(Some("pending_operation"))));
	}

	let modlist = read(profile.join("modlist.txt"))
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?;
	let (installed_mods, problems) = parse_modlist(&modlist);
	let mod_entries = mod_entry_names(&mods, cancellation).await?;

	let mut providers = Vec::new();
	let data = binding.game_directory().as_path().join("Data");
	match metadata(&data).await {
		Ok(metadata) if metadata.is_dir() => {
			providers.push(scan_provider(&data, ProviderIdentity::SteamData, true, cancellation).await?);
		}
		Ok(_) => return Err(report!(ErrorMarker::io_failure().with_phase("conflict_scan"))),
		Err(error) if error.kind() == io::ErrorKind::NotFound => {
			let mut provider = empty_provider(ProviderIdentity::SteamData, true);
			provider.problems.push(ConflictProblem {
				kind: ConflictProblemKind::ProviderMissing,
				scope: ProblemScope::Provider(ProviderIdentity::SteamData),
			});
			providers.push(provider);
		}
		Err(error) => return Err(report!(error).context(ErrorMarker::io_failure().with_phase("conflict_scan"))),
	}

	for installed in installed_mods {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
		}

		let identity = ProviderIdentity::DataMod {
			mod_name: installed.name.clone(),
			priority: installed.priority,
		};
		// A listed mod needs a directory of exactly the listed spelling; a case variant is missing.
		let directory = mods.join(installed.name.as_str());
		let directory_exists = mod_entries.contains(installed.name.as_str())
			&& metadata(&directory)
				.await
				.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?
				.is_dir();
		if !directory_exists {
			let mut provider = empty_provider(identity.clone(), installed.enabled);
			provider.problems.push(ConflictProblem {
				kind: ConflictProblemKind::ProviderMissing,
				scope: ProblemScope::Provider(identity),
			});
			providers.push(provider);
			continue;
		}

		providers.push(scan_provider(&directory, identity, installed.enabled, cancellation).await?);
	}

	providers.push(scan_provider(&overwrite, ProviderIdentity::Overwrite, true, cancellation).await?);

	Ok(EnvironmentConflictScan { providers, problems })
}

pub(crate) async fn read_content(
	root_path: &Path,
	binding: &GameBinding,
	id: &IndexedConflictFileId,
	cancellation: &CancellationToken,
) -> Result<ConflictContentRead, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
	}

	let provider = match id.identity() {
		ProviderIdentity::SteamData => binding.game_directory().as_path().join("Data"),
		ProviderIdentity::DataMod { mod_name, .. } => root_path.join("mods").join(mod_name.as_str()),
		ProviderIdentity::Overwrite => root_path.join("overwrite"),
	};
	let path = id
		.path()
		.components()
		.fold(provider, |path, component| path.join(component));
	let metadata = match metadata(&path).await {
		Ok(metadata) => metadata,
		Err(error) if content_access_is_unavailable(&error) => return Ok(ConflictContentRead::Unavailable),
		Err(error) => return Err(report!(error).context(ErrorMarker::io_failure().with_phase("conflict_scan"))),
	};
	if !metadata.is_file() {
		return Err(report!(ErrorMarker::io_failure().with_phase("conflict_scan")));
	}

	let file = match File::open(&path).await {
		Ok(file) => file,
		Err(error) if content_access_is_unavailable(&error) => return Ok(ConflictContentRead::Unavailable),
		Err(error) => return Err(report!(error).context(ErrorMarker::io_failure().with_phase("conflict_scan"))),
	};
	sha256(file, cancellation).await
}

fn content_access_is_unavailable(error: &io::Error) -> bool {
	if error.kind() == io::ErrorKind::PermissionDenied {
		return true;
	}

	#[cfg(windows)]
	{
		matches!(
			error.raw_os_error(),
			Some(WINDOWS_ERROR_SHARING_VIOLATION | WINDOWS_ERROR_LOCK_VIOLATION)
		)
	}
	#[cfg(not(windows))]
	{
		false
	}
}

fn empty_provider(identity: ProviderIdentity, enabled: bool) -> ScannedConflictProvider {
	ScannedConflictProvider {
		identity,
		enabled,
		files: Vec::new(),
		directories: Vec::new(),
		tombstones: Vec::new(),
		problems: Vec::new(),
	}
}

async fn required_directory(root: &Path, name: &str) -> Result<PathBuf, ErrorMarker> {
	let path = root.join(name);
	match metadata(&path).await {
		Ok(metadata) if metadata.is_dir() => Ok(path),
		Ok(_) => Err(report!(ErrorMarker::io_failure().with_phase("conflict_scan"))),
		Err(error) if error.kind() == io::ErrorKind::NotFound => {
			Err(report!(error).context(ErrorMarker::environment_not_initialized()))
		}
		Err(error) => Err(report!(error).context(ErrorMarker::io_failure().with_phase("conflict_scan"))),
	}
}

async fn directory_has_entries(directory: &Path, cancellation: &CancellationToken) -> Result<bool, ErrorMarker> {
	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?;
	let next = entries.next_entry().await;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
	}

	Ok(next.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?
		.is_some())
}

fn parse_modlist(bytes: &[u8]) -> (Vec<InstalledMod>, Vec<ConflictProblem>) {
	let mut problems = Vec::new();
	let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
	let Ok(text) = from_utf8(bytes) else {
		return (
			Vec::new(),
			vec![ConflictProblem {
				kind: ConflictProblemKind::ModlistInvalid,
				scope: ProblemScope::Modlist,
			}],
		);
	};
	let mut listed = Vec::new();
	let mut seen = HashSet::new();
	for raw_line in text.split('\n') {
		let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		let Some((marker, name)) = line.split_at_checked(1) else {
			problems.push(modlist_problem());
			continue;
		};
		let enabled = match marker {
			"+" => true,
			"-" => false,
			_ => {
				problems.push(modlist_problem());
				continue;
			}
		};
		if name.trim() != name {
			problems.push(modlist_problem());
			continue;
		}
		let Ok(name) = ModName::new(name.to_owned()) else {
			problems.push(modlist_problem());
			continue;
		};
		if !seen.insert(name.comparison_key().to_owned()) {
			problems.push(modlist_problem());
			continue;
		}
		listed.push((name, enabled));
	}

	// MO2 lists the highest Mod Priority first. Return mods from lowest to highest.
	let mut installed = Vec::with_capacity(listed.len());
	for (priority, (name, enabled)) in listed.into_iter().rev().enumerate() {
		let Ok(priority) = u32::try_from(priority) else {
			problems.push(modlist_problem());
			continue;
		};
		installed.push(InstalledMod {
			name,
			priority: ModPriority::new(priority),
			enabled,
		});
	}
	(installed, problems)
}

fn modlist_problem() -> ConflictProblem {
	ConflictProblem {
		kind: ConflictProblemKind::ModlistInvalid,
		scope: ProblemScope::Modlist,
	}
}

/// Lists the exact spellings of the `mods` entries.
///
/// A name that is not valid UTF-8 cannot appear in `modlist.txt`, so it is skipped.
async fn mod_entry_names(mods: &Path, cancellation: &CancellationToken) -> Result<HashSet<String>, ErrorMarker> {
	let mut entries = read_dir(mods)
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?;
	let mut names = HashSet::new();
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
		}

		if let Ok(name) = entry.file_name().into_string() {
			names.insert(name);
		}
	}
	Ok(names)
}

async fn scan_provider(
	directory: &Path,
	identity: ProviderIdentity,
	enabled: bool,
	cancellation: &CancellationToken,
) -> Result<ScannedConflictProvider, ErrorMarker> {
	let mut provider = empty_provider(identity.clone(), enabled);
	let mut problems = Vec::new();
	let mut keys = HashMap::new();
	scan_provider_directory(
		directory,
		&identity,
		enabled,
		"",
		&mut provider,
		&mut keys,
		&mut problems,
		cancellation,
	)
	.await?;

	let metadata_path = directory.join("meta.toml");
	if try_exists(&metadata_path)
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?
	{
		let bytes = read(&metadata_path)
			.await
			.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?;
		provider.tombstones = read_tombstones(&bytes, &identity, enabled, &mut problems);
	}
	validate_provider_tombstones(&provider, &keys, &mut problems);
	provider.problems = problems;
	Ok(provider)
}

#[expect(
	clippy::too_many_arguments,
	reason = "recursive enumeration keeps provider state explicit at the adapter boundary"
)]
async fn scan_provider_directory(
	directory: &Path,
	identity: &ProviderIdentity,
	enabled: bool,
	prefix: &str,
	provider: &mut ScannedConflictProvider,
	keys: &mut HashMap<String, bool>,
	problems: &mut Vec<ConflictProblem>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
	}

	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
		}

		let os_name = entry.file_name();
		let Some(spelling) = os_name.to_str() else {
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::NonLosslessName,
				scope: ProblemScope::Provider(identity.clone()),
			});
			continue;
		};
		let metadata_key = case_fold_key("meta.toml");
		if prefix.is_empty()
			&& case_fold_key(spelling) == metadata_key
			&& identity.class() != ProviderClass::SteamData
		{
			if spelling != "meta.toml" {
				let path = DataRelativePath::new(spelling.to_owned())
					.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?;
				problems.push(path_problem(ConflictProblemKind::ReservedPath, &path));
			}
			continue;
		}

		let relative = if prefix.is_empty() {
			spelling.to_owned()
		} else {
			format!("{prefix}/{spelling}")
		};
		let Ok(path) = DataRelativePath::new(relative.clone()) else {
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::NonLosslessName,
				scope: ProblemScope::Provider(identity.clone()),
			});
			continue;
		};
		if identity.class() == ProviderClass::SteamData
			&& prefix.is_empty() && path.comparison_key() == case_fold_key("meta.toml")
		{
			problems.push(path_problem(ConflictProblemKind::ReservedPath, &path));
			continue;
		}

		let metadata = metadata(entry.path())
			.await
			.context(ErrorMarker::io_failure().with_phase("conflict_scan"))?;

		let is_directory = metadata.is_dir();
		if keys.insert(path.comparison_key().to_owned(), is_directory).is_some() {
			problems.push(path_problem(ConflictProblemKind::InternalKeyCollision, &path));
			continue;
		}
		if is_directory {
			let reference = provider_reference(identity, path.clone(), enabled);
			provider.directories.push(reference);
			Box::pin(scan_provider_directory(
				&entry.path(),
				identity,
				enabled,
				&relative,
				provider,
				keys,
				problems,
				cancellation,
			))
			.await?;
			continue;
		}
		if !metadata.is_file() {
			problems.push(path_problem(ConflictProblemKind::UnsupportedEntryType, &path));
			continue;
		}

		let reference = provider_reference(identity, path.clone(), enabled);
		provider.files.push(IndexedConflictFile {
			id: IndexedConflictFileId::new(identity.clone(), path),
			provider: reference,
		});
	}
	Ok(())
}

fn provider_reference(identity: &ProviderIdentity, path: DataRelativePath, enabled: bool) -> ProviderReference {
	match identity {
		ProviderIdentity::SteamData => ProviderReference::SteamData { original_path: path },
		ProviderIdentity::DataMod { mod_name, priority } => ProviderReference::DataMod {
			mod_name: mod_name.clone(),
			priority: *priority,
			original_path: path,
			participation_reason: if enabled {
				ParticipationReason::EnabledMod
			} else {
				ParticipationReason::DisabledMod
			},
		},
		ProviderIdentity::Overwrite => ProviderReference::Overwrite { original_path: path },
	}
}

fn read_tombstones(
	bytes: &[u8],
	identity: &ProviderIdentity,
	enabled: bool,
	problems: &mut Vec<ConflictProblem>,
) -> Vec<Tombstone> {
	let Ok(text) = from_utf8(bytes) else {
		problems.push(ConflictProblem {
			kind: ConflictProblemKind::InvalidTombstoneMetadata,
			scope: ProblemScope::Provider(identity.clone()),
		});
		return Vec::new();
	};
	let Ok(value) = from_str::<Value>(text) else {
		problems.push(ConflictProblem {
			kind: ConflictProblemKind::InvalidTombstoneMetadata,
			scope: ProblemScope::Provider(identity.clone()),
		});
		return Vec::new();
	};
	let Some(table) = value.as_table() else {
		problems.push(metadata_problem(identity));
		return Vec::new();
	};
	if table.get("schema_version").and_then(Value::as_integer) != Some(1) {
		problems.push(metadata_problem(identity));
		return Vec::new();
	}
	let Some(tombstone_value) = table.get("tombstones") else {
		return Vec::new();
	};
	let Some(tombstone_table) = tombstone_value.as_table() else {
		problems.push(metadata_problem(identity));
		return Vec::new();
	};
	if tombstone_table
		.keys()
		.any(|key| !matches!(key.as_str(), "files" | "directories"))
	{
		problems.push(metadata_problem(identity));
		return Vec::new();
	}

	let mut result = Vec::new();
	let mut seen = HashSet::new();
	for (name, scope) in [
		("files", TombstoneScope::ExactFile),
		("directories", TombstoneScope::DirectorySubtree),
	] {
		let Some(values) = tombstone_table.get(name) else {
			continue;
		};
		let Some(values) = values.as_array() else {
			problems.push(metadata_problem(identity));
			continue;
		};
		for value in values {
			let Some(stored) = value.as_str() else {
				problems.push(metadata_problem(identity));
				continue;
			};
			let Ok(path) = DataRelativePath::new(stored.to_owned()) else {
				problems.push(ConflictProblem {
					kind: ConflictProblemKind::InvalidTombstonePath,
					scope: ProblemScope::Provider(identity.clone()),
				});
				continue;
			};
			let reserved_metadata =
				path.components().count() == 1 && path.comparison_key() == case_fold_key("meta.toml");
			if stored != path.as_str()
				|| reserved_metadata || !seen.insert(path.comparison_key().to_owned())
			{
				problems.push(path_problem(ConflictProblemKind::InvalidTombstonePath, &path));
				continue;
			}
			result.push(Tombstone {
				scope,
				owner: provider_reference(identity, path, enabled),
			});
		}
	}
	result
}

fn metadata_problem(identity: &ProviderIdentity) -> ConflictProblem {
	ConflictProblem {
		kind: ConflictProblemKind::InvalidTombstoneMetadata,
		scope: ProblemScope::Provider(identity.clone()),
	}
}

fn validate_provider_tombstones(
	provider: &ScannedConflictProvider,
	ordinary_keys: &HashMap<String, bool>,
	problems: &mut Vec<ConflictProblem>,
) {
	let directory_tombstones = provider
		.tombstones
		.iter()
		.filter(|tombstone| tombstone.scope == TombstoneScope::DirectorySubtree)
		.map(|tombstone| tombstone.path().comparison_key())
		.collect::<HashSet<_>>();
	for tombstone in &provider.tombstones {
		let path = tombstone.path();
		if ordinary_keys.contains_key(path.comparison_key()) {
			problems.push(path_problem(ConflictProblemKind::OrdinaryTombstoneCollision, path));
		}
		for (boundary, _) in path.comparison_key().match_indices('/') {
			if directory_tombstones.contains(&path.comparison_key()[..boundary]) {
				problems.push(path_problem(
					ConflictProblemKind::DirectoryTombstoneDescendantCollision,
					path,
				));
			}
		}
	}
	for key in ordinary_keys.keys() {
		for (boundary, _) in key.match_indices('/') {
			if directory_tombstones.contains(&key[..boundary])
				&& let Ok(path) = DataRelativePath::new(key.clone())
			{
				problems.push(path_problem(
					ConflictProblemKind::DirectoryTombstoneDescendantCollision,
					&path,
				));
			}
		}
	}
}

fn path_problem(kind: ConflictProblemKind, path: &DataRelativePath) -> ConflictProblem {
	ConflictProblem {
		kind,
		scope: ProblemScope::Path {
			normalized_key: path.comparison_key().to_owned(),
			display_path: path.clone(),
		},
	}
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "fixture construction and output assertions require known-success values"
)]
mod tests {
	#[cfg(windows)]
	use super::content_access_is_unavailable;
	use super::parse_modlist;
	use super::read_content;
	use super::scan;
	use crate::EnvironmentAdapter;
	use crate::profile::PROFILE_FILES;
	use application::ErrorCode;
	use application::conflicts::ConflictContentRead;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileSource;
	use domain::ConflictProblemKind;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ParticipationReason;
	use domain::ProviderIdentity;
	use domain::TombstoneScope;
	use std::env::current_dir;
	use std::error::Error;
	use std::fs;
	#[cfg(windows)]
	use std::io::Error as IoError;
	use std::io::ErrorKind;
	use std::path::Path;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	async fn fixture() -> Result<(TempDir, EnvironmentRoot, GameBinding), Box<dyn Error>> {
		let temp = TempDir::new_in(current_dir()?)?;
		let root = EnvironmentRoot::new(temp.path().join("environment"))
			.map_err(|_| "invalid environment root")?;
		let game = temp.path().join("game");
		fs::create_dir_all(game.join("Data"))?;
		let plan = InitializationPlan {
			game_binding: GameBinding::new(
				GameInstallationPath::new(game).map_err(|_| "invalid game path")?,
			),
			profile_sources: InitializationProfileSources {
				files: PROFILE_FILES
					.into_iter()
					.map(|name| ProfileSource { name, contents: None })
					.collect(),
				fallout_default_ini: b"[Archive]\r\nsArchiveList=Fallout - Meshes.bsa\r\n".to_vec(),
			},
		};
		let binding = plan.game_binding.clone();
		EnvironmentAdapter
			.publish(&root, plan, &CancellationToken::new())
			.await
			.expect("environment must publish");
		Ok((temp, root, binding))
	}

	fn write_provider(
		root: &Path,
		name: &str,
		files: &[(&str, &[u8])],
		metadata: &str,
	) -> Result<(), Box<dyn Error>> {
		let directory = root.join("mods").join(name);
		fs::create_dir(&directory)?;
		for (path, contents) in files {
			let destination = directory.join(path);
			if let Some(parent) = destination.parent() {
				fs::create_dir_all(parent)?;
			}
			fs::write(destination, contents)?;
		}
		fs::write(directory.join("meta.toml"), metadata)?;
		Ok(())
	}

	#[tokio::test]
	async fn composed_scan_ports_keep_distinct_supplied_bindings() -> Result<(), Box<dyn Error>> {
		let (temp, root, first) = fixture().await?;
		let second_path = temp.path().join("second-game");
		fs::create_dir_all(second_path.join("Data"))?;
		fs::write(first.game_directory().as_path().join("Data/first.txt"), b"first")?;
		fs::write(second_path.join("Data/second.txt"), b"second")?;
		let second =
			GameBinding::new(GameInstallationPath::new(second_path).map_err(|_| "invalid second game")?);
		let first_scan = EnvironmentAdapter.scan_environment_conflicts_port(root.clone(), first);
		let second_scan = EnvironmentAdapter.scan_environment_conflicts_port(root.clone(), second);
		fs::write(root.as_path().join("mods.toml"), "not a settings source anymore")?;

		let first = first_scan
			.call((CancellationToken::new(),))
			.await
			.map_err(|_| "first scan failed")?;
		let second = second_scan
			.call((CancellationToken::new(),))
			.await
			.map_err(|_| "second scan failed")?;
		assert!(first.providers[0]
			.files
			.iter()
			.any(|file| file.id.path().as_str() == "first.txt"));
		assert!(second.providers[0]
			.files
			.iter()
			.any(|file| file.id.path().as_str() == "second.txt"));
		Ok(())
	}

	#[tokio::test]
	async fn scan_enumerates_base_mods_disabled_state_overwrite_and_tombstones() -> Result<(), Box<dyn Error>> {
		let (temp, root, binding) = fixture().await?;
		fs::write(temp.path().join("game/Data/Textures/Shared.dds"), b"base").or_else(|error| {
			if error.kind() == ErrorKind::NotFound {
				fs::create_dir_all(temp.path().join("game/Data/Textures"))?;
				fs::write(temp.path().join("game/Data/Textures/Shared.dds"), b"base")
			} else {
				Err(error)
			}
		})?;
		write_provider(
			root.as_path(),
			"Low",
			&[("textures/shared.DDS", b"low")],
			"schema_version = 1\n",
		)?;
		write_provider(
			root.as_path(),
			"Disabled",
			&[("textures/shared.dds", b"disabled")],
			"schema_version = 1\n",
		)?;
		write_provider(
			root.as_path(),
			"High",
			&[],
			"schema_version = 1\n[tombstones]\ndirectories = [\"textures/old\"]\n",
		)?;
		fs::create_dir_all(root.as_path().join("overwrite/textures"))?;
		fs::write(root.as_path().join("overwrite/textures/shared.dds"), b"overwrite")?;
		fs::write(
			root.as_path().join("profile/modlist.txt"),
			b"+High\r\n-Disabled\n+Low\r\n",
		)?;

		let completed = scan(root.as_path(), &binding, &CancellationToken::new())
			.await
			.expect("scan must complete");

		assert!(completed.problems.is_empty());
		assert_eq!(completed.providers.len(), 5);
		assert!(matches!(completed.providers[0].identity, ProviderIdentity::SteamData));
		let disabled = &completed.providers[2];
		assert!(!disabled.enabled);
		assert!(matches!(
			disabled.files[0].provider.participation_reason(),
			ParticipationReason::DisabledMod
		));
		let high = &completed.providers[3];
		assert_eq!(high.tombstones.len(), 1);
		assert_eq!(high.tombstones[0].scope, TombstoneScope::DirectorySubtree);
		assert!(matches!(completed.providers[4].identity, ProviderIdentity::Overwrite));
		assert_eq!(
			completed
				.providers
				.iter()
				.map(|provider| provider.files.len())
				.sum::<usize>(),
			4
		);
		Ok(())
	}

	#[tokio::test]
	async fn missing_metadata_has_no_tombstones_or_metadata_problem() -> Result<(), Box<dyn Error>> {
		let (_temp, root, binding) = fixture().await?;
		write_provider(
			root.as_path(),
			"Plain",
			&[("ordinary.dds", b"content")],
			"schema_version = 1\n",
		)?;
		fs::remove_file(root.as_path().join("mods/Plain/meta.toml"))?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Plain\n")?;

		let completed = scan(root.as_path(), &binding, &CancellationToken::new())
			.await
			.expect("scan must complete");
		let provider = completed
			.providers
			.iter()
			.find(|provider| matches!(provider.identity, ProviderIdentity::DataMod { .. }))
			.ok_or("Data Mod must be scanned")?;
		assert!(completed.problems.is_empty());
		assert!(provider.problems.is_empty());
		assert!(provider.tombstones.is_empty());
		assert_eq!(provider.files.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn present_invalid_metadata_is_reported() -> Result<(), Box<dyn Error>> {
		for metadata in ["not = [", "schema_version = 2\n"] {
			let (_temp, root, binding) = fixture().await?;
			write_provider(root.as_path(), "Broken", &[], metadata)?;
			fs::write(root.as_path().join("profile/modlist.txt"), b"+Broken\n")?;

			let completed = scan(root.as_path(), &binding, &CancellationToken::new())
				.await
				.expect("scan must complete");
			let provider = completed
				.providers
				.iter()
				.find(|provider| matches!(provider.identity, ProviderIdentity::DataMod { .. }))
				.ok_or("Data Mod must be scanned")?;
			assert!(provider
				.problems
				.iter()
				.any(|problem| problem.kind == ConflictProblemKind::InvalidTombstoneMetadata));
		}
		Ok(())
	}

	#[tokio::test]
	async fn modlist_lists_the_highest_priority_first() {
		let (installed, problems) = parse_modlist(b"# Mod Organizer header\r\n+High\r\n-Disabled\r\n+Base\r\n");

		assert!(problems.is_empty());
		let priorities: Vec<_> = installed
			.iter()
			.map(|installed| (installed.name.as_str(), installed.priority.get()))
			.collect();
		assert_eq!(priorities, [("Base", 0), ("Disabled", 1), ("High", 2)]);
	}

	#[tokio::test]
	async fn modlist_rejects_surrounding_name_whitespace() {
		let (installed, problems) = parse_modlist(b"+ Visuals\n-Trailing \n");

		assert!(installed.is_empty());
		assert_eq!(problems.len(), 2);
		assert!(problems
			.iter()
			.all(|problem| problem.kind == ConflictProblemKind::ModlistInvalid));
	}

	#[cfg(windows)]
	#[tokio::test]
	async fn windows_sharing_and_lock_violations_are_content_unavailability() {
		assert!(content_access_is_unavailable(&IoError::from_raw_os_error(
			super::WINDOWS_ERROR_SHARING_VIOLATION
		)));
		assert!(content_access_is_unavailable(&IoError::from_raw_os_error(
			super::WINDOWS_ERROR_LOCK_VIOLATION
		)));
	}

	#[tokio::test]
	async fn unlisted_entries_are_ignored_and_a_case_variant_directory_is_missing() -> Result<(), Box<dyn Error>> {
		let (_temp, root, binding) = fixture().await?;
		for name in ["Visuals", "Unlisted"] {
			write_provider(
				root.as_path(),
				name,
				&[("content.txt", b"content")],
				"schema_version = 1\n",
			)?;
		}
		fs::write(root.as_path().join("mods/stray.txt"), b"stray")?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+visuals\n")?;

		let completed = scan(root.as_path(), &binding, &CancellationToken::new())
			.await
			.expect("scan must complete");

		assert!(completed.problems.is_empty());
		let data_mods = completed
			.providers
			.iter()
			.filter_map(|provider| match &provider.identity {
				ProviderIdentity::DataMod { mod_name, .. } => Some((mod_name.as_str(), provider)),
				_ => None,
			})
			.collect::<Vec<_>>();
		assert_eq!(data_mods.len(), 1);
		let (name, provider) = data_mods[0];
		assert_eq!(name, "visuals");
		assert!(provider.files.is_empty());
		assert_eq!(
			provider.problems.iter().map(|problem| problem.kind).collect::<Vec<_>>(),
			[ConflictProblemKind::ProviderMissing]
		);
		Ok(())
	}

	#[tokio::test]
	async fn completed_semantic_failures_are_typed_invalid_problems() -> Result<(), Box<dyn Error>> {
		let (_temp, root, binding) = fixture().await?;
		write_provider(
			root.as_path(),
			"Broken",
			&[("same.txt", b"file")],
			"schema_version = 1\n[tombstones]\nfiles = [\"same.txt\", \"SAME.TXT\"]\n",
		)?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Broken\n+broken\n")?;

		let completed = scan(root.as_path(), &binding, &CancellationToken::new())
			.await
			.expect("scan must complete");
		let kinds = completed
			.problems
			.iter()
			.chain(completed.providers.iter().flat_map(|provider| &provider.problems))
			.map(|problem| problem.kind)
			.collect::<Vec<_>>();

		assert!(kinds.contains(&ConflictProblemKind::ModlistInvalid));
		assert!(kinds.contains(&ConflictProblemKind::InvalidTombstonePath));
		assert!(kinds.contains(&ConflictProblemKind::OrdinaryTombstoneCollision));
		Ok(())
	}

	#[tokio::test]
	async fn root_metadata_is_excluded_by_windows_case_insensitive_name() -> Result<(), Box<dyn Error>> {
		let (_temp, root, binding) = fixture().await?;
		write_provider(
			root.as_path(),
			"Aliased",
			&[("content.txt", b"content")],
			"schema_version = 1\n",
		)?;
		fs::rename(
			root.as_path().join("mods/Aliased/meta.toml"),
			root.as_path().join("mods/Aliased/META.TOML"),
		)?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Aliased\n")?;

		let completed = scan(root.as_path(), &binding, &CancellationToken::new())
			.await
			.expect("scan must complete");
		let provider = completed
			.providers
			.iter()
			.find(|provider| matches!(&provider.identity, ProviderIdentity::DataMod { mod_name, .. } if mod_name.as_str() == "Aliased"))
			.ok_or("aliased provider")?;

		assert!(provider
			.files
			.iter()
			.all(|file| file.provider.original_path().comparison_key() != "meta.toml"));
		assert!(provider
			.problems
			.iter()
			.any(|problem| problem.kind == ConflictProblemKind::ReservedPath));
		Ok(())
	}

	#[tokio::test]
	async fn modlist_access_failure_aborts_the_whole_scan_as_io_failure() -> Result<(), Box<dyn Error>> {
		let (_temp, root, binding) = fixture().await?;
		fs::remove_file(root.as_path().join("profile/modlist.txt"))?;

		let error = scan(root.as_path(), &binding, &CancellationToken::new())
			.await
			.expect_err("missing modlist must abort the scan");

		assert_eq!(error.current_context().code(), ErrorCode::IoFailure);
		Ok(())
	}

	#[tokio::test]
	async fn indexed_content_reads_hash_once_opened_and_report_namespace_replacements_as_failures()
	-> Result<(), Box<dyn Error>> {
		let (_temp, root, binding) = fixture().await?;
		write_provider(
			root.as_path(),
			"Hashable",
			&[("file.txt", b"abc")],
			"schema_version = 1\n",
		)?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Hashable\n")?;
		let completed = scan(root.as_path(), &binding, &CancellationToken::new())
			.await
			.expect("scan must complete");
		let id = completed
			.providers
			.iter()
			.flat_map(|provider| &provider.files)
			.find(|file| matches!(file.id.identity(), ProviderIdentity::DataMod { .. }))
			.map(|file| &file.id)
			.ok_or("indexed mod file")?;

		let read = read_content(root.as_path(), &binding, id, &CancellationToken::new())
			.await
			.expect("content read must complete");
		let ConflictContentRead::Sha256(digest) = read else {
			return Err("stable content must hash".into());
		};
		assert_eq!(
			digest.as_str(),
			"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
		);

		fs::remove_file(root.as_path().join("mods/Hashable/file.txt"))?;
		let error = read_content(root.as_path(), &binding, id, &CancellationToken::new())
			.await
			.expect_err("deleted indexed content must invalidate the complete query");
		assert_eq!(error.current_context().code(), ErrorCode::IoFailure);
		Ok(())
	}
}
