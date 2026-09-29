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

pub(super) fn plan_inventory(candidates: Vec<ExportFile>) -> Result<(Vec<ExportFile>, u64), ErrorMarker> {
	let mut files: Vec<_> = candidates
		.into_iter()
		.filter(|file| file.provider != ExportProvider::Data(ProviderIdentity::SteamData))
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
