use crate::hashing;
use crate::manifest::manifest_game_binding;
use crate::profile::MAX_PROFILE_BYTES;
use crate::safe_fs::EntryBudget;
use crate::safe_fs::MAX_TRAVERSAL_DEPTH;
use crate::safe_fs::SafeDir;
use crate::safe_fs::is_reparse;
use crate::safe_fs::read_bounded;
use crate::snapshot::MAX_MODS;
use crate::snapshot::MAX_PROVIDER_ENTRIES;
use crate::snapshot::MAX_PROVIDER_METADATA_BYTES;
use application::ErrorMarker;
use application::conflicts::ConflictContentRead;
use application::conflicts::EnvironmentConflictScan;
use application::conflicts::IndexedConflictFile;
use application::conflicts::IndexedConflictFileId;
use application::conflicts::ScannedConflictProvider;
use cap_fs_ext::MetadataExt;
use domain::ConflictProblem;
use domain::ConflictProblemKind;
use domain::DataRelativePath;
use domain::InstalledMod;
use domain::ModName;
use domain::ModPriority;
use domain::ParticipationReason;
use domain::ProblemScope;
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
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::str::from_utf8;
use tokio_util::sync::CancellationToken;
use toml::Value;
use toml::from_str;

const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];

pub(crate) fn scan(root_path: &Path, cancellation: &CancellationToken) -> Result<EnvironmentConflictScan, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let root = SafeDir::open_absolute(root_path).map_err(|error| {
		if error.current_context().kind() == io::ErrorKind::NotFound {
			error.context(ErrorMarker::environment_not_initialized())
		} else {
			error.context(ErrorMarker::io_failure())
		}
	})?;
	if !root.exists("mods.toml").context(ErrorMarker::io_failure())? {
		return Err(report!(ErrorMarker::environment_not_initialized()));
	}
	let profile = open_required_directory(&root, "profile")?;
	let mods = open_required_directory(&root, "mods")?;
	let overwrite = open_required_directory(&root, "overwrite")?;
	let _cache = open_required_directory(&root, "cache")?;
	let temp = open_required_directory(&root, "temp")?;
	if directory_has_entries(&temp, cancellation)? {
		return Err(report!(ErrorMarker::environment_invalid(Some("pending_operation"))));
	}

	let modlist = read_bounded(
		&profile,
		"modlist.txt",
		MAX_PROFILE_BYTES,
		ErrorMarker::io_failure(),
		cancellation,
	)?;
	let (installed_mods, mut problems) = parse_modlist(&modlist);
	let mut mod_directories = enumerate_mod_directories(&mods, cancellation, &mut problems)?;
	if mod_directories.len() > MAX_MODS {
		return Err(report!(ErrorMarker::io_failure()));
	}

	let mut providers = Vec::new();
	let game_binding = manifest_game_binding(&root, cancellation)?;
	let game = SafeDir::open_absolute(game_binding.game_directory().as_path())
		.context(ErrorMarker::environment_invalid(Some("game_binding")))?;
	match game.open_dir("Data") {
		Ok(data) => providers.push(scan_provider(
			&data,
			ProviderIdentity::SteamData,
			true,
			false,
			&mut problems,
			cancellation,
		)?),
		Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::ProviderMissing,
				scope: ProblemScope::Provider(ProviderIdentity::SteamData),
			});
			providers.push(empty_provider(ProviderIdentity::SteamData, true));
		}
		Err(error) => return Err(error.context(ErrorMarker::io_failure())),
	}

	for installed in installed_mods {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let key = installed.name.comparison_key().to_owned();
		let identity = ProviderIdentity::DataMod {
			mod_name: installed.name.clone(),
			priority: installed.priority,
		};
		let Some((directory_name, canonical_name)) = mod_directories.remove(&key) else {
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::ProviderMissing,
				scope: ProblemScope::Provider(identity.clone()),
			});
			providers.push(empty_provider(identity, installed.enabled));
			continue;
		};
		if canonical_name.as_str() != installed.name.as_str() {
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::ModlistInvalid,
				scope: ProblemScope::Modlist,
			});
		}
		let identity = ProviderIdentity::DataMod {
			mod_name: canonical_name,
			priority: installed.priority,
		};
		let directory = mods.open_dir(&directory_name).context(ErrorMarker::io_failure())?;
		providers.push(scan_provider(
			&directory,
			identity,
			installed.enabled,
			true,
			&mut problems,
			cancellation,
		)?);
	}
	if !mod_directories.is_empty() {
		problems.push(ConflictProblem {
			kind: ConflictProblemKind::ModlistInvalid,
			scope: ProblemScope::Modlist,
		});
	}

	providers.push(scan_provider(
		&overwrite,
		ProviderIdentity::Overwrite,
		true,
		false,
		&mut problems,
		cancellation,
	)?);

	Ok(EnvironmentConflictScan { providers, problems })
}

pub(crate) fn read_content(
	root_path: &Path,
	id: &IndexedConflictFileId,
	cancellation: &CancellationToken,
) -> Result<ConflictContentRead, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let Ok(root) = SafeDir::open_absolute(root_path) else {
		return Ok(ConflictContentRead::Unavailable);
	};
	let provider = match id.identity() {
		ProviderIdentity::SteamData => {
			let Ok(binding) = manifest_game_binding(&root, cancellation) else {
				return Ok(ConflictContentRead::Unavailable);
			};
			let Ok(game) = SafeDir::open_absolute(binding.game_directory().as_path()) else {
				return Ok(ConflictContentRead::Unavailable);
			};
			let Ok(data) = game.open_dir("Data") else {
				return Ok(ConflictContentRead::Unavailable);
			};
			data
		}
		ProviderIdentity::DataMod { mod_name, .. } => {
			let Ok(mods) = root.open_dir("mods") else {
				return Ok(ConflictContentRead::Unavailable);
			};
			let Ok(directory) = mods.open_dir(mod_name.as_str()) else {
				return Ok(ConflictContentRead::Unavailable);
			};
			directory
		}
		ProviderIdentity::Overwrite => {
			let Ok(directory) = root.open_dir("overwrite") else {
				return Ok(ConflictContentRead::Unavailable);
			};
			directory
		}
	};

	let mut current = provider;
	let mut components = id.path().components().peekable();
	while let Some(component) = components.next() {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if components.peek().is_none() {
			let Ok(file) = current.open_regular(component) else {
				return Ok(ConflictContentRead::Unavailable);
			};
			return hashing::sha256(file, cancellation);
		}
		let Ok(child) = current.open_dir(component) else {
			return Ok(ConflictContentRead::Unavailable);
		};
		current = child;
	}
	Ok(ConflictContentRead::Unavailable)
}

fn empty_provider(identity: ProviderIdentity, enabled: bool) -> ScannedConflictProvider {
	ScannedConflictProvider {
		identity,
		enabled,
		files: Vec::new(),
		directories: Vec::new(),
		tombstones: Vec::new(),
	}
}

fn open_required_directory(root: &SafeDir, name: &str) -> Result<SafeDir, ErrorMarker> {
	root.open_dir(name).map_err(|error| {
		if error.current_context().kind() == io::ErrorKind::NotFound {
			error.context(ErrorMarker::environment_not_initialized())
		} else {
			error.context(ErrorMarker::io_failure())
		}
	})
}

fn directory_has_entries(directory: &SafeDir, cancellation: &CancellationToken) -> Result<bool, ErrorMarker> {
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::io_failure())?;
	let next = entries.next();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	match next {
		Some(entry) => {
			entry.into_report().context(ErrorMarker::io_failure())?;
			Ok(true)
		}
		None => Ok(false),
	}
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
	let mut installed = Vec::new();
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
		let Ok(name) = ModName::new(name.to_owned()) else {
			problems.push(modlist_problem());
			continue;
		};
		if !seen.insert(name.comparison_key().to_owned()) {
			problems.push(modlist_problem());
			continue;
		}
		let Ok(priority) = u32::try_from(installed.len()) else {
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

fn enumerate_mod_directories(
	mods: &SafeDir,
	cancellation: &CancellationToken,
	problems: &mut Vec<ConflictProblem>,
) -> Result<HashMap<String, (OsString, ModName)>, ErrorMarker> {
	let opened = mods.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::io_failure())?;
	let mut result = HashMap::new();
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		let entry = entry.into_report().context(ErrorMarker::io_failure())?;
		let os_name = entry.file_name();
		let Some(spelling) = os_name.to_str() else {
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::NonLosslessName,
				scope: ProblemScope::Global,
			});
			continue;
		};
		let Ok(name) = ModName::new(spelling.to_owned()) else {
			problems.push(modlist_problem());
			continue;
		};
		let metadata = mods.entry_metadata(&os_name).context(ErrorMarker::io_failure())?;
		if is_reparse(&metadata) || !metadata.is_dir() {
			problems.push(ConflictProblem {
				kind: if is_reparse(&metadata) {
					ConflictProblemKind::ReparsePoint
				} else {
					ConflictProblemKind::ProviderMissing
				},
				scope: ProblemScope::Global,
			});
			continue;
		}
		let key = name.comparison_key().to_owned();
		if result.insert(key, (os_name, name)).is_some() {
			problems.push(modlist_problem());
		}
	}
	Ok(result)
}

fn scan_provider(
	directory: &SafeDir,
	identity: ProviderIdentity,
	enabled: bool,
	require_metadata: bool,
	problems: &mut Vec<ConflictProblem>,
	cancellation: &CancellationToken,
) -> Result<ScannedConflictProvider, ErrorMarker> {
	let mut provider = empty_provider(identity.clone(), enabled);
	let mut keys = HashMap::new();
	let mut budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
	scan_provider_directory(
		directory,
		directory,
		&identity,
		enabled,
		"",
		&mut provider,
		&mut keys,
		problems,
		cancellation,
		&mut budget,
		MAX_TRAVERSAL_DEPTH,
	)?;

	let metadata_exists = directory.exists("meta.toml").context(ErrorMarker::io_failure())?;
	if require_metadata && !metadata_exists {
		problems.push(ConflictProblem {
			kind: ConflictProblemKind::InvalidTombstoneMetadata,
			scope: ProblemScope::Provider(identity.clone()),
		});
	}
	if metadata_exists {
		provider.tombstones = read_tombstones(directory, &identity, enabled, problems, cancellation)?;
	}
	validate_provider_tombstones(&provider, &keys, problems);
	Ok(provider)
}

#[expect(
	clippy::too_many_arguments,
	reason = "bounded recursive enumeration keeps safety state explicit at the adapter boundary"
)]
fn scan_provider_directory(
	root: &SafeDir,
	directory: &SafeDir,
	identity: &ProviderIdentity,
	enabled: bool,
	prefix: &str,
	provider: &mut ScannedConflictProvider,
	keys: &mut HashMap<String, bool>,
	problems: &mut Vec<ConflictProblem>,
	cancellation: &CancellationToken,
	budget: &mut EntryBudget,
	remaining_depth: usize,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::io_failure())?;
	let mut directory_entries = 0_usize;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		let entry = entry.into_report().context(ErrorMarker::io_failure())?;
		budget.consume(&mut directory_entries)
			.context(ErrorMarker::io_failure())?;
		if remaining_depth == 0 {
			return Err(
				report!(io::Error::other("provider traversal depth changed during scan"))
					.context(ErrorMarker::io_failure()),
			);
		}
		let os_name = entry.file_name();
		let Some(spelling) = os_name.to_str() else {
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::NonLosslessName,
				scope: ProblemScope::Provider(identity.clone()),
			});
			continue;
		};
		if prefix.is_empty() && spelling == "meta.toml" && identity.class() != domain::ProviderClass::SteamData
		{
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
		if identity.class() == domain::ProviderClass::SteamData
			&& prefix.is_empty() && path.comparison_key() == case_fold_key("meta.toml")
		{
			problems.push(path_problem(ConflictProblemKind::ReservedPath, &path));
			continue;
		}
		let metadata = directory.entry_metadata(&os_name).context(ErrorMarker::io_failure())?;
		if is_reparse(&metadata) {
			problems.push(path_problem(ConflictProblemKind::ReparsePoint, &path));
			continue;
		}
		let is_directory = metadata.is_dir();
		if keys.insert(path.comparison_key().to_owned(), is_directory).is_some() {
			problems.push(path_problem(ConflictProblemKind::InternalKeyCollision, &path));
			continue;
		}
		if is_directory {
			let reference = provider_reference(identity, path.clone(), enabled);
			provider.directories.push(reference);
			let child = directory.open_dir(&os_name).context(ErrorMarker::io_failure())?;
			if !root.is_ancestor_of(&child).context(ErrorMarker::io_failure())? {
				problems.push(path_problem(ConflictProblemKind::ContainmentEscape, &path));
				continue;
			}
			scan_provider_directory(
				root,
				&child,
				identity,
				enabled,
				&relative,
				provider,
				keys,
				problems,
				cancellation,
				budget,
				remaining_depth - 1,
			)?;
			continue;
		}
		if !metadata.is_file() {
			problems.push(path_problem(ConflictProblemKind::UnsupportedEntryType, &path));
			continue;
		}
		if metadata.nlink() != 1 {
			problems.push(path_problem(ConflictProblemKind::HardLink, &path));
			continue;
		}
		if directory.open_regular(&os_name).is_err() {
			return Err(report!(io::Error::other("provider file changed during scan"))
				.context(ErrorMarker::io_failure()));
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
	directory: &SafeDir,
	identity: &ProviderIdentity,
	enabled: bool,
	problems: &mut Vec<ConflictProblem>,
	cancellation: &CancellationToken,
) -> Result<Vec<Tombstone>, ErrorMarker> {
	let bytes = read_bounded(
		directory,
		"meta.toml",
		MAX_PROVIDER_METADATA_BYTES,
		ErrorMarker::io_failure(),
		cancellation,
	)?;
	let Ok(text) = from_utf8(&bytes) else {
		problems.push(ConflictProblem {
			kind: ConflictProblemKind::InvalidTombstoneMetadata,
			scope: ProblemScope::Provider(identity.clone()),
		});
		return Ok(Vec::new());
	};
	let Ok(value) = from_str::<Value>(text) else {
		problems.push(ConflictProblem {
			kind: ConflictProblemKind::InvalidTombstoneMetadata,
			scope: ProblemScope::Provider(identity.clone()),
		});
		return Ok(Vec::new());
	};
	let Some(table) = value.as_table() else {
		problems.push(metadata_problem(identity));
		return Ok(Vec::new());
	};
	if table.get("schema_version").and_then(Value::as_integer) != Some(1) {
		problems.push(metadata_problem(identity));
		return Ok(Vec::new());
	}
	let Some(tombstone_value) = table.get("tombstones") else {
		return Ok(Vec::new());
	};
	let Some(tombstone_table) = tombstone_value.as_table() else {
		problems.push(metadata_problem(identity));
		return Ok(Vec::new());
	};
	if tombstone_table
		.keys()
		.any(|key| !matches!(key.as_str(), "files" | "directories"))
	{
		problems.push(metadata_problem(identity));
		return Ok(Vec::new());
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
	Ok(result)
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
	use domain::SteamBuildId;
	use domain::TombstoneScope;
	use std::env::current_dir;
	use std::error::Error;
	use std::fs;
	use std::path::Path;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	fn fixture() -> Result<(TempDir, EnvironmentRoot), Box<dyn Error>> {
		let temp = TempDir::new_in(current_dir()?)?;
		let root = EnvironmentRoot::new(temp.path().join("environment"))
			.map_err(|_| "invalid environment root")?;
		let game = temp.path().join("game");
		fs::create_dir_all(game.join("Data"))?;
		let plan = InitializationPlan {
			game_binding: GameBinding::new(
				GameInstallationPath::new(game).map_err(|_| "invalid game path")?,
				SteamBuildId::new(1).map_err(|_| "invalid build ID")?,
			),
			profile_sources: InitializationProfileSources {
				files: PROFILE_FILES
					.into_iter()
					.map(|name| ProfileSource { name, contents: None })
					.collect(),
				fallout_default_ini: b"[Archive]\r\nsArchiveList=Fallout - Meshes.bsa\r\n".to_vec(),
			},
		};
		EnvironmentAdapter
			.publish(&root, plan, &CancellationToken::new())
			.expect("environment must publish");
		Ok((temp, root))
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

	#[test]
	fn scan_enumerates_base_mods_disabled_state_overwrite_and_tombstones() -> Result<(), Box<dyn Error>> {
		let (temp, root) = fixture()?;
		fs::write(temp.path().join("game/Data/Textures/Shared.dds"), b"base").or_else(|error| {
			if error.kind() == std::io::ErrorKind::NotFound {
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
			b"+Low\r\n-Disabled\n+High\r\n",
		)?;

		let completed = scan(root.as_path(), &CancellationToken::new()).expect("scan must complete");

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

	#[test]
	fn completed_semantic_failures_are_typed_invalid_problems() -> Result<(), Box<dyn Error>> {
		let (_temp, root) = fixture()?;
		write_provider(
			root.as_path(),
			"Broken",
			&[("same.txt", b"file")],
			"schema_version = 1\n[tombstones]\nfiles = [\"same.txt\", \"SAME.TXT\"]\n",
		)?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Broken\n+broken\n")?;

		let completed = scan(root.as_path(), &CancellationToken::new()).expect("scan must complete");
		let kinds = completed
			.problems
			.iter()
			.map(|problem| problem.kind)
			.collect::<Vec<_>>();

		assert!(kinds.contains(&ConflictProblemKind::ModlistInvalid));
		assert!(kinds.contains(&ConflictProblemKind::InvalidTombstonePath));
		assert!(kinds.contains(&ConflictProblemKind::OrdinaryTombstoneCollision));
		Ok(())
	}

	#[test]
	fn modlist_access_failure_aborts_the_whole_scan_as_io_failure() -> Result<(), Box<dyn Error>> {
		let (_temp, root) = fixture()?;
		fs::remove_file(root.as_path().join("profile/modlist.txt"))?;

		let error = scan(root.as_path(), &CancellationToken::new())
			.expect_err("missing modlist must abort the scan");

		assert_eq!(error.current_context().code(), ErrorCode::IoFailure);
		Ok(())
	}

	#[test]
	fn indexed_content_reads_hash_once_opened_and_report_deleted_files_unavailable() -> Result<(), Box<dyn Error>> {
		let (_temp, root) = fixture()?;
		write_provider(
			root.as_path(),
			"Hashable",
			&[("file.txt", b"abc")],
			"schema_version = 1\n",
		)?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Hashable\n")?;
		let completed = scan(root.as_path(), &CancellationToken::new()).expect("scan must complete");
		let id = completed
			.providers
			.iter()
			.flat_map(|provider| &provider.files)
			.find(|file| matches!(file.id.identity(), ProviderIdentity::DataMod { .. }))
			.map(|file| &file.id)
			.ok_or("indexed mod file")?;

		let read = read_content(root.as_path(), id, &CancellationToken::new())
			.expect("content read must complete");
		let ConflictContentRead::Sha256(digest) = read else {
			return Err("stable content must hash".into());
		};
		assert_eq!(
			digest.as_str(),
			"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
		);

		fs::remove_file(root.as_path().join("mods/Hashable/file.txt"))?;
		assert_eq!(
			read_content(root.as_path(), id, &CancellationToken::new())
				.expect("unavailable content read must complete"),
			ConflictContentRead::Unavailable
		);
		Ok(())
	}
}
