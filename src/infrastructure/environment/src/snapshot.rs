use crate::manifest::manifest_game_binding;
use crate::manifest::validate_manifest;
use crate::profile::MAX_PROFILE_BYTES;
use crate::profile::is_activatable_plugin_name;
use crate::profile::validate_execution_profile;
use crate::profile::validate_profile_files;
use crate::profile_activation::ProfileActivation;
use crate::publication::OPERATION_DIRECTORY;
use crate::safe_fs::EntryBudget;
use crate::safe_fs::MAX_TRAVERSAL_DEPTH;
use crate::safe_fs::SafeDir;
use crate::safe_fs::is_reparse;
use crate::safe_fs::read_bounded;
use crate::safe_fs::validate_exact_entries;
use crate::validate_bsa_file;
use application::ErrorCode;
use application::ErrorMarker;
use application::installation::CandidateDecision;
use application::installation::EffectiveResult;
use application::installation::FileDependencyFact;
use application::installation::FileDependencyKind;
use application::installation::InstallOverlap;
use application::installation::InstallPlan;
use application::installation::InstallationAssessment;
use application::installation::TombstoneReference;
use application::installation::TombstoneScope;
use application::ports::InstallationStateAccess;
use cap_fs_ext::MetadataExt;
use domain::DataRelativePath;
use domain::FileDependencyState;
use domain::GameBinding;
use domain::InstalledMod;
use domain::ModName;
use domain::ModPriority;
use domain::ParticipationReason;
use domain::ProviderClass;
use domain::ProviderReference;
use domain::TombstoneIndex;
use domain::case_fold_key;
use domain::resolve_effective_file;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::str::from_utf8;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use toml::Value;
use toml::from_str;

const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];
const INVALIDATION_ARCHIVE: &str = "Fallout - Invalidation.bsa";
pub(crate) const MAX_MODS: usize = 4096;
pub(crate) const MAX_PROVIDER_ENTRIES: usize = 100_000;
pub(crate) const MAX_PROVIDER_METADATA_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct EnvironmentSnapshotData {
	pub(crate) game_binding: GameBinding,
	pub(crate) installed_mods: Vec<InstalledMod>,
	pub(crate) current_winners: HashMap<DataRelativePath, EffectiveResult>,
	pub(crate) file_dependencies: HashMap<String, FileDependencyFact>,
}

pub(crate) fn load(
	root_path: &Path,
	access: InstallationStateAccess,
	cancellation: &CancellationToken,
) -> Result<EnvironmentSnapshotData, ErrorMarker> {
	load_inner(root_path, SnapshotLoad::Installation(access), None, None, cancellation)
}

pub(crate) fn load_during_publication(
	root_path: &Path,
	cancellation: &CancellationToken,
) -> Result<EnvironmentSnapshotData, ErrorMarker> {
	load_inner(root_path, SnapshotLoad::Publication, None, None, cancellation)
}

pub(crate) fn load_execution(
	root_path: &Path,
	binding: &GameBinding,
	owned_spool: Option<&TempDir>,
	cancellation: &CancellationToken,
) -> Result<EnvironmentSnapshotData, ErrorMarker> {
	load_inner(
		root_path,
		SnapshotLoad::Installation(InstallationStateAccess::Mutation),
		Some(binding),
		owned_spool,
		cancellation,
	)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotLoad {
	Installation(InstallationStateAccess),
	Publication,
}

fn load_inner(
	root_path: &Path,
	load: SnapshotLoad,
	effective_binding: Option<&GameBinding>,
	owned_spool: Option<&TempDir>,
	cancellation: &CancellationToken,
) -> Result<EnvironmentSnapshotData, ErrorMarker> {
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
	let temp_exists = root.exists("temp");
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let temp_exists = temp_exists.context(ErrorMarker::environment_invalid(None))?;
	if load == SnapshotLoad::Publication && !temp_exists {
		return Err(report!(ErrorMarker::manual_cleanup_required()));
	}
	if temp_exists {
		let opened_temp = root.open_dir("temp");
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let temp = opened_temp.context(ErrorMarker::environment_invalid(None))?;
		if let SnapshotLoad::Installation(access) = load {
			let opened = temp.entries();
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let entries = opened.context(ErrorMarker::environment_invalid(None))?;
			for entry in entries {
				let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
				if owned_spool.is_some_and(|spool| {
					spool.path().parent() == Some(root_path.join("temp").as_path())
						&& spool.path().file_name() == Some(entry.file_name().as_os_str())
				}) {
					continue;
				}

				let marker = match access {
					InstallationStateAccess::Preview => ErrorMarker::environment_invalid(None),
					InstallationStateAccess::Mutation => ErrorMarker::manual_cleanup_required(),
				};
				return Err(report!(marker));
			}
		} else {
			let validation = validate_exact_entries(&temp, &[OPERATION_DIRECTORY], cancellation);
			if let Err(error) = validation {
				if error.current_context().code() == ErrorCode::OperationCancelled {
					return Err(error);
				}
				return Err(error.context(ErrorMarker::manual_cleanup_required()));
			}
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let current_operation_exists = temp.exists(OPERATION_DIRECTORY);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if !current_operation_exists.context(ErrorMarker::manual_cleanup_required())? {
				return Err(report!(ErrorMarker::manual_cleanup_required()));
			}
		}
	}

	validate_exact_entries(
		&root,
		&["mods", "profile", "overwrite", "cache", "mods.toml", "temp", "logs"],
		cancellation,
	)?;
	validate_manifest(&root, cancellation)?;
	let cache = root.open_dir("cache").context(ErrorMarker::environment_invalid(None))?;
	validate_exact_entries(&cache, &[INVALIDATION_ARCHIVE], cancellation)?;
	validate_bsa_file(&cache, cancellation)?;
	if root.exists("logs").context(ErrorMarker::environment_invalid(None))? {
		root.open_dir("logs").context(ErrorMarker::environment_invalid(None))?;
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mods = root.open_dir("mods").context(ErrorMarker::environment_invalid(None))?;
	let profile_dir = root
		.open_dir("profile")
		.context(ErrorMarker::environment_invalid(None))?;
	let overwrite = root
		.open_dir("overwrite")
		.context(ErrorMarker::environment_invalid(None))?;
	if effective_binding.is_some() {
		validate_execution_profile(&profile_dir, cancellation)?;
	} else {
		validate_profile_files(&profile_dir, false, cancellation)?;
	}
	validate_provider(&overwrite, ProviderKind::Overwrite, cancellation)?;

	let mut directories = HashMap::new();
	let opened = mods.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
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
		let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
		if directories.len() >= MAX_MODS {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let entry = entry.file_name();
		let spelling = entry
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		let name = ModName::new(spelling.to_owned()).context(ErrorMarker::environment_invalid(None))?;
		let key = name.comparison_key().to_owned();
		if directories.contains_key(&key) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let metadata = mods
			.symlink_metadata(&entry)
			.context(ErrorMarker::environment_invalid(None))?;
		if is_reparse(&metadata) || !metadata.is_dir() {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let directory = mods.open_dir(&entry).context(ErrorMarker::environment_invalid(None))?;
		if !mods.is_ancestor_of(&directory)
			.context(ErrorMarker::environment_invalid(None))?
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		validate_provider(&directory, ProviderKind::DataMod, cancellation)?;
		directories.insert(key, name);
	}

	let modlist = read_bounded(
		&profile_dir,
		"modlist.txt",
		MAX_PROFILE_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)?;
	let parsed = parse_modlist(&modlist)?;
	if parsed.len() != directories.len() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let mut installed_mods = Vec::with_capacity(parsed.len());
	for entry in parsed {
		let Some(canonical) = directories.remove(entry.name.comparison_key()) else {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		};
		if canonical.as_str() != entry.name.as_str() {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		installed_mods.push(InstalledMod {
			name: canonical,
			priority: entry.priority,
			enabled: entry.enabled,
		});
	}
	if !directories.is_empty() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let game_binding = if let Some(binding) = effective_binding {
		binding.clone()
	} else {
		manifest_game_binding(&root, cancellation)?
	};
	let current_winners = current_winners(&root, &mods, &installed_mods, &game_binding, cancellation)?;
	let file_dependencies = if effective_binding.is_some() {
		HashMap::new()
	} else {
		file_dependencies(&profile_dir, &current_winners, cancellation)?
	};
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	Ok(EnvironmentSnapshotData {
		game_binding,
		installed_mods,
		current_winners,
		file_dependencies,
	})
}

#[derive(Clone, Copy)]
enum ProviderKind {
	GameBase,
	DataMod,
	Overwrite,
}

pub(crate) fn validate_staged_provider(
	provider: &SafeDir,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	validate_provider(provider, ProviderKind::DataMod, cancellation)
}

fn validate_provider(
	provider: &SafeDir,
	kind: ProviderKind,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let mut paths = HashSet::new();
	let mut budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
	validate_provider_directory(
		provider,
		provider,
		kind,
		"",
		&mut paths,
		cancellation,
		&mut budget,
		MAX_TRAVERSAL_DEPTH,
	)?;
	if matches!(kind, ProviderKind::DataMod)
		&& !provider
			.exists("meta.toml")
			.context(ErrorMarker::environment_invalid(None))?
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	if !provider
		.exists("meta.toml")
		.context(ErrorMarker::environment_invalid(None))?
	{
		return Ok(());
	}

	let tombstones = validate_metadata(provider, cancellation)?;
	let mut directory_tombstones = HashSet::new();
	for (tombstone, directory) in &tombstones {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if *directory {
			directory_tombstones.insert(tombstone.comparison_key());
		}
	}

	for (tombstone, _) in &tombstones {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if paths.contains(tombstone.comparison_key()) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		for (boundary, _) in tombstone.comparison_key().match_indices('/') {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if directory_tombstones.contains(&tombstone.comparison_key()[..boundary]) {
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
		}
	}

	for path in &paths {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		for (boundary, _) in path.match_indices('/') {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if directory_tombstones.contains(&path[..boundary]) {
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
		}
	}
	Ok(())
}

#[expect(
	clippy::too_many_arguments,
	reason = "the explicit depth bound must remain local to recursive provider validation"
)]
fn validate_provider_directory(
	root: &SafeDir,
	directory: &SafeDir,
	kind: ProviderKind,
	prefix: &str,
	paths: &mut HashSet<String>,
	cancellation: &CancellationToken,
	budget: &mut EntryBudget,
	remaining_depth: usize,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let ancestry = root.is_ancestor_of(directory);
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	if !ancestry.context(ErrorMarker::environment_invalid(None))? {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
	let mut directory_entries = 0_usize;
	let mut local_names = HashSet::new();
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
		let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
		budget.consume(&mut directory_entries)
			.context(ErrorMarker::environment_invalid(None))?;
		if remaining_depth == 0 {
			return Err(report!(io::Error::new(
				io::ErrorKind::InvalidData,
				"directory traversal depth limit exceeded",
			))
			.context(ErrorMarker::environment_invalid(None)));
		}
		let entry = entry.file_name();
		let spelling = entry
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		let relative = if prefix.is_empty() {
			spelling.to_owned()
		} else {
			format!("{prefix}/{spelling}")
		};
		let data_path =
			DataRelativePath::new(relative.clone()).context(ErrorMarker::environment_invalid(None))?;
		if !local_names.insert(case_fold_key(spelling)) || !paths.insert(data_path.comparison_key().to_owned())
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let metadata = directory.symlink_metadata(&entry);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let metadata = metadata.context(ErrorMarker::environment_invalid(None))?;
		if metadata.is_dir() {
			let child = directory.open_dir(&entry);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			validate_provider_directory(
				root,
				&child.context(ErrorMarker::environment_invalid(None))?,
				kind,
				&relative,
				paths,
				cancellation,
				budget,
				remaining_depth - 1,
			)?;
			continue;
		}
		if !metadata.is_file() || metadata.nlink() != 1 {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let opened = directory.open_regular(&entry);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		opened.context(ErrorMarker::environment_invalid(None))?;
		if prefix.is_empty() {
			let identity = case_fold_key(spelling);
			if identity == case_fold_key(INVALIDATION_ARCHIVE) {
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			if identity == case_fold_key("meta.toml")
				&& (matches!(kind, ProviderKind::GameBase) || spelling != "meta.toml")
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
		}
	}
	Ok(())
}

fn validate_metadata(
	provider: &SafeDir,
	cancellation: &CancellationToken,
) -> Result<Vec<(DataRelativePath, bool)>, ErrorMarker> {
	let bytes = read_bounded(
		provider,
		"meta.toml",
		MAX_PROVIDER_METADATA_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)?;
	let text = from_utf8(&bytes).context(ErrorMarker::environment_invalid(None))?;
	let value: Value = from_str(text).context(ErrorMarker::environment_invalid(None))?;
	let table = value
		.as_table()
		.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
	if table.get("schema_version").and_then(Value::as_integer) != Some(1) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let metadata_identity = case_fold_key("meta.toml");
	let invalidation_archive_identity = case_fold_key(INVALIDATION_ARCHIVE);
	let mut result = Vec::new();
	let Some(tombstones) = table.get("tombstones") else {
		return Ok(result);
	};

	let tombstones = tombstones
		.as_table()
		.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
	if tombstones
		.keys()
		.any(|key| !matches!(key.as_str(), "files" | "directories"))
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let mut seen = HashSet::new();
	for (key, directory) in [("files", false), ("directories", true)] {
		let Some(values) = tombstones.get(key) else {
			continue;
		};
		let values = values
			.as_array()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		for value in values {
			let path = value
				.as_str()
				.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
			let canonical = DataRelativePath::new(path.to_owned())
				.context(ErrorMarker::environment_invalid(None))?;
			let reserved_root = canonical.components().count() == 1
				&& (canonical.comparison_key() == metadata_identity
					|| canonical.comparison_key() == invalidation_archive_identity);
			if path != canonical.as_str()
				|| reserved_root || !seen.insert(canonical.comparison_key().to_owned())
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			result.push((canonical, directory));
		}
	}
	Ok(result)
}

pub(crate) fn validate_prospective_namespace(
	root: &SafeDir,
	staged_mod: &SafeDir,
	plan: &InstallPlan,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let profile = root
		.open_dir("profile")
		.context(ErrorMarker::environment_invalid(None))?;
	let installed = parse_modlist(&read_bounded(
		&profile,
		"modlist.txt",
		MAX_PROFILE_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)?)?;
	let mods = root.open_dir("mods").context(ErrorMarker::environment_invalid(None))?;
	let mut namespace = HashMap::new();
	let mut winners = HashMap::new();
	let game = SafeDir::open_absolute(manifest_game_binding(root, cancellation)?.game_directory().as_path())
		.context(ErrorMarker::environment_invalid(None))?;
	match game.open_dir("Data") {
		Ok(data) => {
			validate_provider(&data, ProviderKind::GameBase, cancellation)?;
			apply_provider(
				&data,
				ProviderClass::SteamData,
				None,
				None,
				&mut namespace,
				&mut winners,
				cancellation,
			)?;
		}
		Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(error.context(ErrorMarker::environment_invalid(None))),
	}
	for installed_mod in installed.iter().filter(|installed_mod| installed_mod.enabled) {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		if plan.replacement && installed_mod.name == plan.mod_name {
			continue;
		}
		let directory = mods
			.open_dir(installed_mod.name.as_str())
			.context(ErrorMarker::environment_invalid(None))?;
		apply_provider(
			&directory,
			ProviderClass::DataMod,
			Some(installed_mod.name.clone()),
			Some(installed_mod.priority),
			&mut namespace,
			&mut winners,
			cancellation,
		)?;
	}
	apply_provider(
		staged_mod,
		ProviderClass::DataMod,
		Some(plan.mod_name.clone()),
		Some(plan.projected_state.priority),
		&mut namespace,
		&mut winners,
		cancellation,
	)?;
	let overwrite = root
		.open_dir("overwrite")
		.context(ErrorMarker::environment_invalid(None))?;
	apply_provider(
		&overwrite,
		ProviderClass::Overwrite,
		None,
		None,
		&mut namespace,
		&mut winners,
		cancellation,
	)?;
	Ok(())
}

fn current_winners(
	root: &SafeDir,
	mods: &SafeDir,
	installed: &[InstalledMod],
	binding: &GameBinding,
	cancellation: &CancellationToken,
) -> Result<HashMap<DataRelativePath, EffectiveResult>, ErrorMarker> {
	let mut namespace = HashMap::new();
	let mut winners = HashMap::new();
	let game = SafeDir::open_absolute(binding.game_directory().as_path())
		.context(ErrorMarker::environment_invalid(None))?;
	match game.open_dir("Data") {
		Ok(data) => {
			validate_provider(&data, ProviderKind::GameBase, cancellation)?;
			apply_provider(
				&data,
				ProviderClass::SteamData,
				None,
				None,
				&mut namespace,
				&mut winners,
				cancellation,
			)?;
		}
		Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(error.context(ErrorMarker::environment_invalid(None))),
	}
	for installed_mod in installed.iter().filter(|installed_mod| installed_mod.enabled) {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let directory = mods
			.open_dir(installed_mod.name.as_str())
			.context(ErrorMarker::environment_invalid(None))?;
		apply_provider(
			&directory,
			ProviderClass::DataMod,
			Some(installed_mod.name.clone()),
			Some(installed_mod.priority),
			&mut namespace,
			&mut winners,
			cancellation,
		)?;
	}
	let overwrite = root
		.open_dir("overwrite")
		.context(ErrorMarker::environment_invalid(None))?;
	apply_provider(
		&overwrite,
		ProviderClass::Overwrite,
		None,
		None,
		&mut namespace,
		&mut winners,
		cancellation,
	)?;

	let mut result = HashMap::with_capacity(winners.len());
	for (path, effective) in winners.into_values() {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		result.insert(path, effective);
	}
	Ok(result)
}

fn apply_provider(
	directory: &SafeDir,
	class: ProviderClass,
	mod_name: Option<ModName>,
	priority: Option<ModPriority>,
	namespace: &mut HashMap<String, bool>,
	winners: &mut HashMap<String, (DataRelativePath, EffectiveResult)>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	for (path, is_directory) in collect_entries(directory, "", cancellation)? {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let key = path.comparison_key().to_owned();
		if namespace.get(&key).is_some_and(|existing| *existing != is_directory) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		namespace.insert(key.clone(), is_directory);
		if !is_directory {
			let provider = provider_reference(class, mod_name.clone(), priority, path.clone())?;
			winners.insert(key, (path, EffectiveResult::File(provider)));
		}
	}
	if !directory
		.exists("meta.toml")
		.context(ErrorMarker::environment_invalid(None))?
	{
		return Ok(());
	}

	let mut path_tombstones = HashMap::new();
	let mut tombstone_index = TombstoneIndex::default();
	for (path, directory_scope) in validate_metadata(directory, cancellation)? {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let owner = provider_reference(class, mod_name.clone(), priority, path.clone())?;
		let tombstone = TombstoneReference {
			scope: if directory_scope {
				TombstoneScope::DirectorySubtree
			} else {
				TombstoneScope::ExactFile
			},
			owner,
		};
		tombstone_index.insert(tombstone.clone());
		path_tombstones.insert(path.comparison_key().to_owned(), (path, tombstone));
	}

	for (key, (_, effective)) in winners.iter_mut() {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if let Some(tombstone) = tombstone_index.controlling(key) {
			let file = if let EffectiveResult::File(file) = effective {
				Some(&*file)
			} else {
				None
			};
			*effective = resolve_effective_file(file, Some(tombstone));
		}
	}

	for (key, (path, tombstone)) in path_tombstones {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		winners.insert(
			key,
			(
				path,
				EffectiveResult::Absent {
					controlling_tombstone: Some(tombstone),
				},
			),
		);
	}
	Ok(())
}

fn provider_reference(
	class: ProviderClass,
	mod_name: Option<ModName>,
	priority: Option<ModPriority>,
	path: DataRelativePath,
) -> Result<ProviderReference, ErrorMarker> {
	match class {
		ProviderClass::SteamData => Ok(ProviderReference::SteamData { original_path: path }),
		ProviderClass::DataMod => Ok(ProviderReference::DataMod {
			mod_name: mod_name.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?,
			priority: priority.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?,
			original_path: path,
			participation_reason: ParticipationReason::EnabledMod,
		}),
		ProviderClass::Overwrite => Ok(ProviderReference::Overwrite { original_path: path }),
	}
}

fn collect_entries(
	directory: &SafeDir,
	prefix: &str,
	cancellation: &CancellationToken,
) -> Result<Vec<(DataRelativePath, bool)>, ErrorMarker> {
	let mut result = Vec::new();
	let mut budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
	collect_entries_inner(
		directory,
		prefix,
		cancellation,
		&mut budget,
		&mut result,
		MAX_TRAVERSAL_DEPTH,
	)?;
	Ok(result)
}

fn collect_entries_inner(
	directory: &SafeDir,
	prefix: &str,
	cancellation: &CancellationToken,
	budget: &mut EntryBudget,
	result: &mut Vec<(DataRelativePath, bool)>,
	remaining_depth: usize,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
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
		let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
		budget.consume(&mut directory_entries)
			.context(ErrorMarker::environment_invalid(None))?;
		if remaining_depth == 0 {
			return Err(report!(io::Error::new(
				io::ErrorKind::InvalidData,
				"directory traversal depth limit exceeded",
			))
			.context(ErrorMarker::environment_invalid(None)));
		}
		let name = entry.file_name();
		let text = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if prefix.is_empty() && text == "meta.toml" {
			continue;
		}
		let relative = if prefix.is_empty() {
			text.to_owned()
		} else {
			format!("{prefix}/{text}")
		};
		let path = DataRelativePath::new(relative.clone()).context(ErrorMarker::environment_invalid(None))?;
		let metadata = directory.symlink_metadata(&name);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let metadata = metadata.context(ErrorMarker::environment_invalid(None))?;
		if metadata.is_dir() {
			result.push((path, true));
			let child = directory.open_dir(&name);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			collect_entries_inner(
				&child.context(ErrorMarker::environment_invalid(None))?,
				&relative,
				cancellation,
				budget,
				result,
				remaining_depth - 1,
			)?;
		} else if metadata.is_file() {
			let opened = directory.open_regular(&name);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			opened.context(ErrorMarker::environment_invalid(None))?;
			result.push((path, false));
		} else {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

fn file_dependencies(
	profile: &SafeDir,
	winners: &HashMap<DataRelativePath, EffectiveResult>,
	cancellation: &CancellationToken,
) -> Result<HashMap<String, FileDependencyFact>, ErrorMarker> {
	let activation = ProfileActivation::load(profile, cancellation)?;
	let mut dependencies = HashMap::new();
	for (path, result) in winners {
		if !matches!(result, EffectiveResult::File(_)) {
			continue;
		}
		let kind = dependency_kind(path);
		let state = if kind == FileDependencyKind::Plugin && !plugin_is_active(path, winners, &activation)? {
			FileDependencyState::Inactive
		} else {
			FileDependencyState::Active
		};
		dependencies.insert(path.comparison_key().to_owned(), FileDependencyFact { kind, state });
	}
	Ok(dependencies)
}

fn plugin_is_active(
	path: &DataRelativePath,
	winners: &HashMap<DataRelativePath, EffectiveResult>,
	activation: &ProfileActivation,
) -> Result<bool, ErrorMarker> {
	if path.components().count() != 1 || !is_activatable_plugin_name(path.as_str()) {
		return Ok(false);
	}
	if activation.is_active(path) {
		return Ok(true);
	}
	let Some((stem, _)) = path.as_str().rsplit_once('.') else {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	};
	let nam = DataRelativePath::new(format!("{stem}.nam")).context(ErrorMarker::environment_invalid(None))?;
	Ok(matches!(winners.get(&nam), Some(EffectiveResult::File(_))))
}

fn dependency_kind(path: &DataRelativePath) -> FileDependencyKind {
	if path.components().count() == 1 && is_activatable_plugin_name(path.as_str()) {
		FileDependencyKind::Plugin
	} else {
		FileDependencyKind::OrdinaryDataFile
	}
}

#[derive(Default)]
struct AssessmentTraversal {
	files: HashMap<String, Vec<ProviderReference>>,
	tombstones: TombstoneIndex,
}

pub(crate) fn assess_installation(
	root_path: &Path,
	plan: &InstallPlan,
	cancellation: &CancellationToken,
) -> Result<InstallationAssessment, ErrorMarker> {
	let current = load(root_path, InstallationStateAccess::Preview, cancellation)?;
	let root = SafeDir::open_absolute(root_path).context(ErrorMarker::environment_invalid(None))?;
	let mods = root.open_dir("mods").context(ErrorMarker::environment_invalid(None))?;
	let mut traversal = AssessmentTraversal::default();

	let game = SafeDir::open_absolute(manifest_game_binding(&root, cancellation)?.game_directory().as_path())
		.context(ErrorMarker::environment_invalid(None))?;
	match game.open_dir("Data") {
		Ok(data) => {
			add_assessment_provider(
				&data,
				ProviderClass::SteamData,
				None,
				None,
				&mut traversal,
				cancellation,
			)?;
		}
		Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(error.context(ErrorMarker::environment_invalid(None))),
	}

	let mut proposal_added = false;
	for installed in &current.installed_mods {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if plan.replacement && installed.name == plan.mod_name {
			add_proposed_provider(plan, &mut traversal, cancellation)?;
			proposal_added = true;
			continue;
		}
		if !installed.enabled {
			continue;
		}
		let directory = mods
			.open_dir(installed.name.as_str())
			.context(ErrorMarker::environment_invalid(None))?;
		add_assessment_provider(
			&directory,
			ProviderClass::DataMod,
			Some(installed.name.clone()),
			Some(installed.priority),
			&mut traversal,
			cancellation,
		)?;
	}
	if !proposal_added {
		add_proposed_provider(plan, &mut traversal, cancellation)?;
	}

	let overwrite = root
		.open_dir("overwrite")
		.context(ErrorMarker::environment_invalid(None))?;
	add_assessment_provider(
		&overwrite,
		ProviderClass::Overwrite,
		None,
		None,
		&mut traversal,
		cancellation,
	)?;

	let AssessmentTraversal { files, tombstones } = traversal;

	let mut selected_paths = Vec::new();
	for candidate in &plan.candidates {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if matches!(candidate.decision, CandidateDecision::Winner { .. }) {
			selected_paths.push(candidate.candidate.destination.clone());
		}
	}

	let mut overlaps = Vec::new();
	for path in selected_paths {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let key = path.comparison_key();
		let contenders = files.get(key).map_or(&[][..], Vec::as_slice);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let controlling_tombstone = tombstones.controlling(key);

		let proposed_hypothetical = ProviderReference::DataMod {
			mod_name: plan.mod_name.clone(),
			priority: plan.projected_state.priority,
			original_path: path.clone(),
			participation_reason: ParticipationReason::HypotheticalEnabledMod,
		};
		let proposed_provider = ProviderReference::DataMod {
			mod_name: plan.mod_name.clone(),
			priority: plan.projected_state.priority,
			original_path: path.clone(),
			participation_reason: if plan.projected_state.enabled {
				ParticipationReason::HypotheticalEnabledMod
			} else {
				ParticipationReason::ProjectedDisabledMod
			},
		};
		let mut highest_file = None;
		let mut overlapping_physical_files = Vec::new();
		for provider in contenders {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if highest_file.is_none_or(|current: &ProviderReference| current.rank() < provider.rank()) {
				highest_file = Some(provider);
			}
			if *provider != proposed_hypothetical {
				overlapping_physical_files.push(provider.clone());
			}
		}

		let hypothetical_enabled = resolve_effective_file(highest_file, controlling_tombstone);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let before = current
			.current_winners
			.get(&path)
			.cloned()
			.unwrap_or(EffectiveResult::Absent {
				controlling_tombstone: None,
			});
		if overlapping_physical_files.is_empty() {
			continue;
		}
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		overlaps.push(InstallOverlap {
			path: path.clone(),
			proposed_provider,
			overlapping_physical_files,
			before: before.clone(),
			after_operation: if plan.projected_state.enabled {
				hypothetical_enabled.clone()
			} else {
				before
			},
			hypothetical_enabled: hypothetical_enabled.clone(),
		});
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	overlaps.sort_by(|left, right| left.path.comparison_key().cmp(right.path.comparison_key()));
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	Ok(InstallationAssessment { overlaps })
}

fn add_assessment_provider(
	directory: &SafeDir,
	class: ProviderClass,
	mod_name: Option<ModName>,
	priority: Option<ModPriority>,
	traversal: &mut AssessmentTraversal,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	for (path, is_directory) in collect_entries(directory, "", cancellation)? {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if !is_directory {
			traversal
				.files
				.entry(path.comparison_key().to_owned())
				.or_default()
				.push(provider_reference(class, mod_name.clone(), priority, path)?);
		}
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	if directory
		.exists("meta.toml")
		.context(ErrorMarker::environment_invalid(None))?
	{
		for (path, directory_scope) in validate_metadata(directory, cancellation)? {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			traversal.tombstones.insert(TombstoneReference {
				owner: provider_reference(class, mod_name.clone(), priority, path)?,
				scope: if directory_scope {
					TombstoneScope::DirectorySubtree
				} else {
					TombstoneScope::ExactFile
				},
			});
		}
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	Ok(())
}

fn add_proposed_provider(
	plan: &InstallPlan,
	traversal: &mut AssessmentTraversal,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	for candidate in plan
		.candidates
		.iter()
		.filter(|candidate| matches!(candidate.decision, CandidateDecision::Winner { .. }))
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let path = candidate.candidate.destination.clone();
		traversal
			.files
			.entry(path.comparison_key().to_owned())
			.or_default()
			.push(ProviderReference::DataMod {
				mod_name: plan.mod_name.clone(),
				priority: plan.projected_state.priority,
				original_path: path,
				participation_reason: ParticipationReason::HypotheticalEnabledMod,
			});
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	Ok(())
}

pub(crate) fn visible_plugins(
	root: &SafeDir,
	replacement: Option<(&ModName, &SafeDir)>,
	cancellation: &CancellationToken,
) -> Result<HashMap<String, String>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let profile = root.open_dir("profile");
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let profile = profile.context(ErrorMarker::environment_invalid(None))?;
	let modlist = parse_modlist(&read_bounded(
		&profile,
		"modlist.txt",
		MAX_PROFILE_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)?)?;
	let mods = root.open_dir("mods").context(ErrorMarker::environment_invalid(None))?;
	let mut visible = HashMap::new();
	let mut budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
	let game = SafeDir::open_absolute(manifest_game_binding(root, cancellation)?.game_directory().as_path())
		.context(ErrorMarker::environment_invalid(None))?;
	match game.open_dir("Data") {
		Ok(data) => add_root_plugins(&data, &mut visible, cancellation, &mut budget)?,
		Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {}
		Err(error) => return Err(error.context(ErrorMarker::environment_invalid(None))),
	}
	for installed in modlist.into_iter().filter(|installed| installed.enabled) {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if let Some((name, staged)) = replacement
			&& *name == installed.name
		{
			add_root_plugins(staged, &mut visible, cancellation, &mut budget)?;
		} else {
			let directory = mods.open_dir(installed.name.as_str());
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			add_root_plugins(
				&directory.context(ErrorMarker::environment_invalid(None))?,
				&mut visible,
				cancellation,
				&mut budget,
			)?;
		}
	}
	let overwrite = root.open_dir("overwrite");
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	add_root_plugins(
		&overwrite.context(ErrorMarker::environment_invalid(None))?,
		&mut visible,
		cancellation,
		&mut budget,
	)?;
	Ok(visible)
}

fn add_root_plugins(
	directory: &SafeDir,
	visible: &mut HashMap<String, String>,
	cancellation: &CancellationToken,
	budget: &mut EntryBudget,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
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
		let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
		budget.consume(&mut directory_entries)
			.context(ErrorMarker::environment_invalid(None))?;
		let name = entry.file_name();
		let text = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if is_activatable_plugin_name(text) {
			let opened = directory.open_regular(&name);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			opened.context(ErrorMarker::environment_invalid(None))?;
			visible.insert(case_fold_key(text), text.to_owned());
		}
	}
	if directory
		.exists("meta.toml")
		.context(ErrorMarker::environment_invalid(None))?
	{
		for (path, _) in validate_metadata(directory, cancellation)? {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if path.components().count() == 1 && is_activatable_plugin_name(path.as_str()) {
				visible.remove(path.comparison_key());
			}
		}
	}
	Ok(())
}

pub(crate) fn parse_modlist(bytes: &[u8]) -> Result<Vec<InstalledMod>, ErrorMarker> {
	let text = from_utf8(bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes))
		.context(ErrorMarker::environment_invalid(None))?;
	if text.contains('\r') && text.replace("\r\n", "").contains('\r') {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let mut entries = Vec::new();
	let mut seen = HashSet::new();
	for line in text.split(['\r', '\n']).filter(|line| !line.is_empty()) {
		if line.starts_with('#') {
			continue;
		}
		let (enabled, name) = match line.as_bytes().first() {
			Some(b'+') => (true, &line[1..]),
			Some(b'-') => (false, &line[1..]),
			_ => return Err(report!(ErrorMarker::environment_invalid(None))),
		};
		if name.trim() != name {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let name = ModName::new(name.to_owned()).context(ErrorMarker::environment_invalid(None))?;
		if !seen.insert(name.comparison_key().to_owned()) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let priority = u32::try_from(entries.len()).context(ErrorMarker::environment_invalid(None))?;
		entries.push(InstalledMod {
			name,
			priority: ModPriority::new(priority),
			enabled,
		});
	}
	Ok(entries)
}

pub(crate) fn append_disabled_mod(bytes: &[u8], name: &ModName) -> Result<Vec<u8>, ErrorMarker> {
	parse_modlist(bytes)?;
	let bom_length = usize::from(bytes.starts_with(UTF8_BOM)) * UTF8_BOM.len();
	let body = &bytes[bom_length..];
	let separator = last_separator(body).unwrap_or(b"\r\n");
	let mut result = bytes.to_vec();
	if !body.is_empty() && !body.ends_with(b"\n") {
		result.extend_from_slice(separator);
	}
	result.push(b'-');
	result.extend_from_slice(name.as_str().as_bytes());
	result.extend_from_slice(separator);
	Ok(result)
}

fn last_separator(bytes: &[u8]) -> Option<&'static [u8]> {
	for index in (0..bytes.len()).rev() {
		if bytes[index] == b'\n' {
			return if index > 0 && bytes[index - 1] == b'\r' {
				Some(b"\r\n")
			} else {
				Some(b"\n")
			};
		}
	}
	None
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
mod tests {
	use super::MAX_MODS;
	use super::MAX_PROVIDER_ENTRIES;
	use super::MAX_PROVIDER_METADATA_BYTES;
	use super::ProviderKind;
	use super::assess_installation;
	use super::collect_entries_inner;
	use super::load;
	use super::validate_provider;
	use super::validate_provider_directory;
	use super::visible_plugins;
	use crate::EnvironmentAdapter;
	use crate::profile::PROFILE_FILES;
	use crate::safe_fs::EntryBudget;
	use crate::safe_fs::MAX_TRAVERSAL_DEPTH;
	use crate::safe_fs::SafeDir;
	use application::ErrorCode;
	use application::installation::CandidateDecision;
	use application::installation::EffectiveResult;
	use application::installation::FileDependencyFact;
	use application::installation::FileDependencyKind;
	use application::installation::InstallMode;
	use application::installation::InstallPlan;
	use application::installation::PlanCandidateReference;
	use application::installation::PlannedCandidate;
	use application::installation::ProjectedModState;
	use application::installation::TombstoneReference;
	use application::installation::TombstoneScope;
	use application::installation::WinnerReason;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::InstallationStateAccess;
	use application::ports::ProfileSource;
	use domain::ArchiveIdentity;
	use domain::DataRelativePath;
	use domain::EnvironmentRoot;
	use domain::FileDependencyState;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::InstallCandidate;
	use domain::InstallCandidateOrigin;
	use domain::InstallationPhase;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ParticipationReason;
	use domain::ProviderReference;
	use domain::Sha256Digest;
	use domain::SteamBuildId;
	use std::collections::HashSet;
	use std::env::current_dir;
	use std::error::Error;
	use std::fs;
	use std::io;
	use std::path::Path;
	use std::result::Result as StdResult;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn snapshot_resource_caps_are_deliberate() {
		assert_eq!(MAX_MODS, 4096);
		assert_eq!(MAX_PROVIDER_ENTRIES, 100_000);
		assert_eq!(MAX_PROVIDER_METADATA_BYTES, 1024 * 1024);
		assert_eq!(MAX_TRAVERSAL_DEPTH, 64);
	}

	#[test]
	fn canonical_tree_collection_rejects_total_cap_with_io_cause() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("one.dds"), b"one")?;
		fs::write(temp.path().join("two.dds"), b"two")?;
		let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe provider directory must open")?;
		let mut budget = EntryBudget::new(1);
		let mut result: Vec<(DataRelativePath, bool)> = Vec::new();
		let capped = collect_entries_inner(
			&directory,
			"",
			&CancellationToken::new(),
			&mut budget,
			&mut result,
			MAX_TRAVERSAL_DEPTH,
		)
		.expect_err("entry cap must reject canonical tree");
		assert!(capped.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
		}));
		Ok(())
	}

	#[test]
	fn provider_collection_and_validation_reject_exhausted_depth_with_io_causes() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("entry.dds"), b"contents")?;
		let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe provider directory must open")?;

		let mut collection_budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
		let mut result = Vec::new();
		let collection_error = collect_entries_inner(
			&directory,
			"",
			&CancellationToken::new(),
			&mut collection_budget,
			&mut result,
			0,
		)
		.expect_err("exhausted depth must reject provider collection");
		assert!(collection_error.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
		}));

		let mut validation_budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
		let mut paths = HashSet::new();
		let validation_error = validate_provider_directory(
			&directory,
			&directory,
			ProviderKind::Overwrite,
			"",
			&mut paths,
			&CancellationToken::new(),
			&mut validation_budget,
			0,
		)
		.expect_err("exhausted depth must reject provider validation");
		assert!(validation_error.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
		}));
		Ok(())
	}

	#[test]
	fn provider_metadata_rejects_reserved_root_tombstones_for_both_scopes() -> StdResult<(), Box<dyn Error>> {
		for (scope, path) in [
			("files", "meta.toml"),
			("directories", "META.TOML"),
			("files", "Fallout - Invalidation.bsa"),
			("directories", "Fallout - Invalidation.bſa"),
		] {
			let temp = TempDir::new()?;
			fs::write(
				temp.path().join("meta.toml"),
				format!("schema_version = 1\n[tombstones]\n{scope} = [\"{path}\"]\n"),
			)?;
			let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
				.map_err(|_| "safe provider directory must open")?;

			let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
				.expect_err("reserved tombstone path must be rejected");

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		}
		Ok(())
	}

	#[test]
	fn provider_validation_rejects_case_folded_reserved_root_entries() -> StdResult<(), Box<dyn Error>> {
		for name in ["META.TOML", "Fallout - Invalidation.bſa"] {
			let temp = TempDir::new()?;
			fs::write(temp.path().join(name), b"reserved")?;
			let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
				.map_err(|_| "safe provider directory must open")?;

			let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
				.expect_err("reserved root entry must be rejected");

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		}
		Ok(())
	}

	#[test]
	fn data_mod_validation_allows_exact_canonical_metadata_name() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("meta.toml"), b"schema_version = 1\n")?;
		let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe provider directory must open")?;

		validate_provider(&directory, ProviderKind::DataMod, &CancellationToken::new())
			.map_err(|_| "canonical mod metadata must be allowed")?;
		Ok(())
	}

	#[test]
	fn provider_validation_rejects_paths_nested_under_directory_tombstones() -> StdResult<(), Box<dyn Error>> {
		for (physical_path, metadata) in [
			(
				Some("nested/physical.dds"),
				"schema_version = 1\n[tombstones]\ndirectories = [\"nested\"]\n",
			),
			(
				None,
				concat!(
					"schema_version = 1\n[tombstones]\n",
					"directories = [\"nested\"]\n",
					"files = [\"nested/deleted.dds\"]\n",
				),
			),
		] {
			let temp = TempDir::new()?;
			if let Some(path) = physical_path {
				fs::create_dir(temp.path().join("nested"))?;
				fs::write(temp.path().join(path), b"physical")?;
			}
			fs::write(temp.path().join("meta.toml"), metadata)?;
			let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
				.map_err(|_| "safe provider directory must open")?;

			let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
				.expect_err("nested provider path must be rejected");

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		}
		Ok(())
	}

	#[test]
	fn provider_validation_distinguishes_component_boundaries() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::create_dir(temp.path().join("foobar"))?;
		fs::write(temp.path().join("foobar/physical.dds"), b"physical")?;
		fs::write(
			temp.path().join("meta.toml"),
			concat!(
				"schema_version = 1\n[tombstones]\n",
				"directories = [\"foo\"]\n",
				"files = [\"foobar/deleted.dds\"]\n",
			),
		)?;
		let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe provider directory must open")?;

		validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
			.map_err(|_| "non-boundary prefixes must not overlap")?;
		Ok(())
	}

	#[test]
	fn provider_validation_uses_folded_unicode_for_tombstone_ancestry() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(
			temp.path().join("meta.toml"),
			concat!(
				"schema_version = 1\n[tombstones]\n",
				"directories = [\"ÉΣ\"]\n",
				"files = [\"éς/deleted.dds\"]\n",
			),
		)?;
		let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe provider directory must open")?;

		let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
			.expect_err("folded Unicode ancestor must be rejected");

		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		Ok(())
	}

	#[test]
	fn provider_validation_honors_pre_cancelled_tokens() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("meta.toml"), b"schema_version = 1\n")?;
		let directory = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe provider directory must open")?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let error = validate_provider(&directory, ProviderKind::Overwrite, &cancellation)
			.expect_err("pre-cancelled provider validation must stop");

		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		Ok(())
	}

	#[test]
	fn plugin_dependency_state_uses_effective_profile_activation() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		let data = game.join("Data");
		fs::create_dir_all(&data)?;
		for name in [
			"Listed.ESP",
			"Named.eſp",
			"Named.nAm",
			"FalloutIni.eSm",
			"Prefs.esp",
			"Custom.esm",
			"GeckCustom.esp",
			"GeckPrefs.esm",
			"FalloutNV.esm",
			"Dormant.esp",
			"Leading.esp",
			"Dependency.esl",
			"NamedEsl.esl",
			"NamedEsl.nam",
			"IniEsl.esl",
			"éς.ESP",
			"ordinary.dds",
		] {
			fs::write(data.join(name), b"provider file")?;
		}
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.expect("fixture environment must initialize");
		let safe_root = SafeDir::open_absolute(
			&root.as_path()
				.canonicalize()
				.expect("fixture environment root must canonicalize"),
		)
		.expect("fixture environment root must open");
		let visible = visible_plugins(&safe_root, None, &CancellationToken::new())
			.expect("visible plugins must load");
		assert_eq!(visible.get("éσ.esp").map(String::as_str), Some("éς.ESP"));
		let profile = root.as_path().join("profile");
		fs::write(profile.join("plugins.txt"), b"listed.esp\r\ndEpEnDeNcY.EsL\r\n")?;
		fs::write(profile.join("loadorder.txt"), b"Listed.ESP\r\nDependency.esl\r\n")?;
		let mut ini = fs::read_to_string(profile.join("Fallout.ini"))?;
		ini.push_str(concat!(
			"\r\n[gEnErAl]\r\n",
			"sTeStFiLe10 = FalloutIni.ESM\r\n",
			"sTestFile9=iNiEsL.EsL\r\n",
			"sTestFile01=Leading.esp\r\n",
		));
		fs::write(profile.join("Fallout.ini"), ini)?;
		fs::write(
			profile.join("FalloutPrefs.ini"),
			b"[General]\r\nsTestFile1=Prefs.esp\r\n",
		)?;
		fs::write(
			profile.join("FalloutCustom.ini"),
			b"[General]\r\nsTestFile1=Custom.esm\r\n",
		)?;
		fs::write(
			profile.join("GECKCustom.ini"),
			b"[General]\r\nsTestFile1=GeckCustom.esp\r\n",
		)?;
		fs::write(
			profile.join("GECKPrefs.ini"),
			b"[General]\r\nsTestFile1=GeckPrefs.esm\r\n",
		)?;

		let disabled = root.as_path().join("mods/Disabled");
		fs::create_dir(&disabled)?;
		fs::write(disabled.join("meta.toml"), b"schema_version = 1\n")?;
		fs::write(disabled.join("Only.ESP"), b"disabled plugin")?;
		fs::write(disabled.join("Dormant.nam"), b"disabled activation marker")?;
		fs::write(disabled.join("DisabledOnly.dds"), b"disabled ordinary file")?;
		fs::create_dir(disabled.join("subdir"))?;
		fs::write(disabled.join("subdir/Nested.esp"), b"disabled nested file")?;
		fs::write(profile.join("modlist.txt"), b"-Disabled\r\n")?;

		let snapshot = load(
			root.as_path(),
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		)
		.expect("environment snapshot must load");
		for plugin in [
			"listed.esp",
			"named.esp",
			"falloutini.esm",
			"prefs.esp",
			"custom.esm",
			"geckcustom.esp",
			"geckprefs.esm",
			"falloutnv.esm",
			"dependency.esl",
			"namedesl.esl",
			"iniesl.esl",
		] {
			assert_eq!(
				snapshot.file_dependencies.get(plugin),
				Some(&FileDependencyFact {
					kind: FileDependencyKind::Plugin,
					state: FileDependencyState::Active,
				})
			);
		}
		for plugin in ["dormant.esp", "leading.esp", "éσ.esp"] {
			assert_eq!(
				snapshot.file_dependencies.get(plugin),
				Some(&FileDependencyFact {
					kind: FileDependencyKind::Plugin,
					state: FileDependencyState::Inactive,
				})
			);
		}
		assert_eq!(
			snapshot.file_dependencies.get("ordinary.dds").map(|fact| fact.state),
			Some(FileDependencyState::Active)
		);
		for disabled_path in ["only.esp", "dormant.nam", "disabledonly.dds", "subdir/nested.esp"] {
			assert_eq!(snapshot.file_dependencies.get(disabled_path), None);
		}
		Ok(())
	}

	#[test]
	fn plugin_dependency_classification_requires_a_data_root_path() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		let data = game.join("Data");
		fs::create_dir_all(data.join("subdir"))?;
		fs::write(data.join("Dormant.eſp"), b"root plugin")?;
		fs::write(data.join("subdir/Nested.esp"), b"nested data file")?;
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.expect("fixture environment must initialize");

		let snapshot = load(
			root.as_path(),
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		)
		.expect("environment snapshot must load");

		assert_eq!(
			snapshot.file_dependencies.get("subdir/nested.esp"),
			Some(&FileDependencyFact {
				kind: FileDependencyKind::OrdinaryDataFile,
				state: FileDependencyState::Active,
			})
		);
		assert_eq!(
			snapshot.file_dependencies.get("dormant.esp"),
			Some(&FileDependencyFact {
				kind: FileDependencyKind::Plugin,
				state: FileDependencyState::Inactive,
			})
		);
		Ok(())
	}

	#[test]
	fn installation_assessment_reports_only_paths_with_physical_overlaps() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		let data = game.join("Data");
		fs::create_dir_all(&data)?;
		fs::write(data.join("existing.dds"), b"game file")?;
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.expect("fixture environment must initialize");

		let existing_path =
			DataRelativePath::new("existing.dds".to_owned()).expect("fixture existing path must be valid");
		let new_path = DataRelativePath::new("new.dds".to_owned()).expect("fixture new path must be valid");
		let mod_name = ModName::new("Proposed".to_owned()).expect("fixture mod name must be valid");
		let planned_candidate =
			|candidate_id, destination: DataRelativePath, current_winner| PlannedCandidate {
				origin_condition_evaluation: None,
				candidate: InstallCandidate {
					candidate_id,
					origin: InstallCandidateOrigin::Required,
					phase: InstallationPhase::Required,
					declared_priority: 0,
					descriptor_order: candidate_id,
					source_member: destination.as_str().to_owned(),
					destination: destination.clone(),
				},
				current_winner,
				proposed_winner: PlanCandidateReference {
					candidate_id,
					source_member: destination.as_str().to_owned(),
				},
				decision: CandidateDecision::Winner {
					reason: WinnerReason::OnlyCandidate,
				},
			};
		let plan = InstallPlan {
			archive_identity: ArchiveIdentity::DataArchive {
				archive_sha256: Sha256Digest::new("a".repeat(64))
					.expect("fixture digest must be valid"),
				package_root: String::new(),
			},
			mod_name: mod_name.clone(),
			replacement: false,
			accepted_choices: Vec::new(),
			automatic_events: Vec::new(),
			resolved_flags: Vec::new(),
			warnings: Vec::new(),
			candidates: vec![
				planned_candidate(
					0,
					existing_path.clone(),
					EffectiveResult::File(ProviderReference::SteamData {
						original_path: existing_path.clone(),
					}),
				),
				planned_candidate(
					1,
					new_path.clone(),
					EffectiveResult::Absent {
						controlling_tombstone: None,
					},
				),
			],
			projected_state: ProjectedModState {
				mode: InstallMode::NewInstall,
				mod_name: mod_name.clone(),
				priority: ModPriority::new(0),
				list_position: 0,
				enabled: true,
				overlaps: Vec::new(),
			},
		};

		let assessment = assess_installation(root.as_path(), &plan, &CancellationToken::new())
			.expect("installation must be assessed");

		assert!(assessment.overlaps.iter().all(|overlap| overlap.path != new_path));
		assert_eq!(assessment.overlaps.len(), 1);
		let overlap = &assessment.overlaps[0];
		let existing_provider = ProviderReference::SteamData {
			original_path: existing_path.clone(),
		};
		let proposed_provider = ProviderReference::DataMod {
			mod_name,
			priority: ModPriority::new(0),
			original_path: existing_path.clone(),
			participation_reason: ParticipationReason::HypotheticalEnabledMod,
		};
		assert_eq!(overlap.path, existing_path);
		assert_eq!(overlap.overlapping_physical_files, vec![existing_provider.clone()]);
		assert_eq!(overlap.before, EffectiveResult::File(existing_provider));
		assert_eq!(
			overlap.after_operation,
			EffectiveResult::File(proposed_provider.clone())
		);
		assert_eq!(overlap.hypothetical_enabled, EffectiveResult::File(proposed_provider));
		Ok(())
	}

	#[test]
	fn installation_assessment_preserves_physical_overlap_under_directory_tombstone()
	-> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		let data = game.join("Data");
		fs::create_dir_all(data.join("folder"))?;
		fs::write(data.join("folder/existing.dds"), b"game file")?;
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.expect("fixture environment must initialize");
		fs::write(
			root.as_path().join("overwrite/meta.toml"),
			b"schema_version = 1\n[tombstones]\ndirectories = [\"folder\"]\n",
		)?;

		let path = DataRelativePath::new("folder/existing.dds".to_owned())
			.expect("fixture candidate path must be valid");
		let mod_name = ModName::new("Proposed".to_owned()).expect("fixture mod name must be valid");
		let plan = InstallPlan {
			archive_identity: ArchiveIdentity::DataArchive {
				archive_sha256: Sha256Digest::new("a".repeat(64))
					.expect("fixture digest must be valid"),
				package_root: String::new(),
			},
			mod_name: mod_name.clone(),
			replacement: false,
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
					source_member: path.as_str().to_owned(),
					destination: path.clone(),
				},
				current_winner: EffectiveResult::File(ProviderReference::SteamData {
					original_path: path.clone(),
				}),
				proposed_winner: PlanCandidateReference {
					candidate_id: 0,
					source_member: path.as_str().to_owned(),
				},
				decision: CandidateDecision::Winner {
					reason: WinnerReason::OnlyCandidate,
				},
			}],
			projected_state: ProjectedModState {
				mode: InstallMode::NewInstall,
				mod_name,
				priority: ModPriority::new(0),
				list_position: 0,
				enabled: true,
				overlaps: Vec::new(),
			},
		};

		let assessment = assess_installation(root.as_path(), &plan, &CancellationToken::new())
			.expect("installation must be assessed");

		assert_eq!(assessment.overlaps.len(), 1);
		let overlap = &assessment.overlaps[0];
		assert_eq!(
			overlap.overlapping_physical_files,
			vec![ProviderReference::SteamData {
				original_path: path.clone(),
			}]
		);
		let expected = EffectiveResult::Absent {
			controlling_tombstone: Some(TombstoneReference {
				scope: TombstoneScope::DirectorySubtree,
				owner: ProviderReference::Overwrite {
					original_path: DataRelativePath::new("folder".to_owned())
						.expect("fixture tombstone path must be valid"),
				},
			}),
		};
		assert_eq!(overlap.before, expected);
		assert_eq!(overlap.after_operation, expected);
		assert_eq!(overlap.hypothetical_enabled, expected);
		Ok(())
	}

	fn initialization_plan(game: &Path) -> InitializationPlan {
		InitializationPlan {
			game_binding: GameBinding::new(
				GameInstallationPath::new(game.to_owned()).expect("fixture game path must be valid"),
				SteamBuildId::new(7).expect("fixture build ID must be valid"),
			),
			profile_sources: InitializationProfileSources {
				files: PROFILE_FILES
					.into_iter()
					.map(|name| ProfileSource { name, contents: None })
					.collect(),
				fallout_default_ini: b"[Archive]
sArchiveList=Fallout - Meshes.bsa
"
				.to_vec(),
			},
		}
	}

	#[test]
	fn preview_rejects_unfinished_work_as_invalid_without_mutation() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let stage = fixture.path().join("temp/operation/stage");
		fs::create_dir_all(&stage)?;
		let evidence = stage.join("partial-file");
		fs::write(&evidence, b"unfinished")?;

		let result = load(
			fixture.path(),
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		);

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read(&evidence)?, b"unfinished");
		assert_eq!(fs::read_dir(fixture.path())?.count(), 1);
		Ok(())
	}

	#[test]
	fn unfinished_work_precedes_missing_canonical_layout_without_mutation() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let stage = fixture.path().join("temp/operation/stage");
		fs::create_dir_all(&stage)?;
		let evidence = stage.join("partial-file");
		fs::write(&evidence, b"unfinished")?;

		let result = load(
			fixture.path(),
			InstallationStateAccess::Mutation,
			&CancellationToken::new(),
		);

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::ManualCleanupRequired
		));
		assert_eq!(fs::read(&evidence)?, b"unfinished");
		assert_eq!(fs::read_dir(fixture.path())?.count(), 1);
		Ok(())
	}
}
