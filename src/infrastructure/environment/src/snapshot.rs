use crate::files::read_optional;
use crate::files::validate_exact_entries;
use crate::manifest::validate_manifest;
use crate::profile::is_activatable_plugin_name;
use crate::profile::validate_profile_files;
use crate::profile_activation::ProfileActivation;
use crate::validate_bsa_file;
use application::ErrorMarker;
use application::installation::CandidateDecision;
use application::installation::EffectiveResult;
use application::installation::FileDependencyFact;
use application::installation::FileDependencyKind;
use application::installation::InstallMode;
use application::installation::InstallOverlap;
use application::installation::InstallPlan;
use application::installation::InstallationAssessment;
use application::installation::TombstoneReference;
use application::installation::TombstoneScope;
use application::ports::InstallationStateAccess;
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
#[cfg(test)]
use std::cell::Cell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::str::from_utf8;
use tokio::fs::metadata;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio::fs::try_exists;
use tokio_util::sync::CancellationToken;
use toml::Value;
use toml::from_str;

#[cfg(test)]
thread_local! {
	pub(crate) static INVENTORY_IO: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

const UTF8_BOM: &[u8] = &[0xef, 0xbb, 0xbf];
const INVALIDATION_ARCHIVE: &str = "Fallout - Invalidation.bsa";

#[derive(Debug, Clone)]
pub(crate) struct EnvironmentSnapshotData {
	pub(crate) game_binding: GameBinding,
	pub(crate) installed_mods: Vec<InstalledMod>,
	pub(crate) unlisted_mod_names: Vec<ModName>,
	pub(crate) current_winners: HashMap<DataRelativePath, EffectiveResult>,
	pub(crate) file_dependencies: HashMap<String, FileDependencyFact>,
}

pub(crate) async fn load(
	root_path: &Path,
	binding: &GameBinding,
	access: InstallationStateAccess,
	cancellation: &CancellationToken,
) -> Result<EnvironmentSnapshotData, ErrorMarker> {
	load_inner(root_path, binding, access, cancellation).await
}

async fn load_inner(
	root_path: &Path,
	binding: &GameBinding,
	access: InstallationStateAccess,
	cancellation: &CancellationToken,
) -> Result<EnvironmentSnapshotData, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	match metadata(root_path).await {
		Ok(metadata) if metadata.is_dir() => {}
		Ok(_) => return Err(report!(ErrorMarker::environment_root_unsafe())),
		Err(error) if error.kind() == io::ErrorKind::NotFound => {
			return Err(report!(error).context(ErrorMarker::environment_not_initialized()));
		}
		Err(error) => return Err(report!(error).context(ErrorMarker::environment_root_unsafe())),
	}

	let temp = root_path.join("temp");
	let temp_exists = try_exists(&temp).await;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	if temp_exists.context(ErrorMarker::environment_invalid(None))? {
		let mut entries = read_dir(&temp).await.context(ErrorMarker::environment_invalid(None))?;
		if entries
			.next_entry()
			.await
			.context(ErrorMarker::environment_invalid(None))?
			.is_some()
		{
			let marker = match access {
				InstallationStateAccess::Preview => ErrorMarker::environment_invalid(None),
				InstallationStateAccess::Mutation => ErrorMarker::manual_cleanup_required(),
			};
			return Err(report!(marker));
		}
	}

	validate_exact_entries(
		root_path,
		&["mods", "profile", "overwrite", "cache", "mods.toml", "temp", "logs"],
		cancellation,
	)
	.await?;

	validate_manifest(root_path, cancellation).await?;

	let cache = root_path.join("cache");
	validate_exact_entries(&cache, &[INVALIDATION_ARCHIVE], cancellation).await?;
	validate_bsa_file(&cache, cancellation).await?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mods = root_path.join("mods");
	let profile_dir = root_path.join("profile");
	let overwrite = root_path.join("overwrite");

	validate_profile_files(&profile_dir, false, cancellation).await?;

	let overwrite_inventory = collect_provider_inventory(&overwrite, ProviderKind::Overwrite, cancellation).await?;

	let modlist = read(profile_dir.join("modlist.txt"))
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	let installed_mods = parse_modlist(&modlist)?;

	let unlisted_entries = check_listed_mod_folders(&mods, &installed_mods, cancellation).await?;
	// An invalid mod name cannot be an install target, so it cannot collide with one.
	let unlisted_mod_names = unlisted_entries
		.into_iter()
		.filter_map(|spelling| ModName::new(spelling).ok())
		.collect();

	let mut inventories = Vec::with_capacity(installed_mods.len());
	for installed in &installed_mods {
		let directory = mods.join(installed.name.as_str());
		inventories.push(collect_provider_inventory(&directory, ProviderKind::DataMod, cancellation).await?);
	}

	let game_binding = binding.clone();
	let current_winners = current_winners(
		&installed_mods,
		inventories,
		overwrite_inventory,
		&game_binding,
		cancellation,
	)
	.await?;
	let file_dependencies = file_dependencies(&profile_dir, &current_winners, cancellation).await?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	Ok(EnvironmentSnapshotData {
		game_binding,
		installed_mods,
		unlisted_mod_names,
		current_winners,
		file_dependencies,
	})
}

/// Checks that each listed mod has a directory of exactly the listed spelling.
///
/// Other entries in `mods` are not installed mods: they contribute nothing, so their types and
/// contents are not checked. Returns their spellings. Names that are not valid UTF-8 are skipped,
/// because `modlist.txt` cannot list them.
///
/// # Errors
///
/// - `operation_cancelled` when `cancellation` is cancelled.
/// - `environment_invalid` without a mod name when `mods` cannot be read.
/// - `environment_invalid` with the mod name when a listed mod has no such directory, or when the
///   metadata of its entry cannot be read.
pub(crate) async fn check_listed_mod_folders(
	mods: &Path,
	installed: &[InstalledMod],
	cancellation: &CancellationToken,
) -> Result<Vec<String>, ErrorMarker> {
	let listed: HashMap<_, _> = installed
		.iter()
		.map(|installed| (installed.name.as_str(), &installed.name))
		.collect();
	let mut found = HashSet::new();
	let mut unlisted = Vec::new();
	let mut entries = read_dir(mods).await.context(ErrorMarker::environment_invalid(None))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let Ok(spelling) = entry.file_name().into_string() else {
			continue;
		};
		let Some(name) = listed.get(spelling.as_str()) else {
			unlisted.push(spelling);
			continue;
		};

		let is_directory = metadata(entry.path())
			.await
			.context(ErrorMarker::environment_invalid(None).with_mod_name((*name).clone()))?
			.is_dir();
		if is_directory {
			found.insert(spelling);
		}
	}

	if let Some(missing) = installed
		.iter()
		.find(|installed| !found.contains(installed.name.as_str()))
	{
		return Err(report!(
			ErrorMarker::environment_invalid(None).with_mod_name(missing.name.clone())
		));
	}
	Ok(unlisted)
}

#[derive(Default)]
struct ProviderInventory {
	entries: Vec<(DataRelativePath, bool)>,
	tombstones: Vec<(DataRelativePath, bool)>,
}

pub(crate) struct ProviderMetadata {
	pub(crate) tombstones: Vec<(DataRelativePath, bool)>,
}

#[derive(Clone, Copy)]
enum ProviderKind {
	GameBase,
	DataMod,
	Overwrite,
}

pub(crate) async fn validate_staged_provider(
	provider: &Path,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	validate_provider(provider, ProviderKind::DataMod, cancellation).await
}

async fn validate_provider(
	provider: &Path,
	kind: ProviderKind,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	collect_provider_inventory(provider, kind, cancellation).await?;
	Ok(())
}

async fn collect_provider_inventory(
	provider: &Path,
	kind: ProviderKind,
	cancellation: &CancellationToken,
) -> Result<ProviderInventory, ErrorMarker> {
	let mut inventory = ProviderInventory::default();
	let mut paths = HashSet::new();
	validate_provider_directory(provider, kind, "", &mut paths, &mut inventory, cancellation).await?;
	let Some(ProviderMetadata { tombstones }) = read_metadata(provider).await? else {
		return Ok(inventory);
	};

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

	inventory.tombstones = tombstones;
	Ok(inventory)
}

async fn validate_provider_directory(
	directory: &Path,
	kind: ProviderKind,
	prefix: &str,
	paths: &mut HashSet<String>,
	inventory: &mut ProviderInventory,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	#[cfg(test)]
	INVENTORY_IO.with(|count| {
		let (walks, metadata) = count.get();
		count.set((walks + 1, metadata));
	});
	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	let mut local_names = HashSet::new();
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let file_name = entry.file_name();
		let spelling = file_name
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

		let metadata = metadata(entry.path())
			.await
			.context(ErrorMarker::environment_invalid(None))?;
		if metadata.is_dir() {
			inventory.entries.push((data_path, true));
			Box::pin(validate_provider_directory(
				&entry.path(),
				kind,
				&relative,
				paths,
				inventory,
				cancellation,
			))
			.await?;
			continue;
		}
		if !metadata.is_file() {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
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
		if prefix.is_empty() && spelling == "meta.toml" {
			continue;
		}

		inventory.entries.push((data_path, false));
	}
	Ok(())
}

/// Reads provider metadata, or returns `None` when the provider has no `meta.toml`.
async fn read_metadata(provider: &Path) -> Result<Option<ProviderMetadata>, ErrorMarker> {
	let bytes = read_optional(&provider.join("meta.toml"))
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	#[cfg(test)]
	if bytes.is_some() {
		INVENTORY_IO.with(|count| {
			let (walks, metadata) = count.get();
			count.set((walks, metadata + 1));
		});
	}
	bytes.map(parse_metadata).transpose()
}

pub(crate) fn parse_metadata(bytes: Vec<u8>) -> Result<ProviderMetadata, ErrorMarker> {
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
		return Ok(ProviderMetadata { tombstones: result });
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
	Ok(ProviderMetadata { tombstones: result })
}

pub(crate) async fn validate_prospective_namespace(
	root: &Path,
	binding: &GameBinding,
	staged_mod: &Path,
	plan: &InstallPlan,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let modlist = read(root.join("profile/modlist.txt"))
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	let installed = parse_modlist(&modlist)?;
	let mods = root.join("mods");
	let mut namespace = HashMap::new();
	let mut winners = HashMap::new();
	if let Some(data) = game_data(binding).await? {
		validate_provider(&data, ProviderKind::GameBase, cancellation).await?;
		apply_provider(
			&data,
			ProviderClass::SteamData,
			None,
			None,
			&mut namespace,
			&mut winners,
			cancellation,
		)
		.await?;
	}
	for installed_mod in installed.iter().filter(|installed_mod| installed_mod.enabled) {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		if plan.projected_state.mode == InstallMode::Replacement && installed_mod.name == plan.mod_name {
			continue;
		}
		apply_provider(
			&mods.join(installed_mod.name.as_str()),
			ProviderClass::DataMod,
			Some(installed_mod.name.clone()),
			Some(installed_mod.priority),
			&mut namespace,
			&mut winners,
			cancellation,
		)
		.await?;
	}
	apply_provider(
		staged_mod,
		ProviderClass::DataMod,
		Some(plan.mod_name.clone()),
		Some(plan.projected_state.priority),
		&mut namespace,
		&mut winners,
		cancellation,
	)
	.await?;
	apply_provider(
		&root.join("overwrite"),
		ProviderClass::Overwrite,
		None,
		None,
		&mut namespace,
		&mut winners,
		cancellation,
	)
	.await?;
	Ok(())
}

/// Returns the game `Data` directory, or `None` when the game has none.
async fn game_data(binding: &GameBinding) -> Result<Option<PathBuf>, ErrorMarker> {
	let data = binding.game_directory().as_path().join("Data");
	match metadata(&data).await {
		Ok(metadata) if metadata.is_dir() => Ok(Some(data)),
		Ok(_) => Err(report!(ErrorMarker::environment_invalid(None))),
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(report!(error).context(ErrorMarker::environment_invalid(None))),
	}
}

async fn current_winners(
	installed: &[InstalledMod],
	inventories: Vec<ProviderInventory>,
	overwrite: ProviderInventory,
	binding: &GameBinding,
	cancellation: &CancellationToken,
) -> Result<HashMap<DataRelativePath, EffectiveResult>, ErrorMarker> {
	let mut namespace = HashMap::new();
	let mut winners = HashMap::new();
	if let Some(data) = game_data(binding).await? {
		let inventory = collect_provider_inventory(&data, ProviderKind::GameBase, cancellation).await?;
		apply_inventory(
			inventory,
			ProviderClass::SteamData,
			None,
			None,
			&mut namespace,
			&mut winners,
			cancellation,
		)?;
	}
	for (installed_mod, inventory) in installed.iter().zip(inventories) {
		if !installed_mod.enabled {
			continue;
		}
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		apply_inventory(
			inventory,
			ProviderClass::DataMod,
			Some(installed_mod.name.clone()),
			Some(installed_mod.priority),
			&mut namespace,
			&mut winners,
			cancellation,
		)?;
	}
	apply_inventory(
		overwrite,
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

async fn apply_provider(
	directory: &Path,
	class: ProviderClass,
	mod_name: Option<ModName>,
	priority: Option<ModPriority>,
	namespace: &mut HashMap<String, bool>,
	winners: &mut HashMap<String, (DataRelativePath, EffectiveResult)>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let entries = collect_entries(directory, cancellation).await?;
	let tombstones = read_metadata(directory)
		.await?
		.map(|metadata| metadata.tombstones)
		.unwrap_or_default();

	apply_inventory(
		ProviderInventory { entries, tombstones },
		class,
		mod_name,
		priority,
		namespace,
		winners,
		cancellation,
	)
}

fn apply_inventory(
	inventory: ProviderInventory,
	class: ProviderClass,
	mod_name: Option<ModName>,
	priority: Option<ModPriority>,
	namespace: &mut HashMap<String, bool>,
	winners: &mut HashMap<String, (DataRelativePath, EffectiveResult)>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	for (path, is_directory) in inventory.entries {
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
	if inventory.tombstones.is_empty() {
		return Ok(());
	}

	let mut path_tombstones = HashMap::new();
	let mut tombstone_index = TombstoneIndex::default();
	for (path, directory_scope) in inventory.tombstones {
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
		let mut controlling_tombstone = None;
		for controlling in tombstone_index.controlling_steps(key) {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			controlling_tombstone = controlling;
		}
		if let Some(tombstone) = controlling_tombstone {
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

async fn collect_entries(
	directory: &Path,
	cancellation: &CancellationToken,
) -> Result<Vec<(DataRelativePath, bool)>, ErrorMarker> {
	let mut result = Vec::new();
	collect_entries_inner(directory, "", cancellation, &mut result).await?;
	Ok(result)
}

async fn collect_entries_inner(
	directory: &Path,
	prefix: &str,
	cancellation: &CancellationToken,
	result: &mut Vec<(DataRelativePath, bool)>,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	#[cfg(test)]
	INVENTORY_IO.with(|count| {
		let (walks, metadata) = count.get();
		count.set((walks + 1, metadata));
	});
	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
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
		let metadata = metadata(entry.path())
			.await
			.context(ErrorMarker::environment_invalid(None))?;
		if metadata.is_dir() {
			result.push((path, true));
			Box::pin(collect_entries_inner(&entry.path(), &relative, cancellation, result)).await?;
		} else if metadata.is_file() {
			result.push((path, false));
		} else {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

async fn file_dependencies(
	profile: &Path,
	winners: &HashMap<DataRelativePath, EffectiveResult>,
	cancellation: &CancellationToken,
) -> Result<HashMap<String, FileDependencyFact>, ErrorMarker> {
	let activation = ProfileActivation::load(profile, cancellation).await?;
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

pub(crate) async fn assess_installation(
	root_path: &Path,
	binding: &GameBinding,
	plan: &InstallPlan,
	cancellation: &CancellationToken,
) -> Result<InstallationAssessment, ErrorMarker> {
	let current = load(root_path, binding, InstallationStateAccess::Preview, cancellation).await?;
	let mods = root_path.join("mods");
	let mut traversal = AssessmentTraversal::default();

	if let Some(data) = game_data(binding).await? {
		add_assessment_provider(
			&data,
			ProviderClass::SteamData,
			None,
			None,
			&mut traversal,
			cancellation,
		)
		.await?;
	}

	let mut proposal_added = false;
	for installed in &current.installed_mods {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if plan.projected_state.mode == InstallMode::Replacement && installed.name == plan.mod_name {
			add_proposed_provider(plan, &mut traversal, cancellation)?;
			proposal_added = true;
			continue;
		}
		if !installed.enabled {
			continue;
		}
		add_assessment_provider(
			&mods.join(installed.name.as_str()),
			ProviderClass::DataMod,
			Some(installed.name.clone()),
			Some(installed.priority),
			&mut traversal,
			cancellation,
		)
		.await?;
	}
	if !proposal_added {
		add_proposed_provider(plan, &mut traversal, cancellation)?;
	}

	add_assessment_provider(
		&root_path.join("overwrite"),
		ProviderClass::Overwrite,
		None,
		None,
		&mut traversal,
		cancellation,
	)
	.await?;

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

		let mut controlling_tombstone = None;
		for controlling in tombstones.controlling_steps(key) {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			controlling_tombstone = controlling;
		}

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

async fn add_assessment_provider(
	directory: &Path,
	class: ProviderClass,
	mod_name: Option<ModName>,
	priority: Option<ModPriority>,
	traversal: &mut AssessmentTraversal,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	for (path, is_directory) in collect_entries(directory, cancellation).await? {
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
	let tombstones = read_metadata(directory)
		.await?
		.map(|metadata| metadata.tombstones)
		.unwrap_or_default();
	for (path, directory_scope) in tombstones {
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

pub(crate) async fn visible_plugins(
	root: &Path,
	binding: &GameBinding,
	cancellation: &CancellationToken,
) -> Result<HashMap<String, String>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let modlist = read(root.join("profile/modlist.txt"))
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	let modlist = parse_modlist(&modlist)?;
	let mods = root.join("mods");
	let mut visible = HashMap::new();
	if let Some(data) = game_data(binding).await? {
		add_root_plugins(&data, &mut visible, cancellation).await?;
	}
	for installed in modlist.into_iter().filter(|installed| installed.enabled) {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		add_root_plugins(&mods.join(installed.name.as_str()), &mut visible, cancellation).await?;
	}
	add_root_plugins(&root.join("overwrite"), &mut visible, cancellation).await?;
	Ok(visible)
}

async fn add_root_plugins(
	directory: &Path,
	visible: &mut HashMap<String, String>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let name = entry.file_name();
		let text = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if is_activatable_plugin_name(text) {
			if !metadata(entry.path())
				.await
				.context(ErrorMarker::environment_invalid(None))?
				.is_file()
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			visible.insert(case_fold_key(text), text.to_owned());
		}
	}

	let tombstones = read_metadata(directory)
		.await?
		.map(|metadata| metadata.tombstones)
		.unwrap_or_default();
	for (path, _) in tombstones {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if path.components().count() == 1 && is_activatable_plugin_name(path.as_str()) {
			visible.remove(path.comparison_key());
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
		entries.push((name, enabled));
	}

	// MO2 lists the highest Mod Priority first. Return mods from lowest to highest.
	entries.into_iter()
		.rev()
		.enumerate()
		.map(|(priority, (name, enabled))| {
			let priority = u32::try_from(priority).context(ErrorMarker::environment_invalid(None))?;
			Ok(InstalledMod {
				name,
				priority: ModPriority::new(priority),
				enabled,
			})
		})
		.collect()
}

/// Inserts a new disabled mod at the highest Mod Priority.
///
/// MO2 lists the highest priority first, so the entry goes before the first
/// non-comment line. Leading `#` comments, the BOM, and existing separators stay.
pub(crate) fn insert_disabled_mod(bytes: &[u8], name: &ModName) -> Result<Vec<u8>, ErrorMarker> {
	parse_modlist(bytes)?;
	let bom_length = usize::from(bytes.starts_with(UTF8_BOM)) * UTF8_BOM.len();
	let body = &bytes[bom_length..];
	let separator = last_separator(body).unwrap_or(b"\r\n");

	let mut insertion = bom_length;
	for line in body.split_inclusive(|byte| *byte == b'\n') {
		if !line.starts_with(b"#") {
			break;
		}
		insertion += line.len();
	}

	let mut result = bytes[..insertion].to_vec();
	if insertion > bom_length && !result.ends_with(b"\n") {
		result.extend_from_slice(separator);
	}
	result.push(b'-');
	result.extend_from_slice(name.as_str().as_bytes());
	result.extend_from_slice(separator);
	result.extend_from_slice(&bytes[insertion..]);
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
	use super::ProviderKind;
	use super::assess_installation;
	use super::insert_disabled_mod;
	use super::load;
	use super::parse_modlist;
	use super::validate_provider;
	use super::visible_plugins;
	use crate::EnvironmentAdapter;
	use crate::profile::PROFILE_FILES;
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
	use std::env::current_dir;
	use std::error::Error;
	use std::fs;
	use std::path::Path;
	use std::result::Result as StdResult;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn new_mods_are_inserted_at_the_top_after_leading_comments() {
		let name = ModName::new("New".to_owned()).expect("mod name must be valid");
		for (current, expected) in [
			(&b""[..], &b"-New\r\n"[..]),
			(
				b"\xef\xbb\xbf# header\n+High\n+Base\n",
				b"\xef\xbb\xbf# header\n-New\n+High\n+Base\n",
			),
			(b"# one\r\n# two\r\n+Base", b"# one\r\n# two\r\n-New\r\n+Base"),
			(b"# only", b"# only\r\n-New\r\n"),
			(b"+Base\r\n", b"-New\r\n+Base\r\n"),
		] {
			let updated = insert_disabled_mod(current, &name).expect("modlist must update");

			assert_eq!(updated, expected);
			let installed = parse_modlist(&updated).expect("updated modlist must parse");
			assert_eq!(installed.last().map(|installed| installed.name.as_str()), Some("New"));
		}
	}

	#[tokio::test]
	async fn provider_metadata_rejects_reserved_root_tombstones_for_both_scopes() -> StdResult<(), Box<dyn Error>> {
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
			let directory = temp.path().canonicalize()?;

			let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
				.await
				.expect_err("reserved tombstone path must be rejected");

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		}
		Ok(())
	}

	#[tokio::test]
	async fn provider_validation_rejects_case_folded_reserved_root_entries() -> StdResult<(), Box<dyn Error>> {
		for name in ["META.TOML", "Fallout - Invalidation.bſa"] {
			let temp = TempDir::new()?;
			fs::write(temp.path().join(name), b"reserved")?;
			let directory = temp.path().canonicalize()?;

			let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
				.await
				.expect_err("reserved root entry must be rejected");

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		}
		Ok(())
	}

	#[tokio::test]
	async fn data_mod_validation_accepts_missing_metadata() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("ordinary.dds"), b"content")?;
		let directory = temp.path().canonicalize()?;

		validate_provider(&directory, ProviderKind::DataMod, &CancellationToken::new())
			.await
			.map_err(|_| "missing metadata must mean empty tombstones")?;
		Ok(())
	}

	#[tokio::test]
	async fn data_mod_validation_allows_exact_canonical_metadata_name() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("meta.toml"), b"schema_version = 1\n")?;
		let directory = temp.path().canonicalize()?;

		validate_provider(&directory, ProviderKind::DataMod, &CancellationToken::new())
			.await
			.map_err(|_| "canonical mod metadata must be allowed")?;
		Ok(())
	}

	#[tokio::test]
	async fn provider_validation_rejects_paths_nested_under_directory_tombstones() -> StdResult<(), Box<dyn Error>>
	{
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
			let directory = temp.path().canonicalize()?;

			let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
				.await
				.expect_err("nested provider path must be rejected");

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		}
		Ok(())
	}

	#[tokio::test]
	async fn provider_validation_distinguishes_component_boundaries() -> StdResult<(), Box<dyn Error>> {
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
		let directory = temp.path().canonicalize()?;

		validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
			.await
			.map_err(|_| "non-boundary prefixes must not overlap")?;
		Ok(())
	}

	#[tokio::test]
	async fn provider_validation_uses_folded_unicode_for_tombstone_ancestry() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(
			temp.path().join("meta.toml"),
			concat!(
				"schema_version = 1\n[tombstones]\n",
				"directories = [\"ÉΣ\"]\n",
				"files = [\"éς/deleted.dds\"]\n",
			),
		)?;
		let directory = temp.path().canonicalize()?;

		let error = validate_provider(&directory, ProviderKind::Overwrite, &CancellationToken::new())
			.await
			.expect_err("folded Unicode ancestor must be rejected");

		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		Ok(())
	}

	#[tokio::test]
	async fn provider_validation_honors_pre_cancelled_tokens() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("meta.toml"), b"schema_version = 1\n")?;
		let directory = temp.path().canonicalize()?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let error = validate_provider(&directory, ProviderKind::Overwrite, &cancellation)
			.await
			.expect_err("pre-cancelled provider validation must stop");

		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		Ok(())
	}

	#[tokio::test]
	async fn snapshot_accepts_data_mod_without_metadata() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		fs::create_dir_all(game.join("Data"))?;
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.await
			.expect("fixture environment must initialize");
		let mod_dir = root.as_path().join("mods/Plain");
		fs::create_dir(&mod_dir)?;
		fs::write(mod_dir.join("ordinary.dds"), b"content")?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Plain\n")?;

		let snapshot = load(
			root.as_path(),
			&initialization_plan(&fixture.path().join("game")).game_binding,
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		)
		.await
		.expect("metadata-free Data Mod must load");
		assert_eq!(
			snapshot.file_dependencies.get("ordinary.dds").map(|fact| fact.state),
			Some(FileDependencyState::Active)
		);
		Ok(())
	}

	#[tokio::test]
	async fn snapshot_ignores_unlisted_entries_and_names_them() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		fs::create_dir_all(game.join("Data"))?;
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.await
			.expect("fixture environment must initialize");
		let mods = root.as_path().join("mods");
		for name in ["Listed", "Unlisted"] {
			fs::create_dir(mods.join(name))?;
			fs::write(mods.join(name).join(format!("{name}.dds")), b"content")?;
		}
		fs::write(mods.join("Unlisted/meta.toml"), b"invalid")?;
		fs::write(mods.join("stray.txt"), b"stray")?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Listed\n")?;

		let snapshot = load(
			root.as_path(),
			&initialization_plan(&game).game_binding,
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		)
		.await
		.expect("unlisted entries must not invalidate the environment");

		let installed: Vec<_> = snapshot
			.installed_mods
			.iter()
			.map(|installed| installed.name.as_str())
			.collect();
		assert_eq!(installed, ["Listed"]);
		let mut unlisted: Vec<_> = snapshot.unlisted_mod_names.iter().map(ModName::as_str).collect();
		unlisted.sort_unstable();
		assert_eq!(unlisted, ["Unlisted", "stray.txt"]);
		let mut winners: Vec<_> = snapshot.current_winners.keys().map(DataRelativePath::as_str).collect();
		winners.sort_unstable();
		assert_eq!(winners, ["Listed.dds"]);
		assert_eq!(fs::read(root.as_path().join("profile/modlist.txt"))?, b"+Listed\n");
		Ok(())
	}

	#[tokio::test]
	async fn snapshot_names_a_listed_mod_without_an_exactly_spelled_directory() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		fs::create_dir_all(game.join("Data"))?;
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.await
			.expect("fixture environment must initialize");
		fs::create_dir(root.as_path().join("mods/Present"))?;
		fs::write(root.as_path().join("mods/File"), b"not a directory")?;

		for (modlist, missing) in [
			("+Missing\n", "Missing"),
			("+present\n", "present"),
			("-File\n", "File"),
		] {
			fs::write(root.as_path().join("profile/modlist.txt"), modlist)?;

			let error = load(
				root.as_path(),
				&initialization_plan(&game).game_binding,
				InstallationStateAccess::Preview,
				&CancellationToken::new(),
			)
			.await
			.expect_err("a listed mod without its directory must be invalid");

			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
			assert_eq!(error.current_context().mod_name().map(ModName::as_str), Some(missing));
		}
		Ok(())
	}

	#[tokio::test]
	async fn plugin_dependency_state_uses_effective_profile_activation() -> StdResult<(), Box<dyn Error>> {
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
			.await
			.expect("fixture environment must initialize");
		let safe_root = root
			.as_path()
			.canonicalize()
			.expect("fixture environment root must canonicalize");
		let visible = visible_plugins(
			&safe_root,
			&initialization_plan(&fixture.path().join("game")).game_binding,
			&CancellationToken::new(),
		)
		.await
		.expect("visible plugins must load");
		assert_eq!(visible.get("éσ.esp").map(String::as_str), Some("éς.ESP"));
		let profile = root.as_path().join("profile");
		fs::write(profile.join("plugins.txt"), b"listed.esp\r\ndEpEnDeNcY.EsL\r\n")?;
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
			&initialization_plan(&fixture.path().join("game")).game_binding,
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		)
		.await
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

	#[tokio::test]
	async fn plugin_dependency_classification_requires_a_data_root_path() -> StdResult<(), Box<dyn Error>> {
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
			.await
			.expect("fixture environment must initialize");

		let snapshot = load(
			root.as_path(),
			&initialization_plan(&fixture.path().join("game")).game_binding,
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		)
		.await
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

	#[tokio::test]
	async fn installation_assessment_reports_only_paths_with_physical_overlaps() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let game = fixture.path().join("game");
		let data = game.join("Data");
		fs::create_dir_all(&data)?;
		fs::write(data.join("existing.dds"), b"game file")?;
		let root = EnvironmentRoot::new(fixture.path().join("environment"))
			.expect("fixture environment root must be valid");
		EnvironmentAdapter
			.publish(&root, initialization_plan(&game), &CancellationToken::new())
			.await
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

		let assessment = assess_installation(
			root.as_path(),
			&initialization_plan(&fixture.path().join("game")).game_binding,
			&plan,
			&CancellationToken::new(),
		)
		.await
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

	#[tokio::test]
	async fn installation_assessment_preserves_physical_overlap_under_directory_tombstone()
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
			.await
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

		let assessment = assess_installation(
			root.as_path(),
			&initialization_plan(&fixture.path().join("game")).game_binding,
			&plan,
			&CancellationToken::new(),
		)
		.await
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

	#[tokio::test]
	async fn preview_rejects_unfinished_work_as_invalid_without_mutation() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let stage = fixture.path().join("temp/operation/stage");
		fs::create_dir_all(&stage)?;
		let evidence = stage.join("partial-file");
		fs::write(&evidence, b"unfinished")?;

		let result = load(
			fixture.path(),
			&initialization_plan(&fixture.path().join("game")).game_binding,
			InstallationStateAccess::Preview,
			&CancellationToken::new(),
		)
		.await;

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read(&evidence)?, b"unfinished");
		assert_eq!(fs::read_dir(fixture.path())?.count(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn unfinished_work_precedes_missing_canonical_layout_without_mutation() -> StdResult<(), Box<dyn Error>> {
		let fixture = TempDir::new_in(current_dir()?)?;
		let stage = fixture.path().join("temp/operation/stage");
		fs::create_dir_all(&stage)?;
		let evidence = stage.join("partial-file");
		fs::write(&evidence, b"unfinished")?;

		let result = load(
			fixture.path(),
			&initialization_plan(&fixture.path().join("game")).game_binding,
			InstallationStateAccess::Mutation,
			&CancellationToken::new(),
		)
		.await;

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::ManualCleanupRequired
		));
		assert_eq!(fs::read(&evidence)?, b"unfinished");
		assert_eq!(fs::read_dir(fixture.path())?.count(), 1);
		Ok(())
	}
}
