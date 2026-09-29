use crate::safe_fs::EntryBudget;
use crate::safe_fs::MAX_TRAVERSAL_DEPTH;
use crate::safe_fs::SafeDir;
use crate::snapshot::MAX_MODS;
use crate::snapshot::MAX_PROVIDER_ENTRIES;
use crate::snapshot::parse_metadata;
use crate::snapshot::parse_modlist;
use application::ErrorMarker;
use domain::DataRelativePath;
use domain::InstalledMod;
use domain::ModName;
use domain::ParticipationReason;
use domain::ProviderIdentity;
use domain::ProviderRank;
use domain::ProviderReference;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(super) struct ExecutionInventory {
	pub(super) installed: Vec<InstalledMod>,
	pub(super) winners: HashMap<String, ProviderReference>,
	namespace: HashMap<String, bool>,
	tombstones: HashMap<String, [Option<ProviderRank>; 2]>,
}

impl ExecutionInventory {
	pub(super) fn read(root: &Path, data: &Path, cancellation: &CancellationToken) -> Result<Self, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let bytes =
			fs::read(root.join("profile/modlist.txt")).context(ErrorMarker::environment_invalid(None))?;
		let installed = parse_modlist(&bytes)?;
		let enabled: HashSet<_> = installed
			.iter()
			.filter(|entry| entry.enabled)
			.map(|entry| entry.name.as_str().to_owned())
			.collect();
		let identities: HashMap<_, _> = installed
			.iter()
			.map(|entry| {
				(
					entry.name.as_str().to_owned(),
					ProviderIdentity::DataMod {
						mod_name: entry.name.clone(),
						priority: entry.priority,
					},
				)
			})
			.collect();
		let listed: HashSet<_> = installed.iter().map(|entry| entry.name.as_str().to_owned()).collect();

		let mut result = Self {
			installed,
			..Self::default()
		};
		match fs::metadata(data) {
			Ok(metadata) if metadata.is_dir() => {
				result.provider(data, ProviderIdentity::SteamData, cancellation)?
			}
			Ok(_) => return Err(report!(ErrorMarker::environment_invalid(None))),
			Err(error) if error.kind() == io::ErrorKind::NotFound => {}
			Err(error) => return Err(report!(error).context(ErrorMarker::environment_invalid(None))),
		}

		let mut discovered = HashSet::new();
		let mut folded = HashSet::new();
		for entry in fs::read_dir(root.join("mods")).context(ErrorMarker::environment_invalid(None))? {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let entry = entry.context(ErrorMarker::environment_invalid(None))?;
			let spelling = entry
				.file_name()
				.into_string()
				.map_err(|_| report!(ErrorMarker::environment_invalid(None)))?;
			let name = ModName::new(spelling.clone()).context(ErrorMarker::environment_invalid(None))?;
			if discovered.len() >= MAX_MODS || !folded.insert(name.comparison_key().to_owned()) {
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			if !fs::metadata(entry.path())
				.context(ErrorMarker::environment_invalid(None))?
				.is_dir()
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			discovered.insert(spelling.clone());
			if !enabled.contains(&spelling) {
				continue;
			}
			let identity = identities
				.get(&spelling)
				.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
			result.provider(&entry.path(), identity.clone(), cancellation)?;
		}

		if discovered != listed {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		result.provider(&root.join("overwrite"), ProviderIdentity::Overwrite, cancellation)?;
		Ok(result)
	}

	fn provider(
		&mut self,
		root: &Path,
		identity: ProviderIdentity,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let metadata_path = root.join("meta.toml");
		let bytes = match fs::read(&metadata_path) {
			Ok(bytes) => Some(bytes),
			Err(error)
				if error.kind() == io::ErrorKind::NotFound
					&& matches!(identity, ProviderIdentity::DataMod { .. }) =>
			{
				let directory = SafeDir::open_absolute(
					&root.canonicalize().context(ErrorMarker::environment_invalid(None))?,
				)
				.context(ErrorMarker::environment_invalid(None))?;
				#[cfg(test)]
				super::tests::before_metadata_creation(root)
					.context(ErrorMarker::environment_invalid(None))?;

				match directory.write_new("meta.toml", b"schema_version = 1\n") {
					Ok(()) => {}
					Err(error)
						if error.current_context().kind() == io::ErrorKind::AlreadyExists => {}
					Err(error) => return Err(error.context(ErrorMarker::environment_invalid(None))),
				}
				Some(fs::read(&metadata_path).context(ErrorMarker::environment_invalid(None))?)
			}
			Err(error) if error.kind() == io::ErrorKind::NotFound => None,
			Err(error) => return Err(report!(error).context(ErrorMarker::environment_invalid(None))),
		};

		let tombstones = if let Some(bytes) = bytes {
			#[cfg(test)]
			crate::snapshot::INVENTORY_IO.with(|count| {
				let (walks, reads) = count.get();
				count.set((walks, reads + 1));
			});
			parse_metadata(bytes)?.tombstones
		} else {
			Vec::new()
		};
		let own: HashMap<_, _> = tombstones
			.iter()
			.map(|(path, directory)| (path.comparison_key().to_owned(), *directory))
			.collect();
		for (path, _) in &tombstones {
			if path.comparison_key()
				.match_indices('/')
				.any(|(index, _)| own.get(&path.comparison_key()[..index]) == Some(&true))
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
		}
		let mut paths = HashSet::new();
		let mut budget = EntryBudget::new(MAX_PROVIDER_ENTRIES);
		self.directory(
			root,
			"",
			&identity,
			&own,
			&mut paths,
			&mut budget,
			MAX_TRAVERSAL_DEPTH,
			cancellation,
		)?;

		for (path, directory) in tombstones {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let key = path.comparison_key().to_owned();
			let rank = identity.rank();
			self.winners.retain(|candidate, winner| {
				let affected = candidate == &key
					|| (directory
						&& candidate
							.strip_prefix(&key)
							.is_some_and(|suffix| suffix.starts_with('/')));
				!affected || winner.rank() > rank
			});
			let scopes = self.tombstones.entry(key).or_default();
			let scope = &mut scopes[usize::from(directory)];
			if scope.is_none_or(|existing| rank > existing) {
				*scope = Some(rank);
			}
		}
		Ok(())
	}

	#[expect(
		clippy::too_many_arguments,
		reason = "recursive traversal keeps provider identity and finite budgets explicit"
	)]
	fn directory(
		&mut self,
		directory: &Path,
		prefix: &str,
		identity: &ProviderIdentity,
		own: &HashMap<String, bool>,
		paths: &mut HashSet<String>,
		budget: &mut EntryBudget,
		depth: usize,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		#[cfg(test)]
		crate::snapshot::INVENTORY_IO.with(|count| {
			let (walks, reads) = count.get();
			count.set((walks + 1, reads));
		});
		let mut count = 0;
		for entry in fs::read_dir(directory).context(ErrorMarker::environment_invalid(None))? {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let entry = entry.context(ErrorMarker::environment_invalid(None))?;
			budget.consume(&mut count)
				.context(ErrorMarker::environment_invalid(None))?;
			if depth == 0 {
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			let name = entry
				.file_name()
				.into_string()
				.map_err(|_| report!(ErrorMarker::environment_invalid(None)))?;
			let relative = if prefix.is_empty() {
				name.clone()
			} else {
				format!("{prefix}/{name}")
			};
			let path = DataRelativePath::new(relative.clone())
				.context(ErrorMarker::environment_invalid(None))?;
			let key = path.comparison_key().to_owned();
			if !paths.insert(key.clone())
				|| own.contains_key(&key) || key
				.match_indices('/')
				.any(|(index, _)| own.get(&key[..index]) == Some(&true))
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			let file_type = entry.file_type().context(ErrorMarker::environment_invalid(None))?;
			let file_type = if file_type.is_symlink() {
				fs::metadata(entry.path())
					.context(ErrorMarker::environment_invalid(None))?
					.file_type()
			} else {
				file_type
			};
			if !file_type.is_dir() && !file_type.is_file() {
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			if prefix.is_empty() && case_fold_key(&name) == "meta.toml" {
				if name != "meta.toml"
					|| !file_type.is_file() || matches!(identity, ProviderIdentity::SteamData)
				{
					return Err(report!(ErrorMarker::environment_invalid(None)));
				}
				continue;
			}
			if prefix.is_empty()
				&& case_fold_key(&name) == "fallout - invalidation.bsa"
				&& !matches!(identity, ProviderIdentity::SteamData)
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			if self.namespace
				.get(&key)
				.is_some_and(|directory| *directory != file_type.is_dir())
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			self.namespace.insert(key.clone(), file_type.is_dir());
			if file_type.is_dir() {
				self.directory(
					&entry.path(),
					&relative,
					identity,
					own,
					paths,
					budget,
					depth - 1,
					cancellation,
				)?;
				continue;
			}
			let suppressed =
				self.tombstones.get(&key).is_some_and(|scopes| {
					scopes.iter().flatten().any(|rank| *rank >= identity.rank())
				}) || key.match_indices('/').any(|(index, _)| {
					self.tombstones.get(&key[..index]).is_some_and(|scopes| {
						scopes[1].is_some_and(|rank| rank >= identity.rank())
					})
				});
			if suppressed
				|| self.winners
					.get(&key)
					.is_some_and(|winner| winner.rank() > identity.rank())
			{
				continue;
			}
			let winner = match identity {
				ProviderIdentity::SteamData => ProviderReference::SteamData { original_path: path },
				ProviderIdentity::DataMod { mod_name, priority } => ProviderReference::DataMod {
					mod_name: mod_name.clone(),
					priority: *priority,
					original_path: path,
					participation_reason: ParticipationReason::EnabledMod,
				},
				ProviderIdentity::Overwrite => ProviderReference::Overwrite { original_path: path },
			};
			self.winners.insert(key, winner);
		}
		Ok(())
	}
}
