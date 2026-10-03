use crate::snapshot::match_mod_folders;
use crate::snapshot::parse_metadata;
use crate::snapshot::parse_modlist;
use application::ErrorMarker;
use domain::DataRelativePath;
use domain::InstalledMod;
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
use std::io;
use std::path::Path;
use tokio::fs::OpenOptions;
use tokio::fs::metadata;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(super) struct ExecutionInventory {
	pub(super) installed: Vec<InstalledMod>,
	pub(super) winners: HashMap<String, ProviderReference>,
	namespace: HashMap<String, bool>,
	tombstones: HashMap<String, [Option<ProviderRank>; 2]>,
}

impl ExecutionInventory {
	pub(super) async fn read(
		root: &Path,
		data: &Path,
		cancellation: &CancellationToken,
	) -> Result<Self, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let bytes = read(root.join("profile/modlist.txt"))
			.await
			.context(ErrorMarker::environment_invalid(None))?;
		let installed = parse_modlist(&bytes)?;

		let mods = root.join("mods");
		match_mod_folders(&mods, &installed, cancellation).await?;

		let mut result = Self::default();
		match metadata(data).await {
			Ok(metadata) if metadata.is_dir() => {
				result.provider(data, ProviderIdentity::SteamData, cancellation).await?
			}
			Ok(_) => return Err(report!(ErrorMarker::environment_invalid(None))),
			Err(error) if error.kind() == io::ErrorKind::NotFound => {}
			Err(error) => return Err(report!(error).context(ErrorMarker::environment_invalid(None))),
		}

		for entry in installed.iter().filter(|entry| entry.enabled) {
			let identity = ProviderIdentity::DataMod {
				mod_name: entry.name.clone(),
				priority: entry.priority,
			};
			result.provider(&mods.join(entry.name.as_str()), identity, cancellation)
				.await?;
		}

		result.provider(&root.join("overwrite"), ProviderIdentity::Overwrite, cancellation)
			.await?;

		result.installed = installed;
		Ok(result)
	}

	async fn provider(
		&mut self,
		root: &Path,
		identity: ProviderIdentity,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let metadata_path = root.join("meta.toml");
		let bytes = match read(&metadata_path).await {
			Ok(bytes) => Some(bytes),
			Err(error)
				if error.kind() == io::ErrorKind::NotFound
					&& matches!(identity, ProviderIdentity::DataMod { .. }) =>
			{
				#[cfg(test)]
				super::tests::before_metadata_creation(root)
					.context(ErrorMarker::environment_invalid(None))?;

				create_default_metadata(&metadata_path).await?;
				Some(read(&metadata_path)
					.await
					.context(ErrorMarker::environment_invalid(None))?)
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
		self.directory(root, "", &identity, &own, &mut paths, cancellation)
			.await?;

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

	async fn directory(
		&mut self,
		directory: &Path,
		prefix: &str,
		identity: &ProviderIdentity,
		own: &HashMap<String, bool>,
		paths: &mut HashSet<String>,
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

			let file_type = entry
				.file_type()
				.await
				.context(ErrorMarker::environment_invalid(None))?;
			let file_type = if file_type.is_symlink() {
				metadata(entry.path())
					.await
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
				Box::pin(self.directory(&entry.path(), &relative, identity, own, paths, cancellation))
					.await?;
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

/// Creates the default provider metadata without replacing a file that a concurrent writer created first.
async fn create_default_metadata(path: &Path) -> Result<(), ErrorMarker> {
	let mut file = match OpenOptions::new().write(true).create_new(true).open(path).await {
		Ok(file) => file,
		Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(()),
		Err(error) => return Err(report!(error).context(ErrorMarker::environment_invalid(None))),
	};

	file.write_all(b"schema_version = 1\n")
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	// Tokio completes file writes in the background; the flush makes the bytes visible to the next read.
	file.flush().await.context(ErrorMarker::environment_invalid(None))
}
