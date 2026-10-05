use crate::ErrorMarker;
use crate::export::ExportFile;
use crate::export::ExportProvider;
use domain::DataRelativePath;
use domain::ProviderIdentity;
use domain::ProviderRank;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;

/// Keeps the game's own Data files only with `include_game_data`, gives shared
/// directories the spelling of their highest-ranked provider, and sorts by path.
/// The generated invalidation archive wins over a copy in the game's Data folder.
pub(super) fn plan_inventory(
	candidates: Vec<ExportFile>,
	include_game_data: bool,
) -> Result<(Vec<ExportFile>, u64), ErrorMarker> {
	let generated: HashSet<_> = candidates
		.iter()
		.filter(|file| file.provider == ExportProvider::GeneratedInvalidation)
		.map(|file| file.path.comparison_key().to_owned())
		.collect();
	let mut files: Vec<_> = candidates
		.into_iter()
		.filter(|file| {
			file.provider != ExportProvider::Data(ProviderIdentity::SteamData)
				|| (include_game_data && !generated.contains(file.path.comparison_key()))
		})
		.collect();

	let mut directories: HashMap<String, (ProviderRank, String)> = HashMap::new();
	let mut file_keys = HashSet::new();
	for file in &files {
		if !file_keys.insert(file.path.comparison_key().to_owned()) {
			return Err(report!(ErrorMarker::invalid_data_path()));
		}

		let rank = if let ExportProvider::Data(provider) = &file.provider {
			provider.rank()
		} else {
			ProviderRank::Base
		};
		let mut key = String::new();
		let components: Vec<_> = file.path.components().collect();
		for component in components.iter().take(components.len() - 1) {
			if !key.is_empty() {
				key.push('/');
			}
			key.push_str(&case_fold_key(component));
			if directories.get(&key).is_none_or(|(previous, _)| rank > *previous) {
				directories.insert(key.clone(), (rank, (*component).to_owned()));
			}
		}
	}

	if directories.keys().any(|key| file_keys.contains(key)) {
		return Err(report!(ErrorMarker::invalid_data_path()));
	}

	let mut total_bytes = 0_u64;
	for file in &mut files {
		let components: Vec<_> = file.path.components().collect();
		let mut key = String::new();
		let mut spelling = Vec::new();
		for (index, component) in components.iter().enumerate() {
			if !key.is_empty() {
				key.push('/');
			}
			key.push_str(&case_fold_key(component));
			if index + 1 == components.len() {
				spelling.push((*component).to_owned());
			} else if let Some((_, directory)) = directories.get(&key) {
				spelling.push(directory.clone());
			}
		}
		file.path = DataRelativePath::new(spelling.join("/")).context(ErrorMarker::invalid_data_path())?;
		total_bytes = total_bytes
			.checked_add(file.bytes)
			.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
	}
	files.sort_by(|left, right| left.path.comparison_key().cmp(right.path.comparison_key()));

	Ok((files, total_bytes))
}

#[cfg(test)]
mod tests {
	use super::plan_inventory;
	use crate::ErrorMarker;
	use crate::export::ExportFile;
	use crate::export::ExportProvider;
	use domain::DataRelativePath;
	use domain::ProviderIdentity;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;

	fn entry(id: usize, path: &str, provider: ExportProvider) -> Result<ExportFile, ErrorMarker> {
		Ok(ExportFile {
			source_id: id,
			path: DataRelativePath::new(path.to_owned()).context(ErrorMarker::invalid_data_path())?,
			provider,
			bytes: 4,
		})
	}

	fn candidates() -> Result<Vec<ExportFile>, ErrorMarker> {
		Ok(vec![
			entry(
				0,
				"Data/FalloutNV.esm",
				ExportProvider::Data(ProviderIdentity::SteamData),
			)?,
			entry(
				1,
				"Data/Music/Theme.mp3",
				ExportProvider::Data(ProviderIdentity::SteamData),
			)?,
			entry(2, "Data/Mod.esp", ExportProvider::Data(ProviderIdentity::Overwrite))?,
			entry(
				3,
				"Data/Fallout - Invalidation.bsa",
				ExportProvider::GeneratedInvalidation,
			)?,
		])
	}

	fn paths(files: &[ExportFile]) -> Vec<&str> {
		files.iter().map(|file| file.path.as_str()).collect()
	}

	#[test]
	fn game_data_files_are_kept_only_with_the_flag() -> Result<(), ErrorMarker> {
		let (without, without_bytes) = plan_inventory(candidates()?, false)?;
		let (with, with_bytes) = plan_inventory(candidates()?, true)?;

		assert_eq!(paths(&without), ["Data/Fallout - Invalidation.bsa", "Data/Mod.esp"]);
		assert_eq!(without_bytes, 8);
		assert_eq!(
			paths(&with),
			[
				"Data/Fallout - Invalidation.bsa",
				"Data/FalloutNV.esm",
				"Data/Mod.esp",
				"Data/Music/Theme.mp3"
			]
		);
		assert_eq!(with_bytes, 16);
		Ok(())
	}

	#[test]
	fn the_generated_invalidation_archive_wins_over_the_game_copy() -> Result<(), ErrorMarker> {
		let mut candidates = candidates()?;
		candidates.push(entry(
			4,
			"Data/fallout - invalidation.BSA",
			ExportProvider::Data(ProviderIdentity::SteamData),
		)?);

		let (files, _) = plan_inventory(candidates, true)?;

		let archives: Vec<_> = files
			.iter()
			.filter(|file| file.path.comparison_key() == "data/fallout - invalidation.bsa")
			.map(|file| &file.provider)
			.collect();
		assert_eq!(archives, [&ExportProvider::GeneratedInvalidation]);
		Ok(())
	}
}
