use crate::EnvironmentAdapter;
use crate::active_code_page::decode as decode_active_code_page;
use crate::profile::MAX_PROFILE_BYTES;
use crate::profile::PROFILE_FILES;
use crate::profile::decode;
use crate::safe_fs::SafeDir;
use crate::safe_fs::read_bounded;
use crate::snapshot::load_execution;
use application::ErrorMarker;
use application::installation::InstallationState;
use application::ports::ExecutionProfileText;
use application::ports::ExecutionProvider;
use application::ports::ExecutionVisibleFile;
use application::ports::PreparedExecution;
use domain::EffectiveResult;
use domain::EnvironmentRoot;
use domain::GameBinding;
use domain::ProviderIdentity;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::str::from_utf8;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

impl EnvironmentAdapter {
	/// Reads execution inputs without repairing or rewriting canonical state.
	///
	/// The caller must validate the effective settings binding through the game platform adapter.
	///
	/// # Errors
	///
	/// Refuses pending work, invalid state, unsafe paths, and cancellation.
	pub fn prepare_execution(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<PreparedExecution, ErrorMarker> {
		self.prepare_execution_with_spool(root, effective_binding, None, cancellation)
	}

	fn prepare_execution_with_spool(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		owned_spool: Option<&TempDir>,
		cancellation: &CancellationToken,
	) -> Result<PreparedExecution, ErrorMarker> {
		let snapshot = load_execution(root.as_path(), effective_binding, owned_spool, cancellation)?;
		let data_directory = effective_binding.game_directory().as_path().join("Data");
		let mut providers = vec![ExecutionProvider {
			identity: ProviderIdentity::SteamData,
			root: data_directory.clone(),
			enabled: true,
		}];
		for installed in &snapshot.installed_mods {
			providers.push(ExecutionProvider {
				identity: ProviderIdentity::DataMod {
					mod_name: installed.name.clone(),
					priority: installed.priority,
				},
				root: root.as_path().join("mods").join(installed.name.as_str()),
				enabled: installed.enabled,
			});
		}
		providers.push(ExecutionProvider {
			identity: ProviderIdentity::Overwrite,
			root: root.as_path().join("overwrite"),
			enabled: true,
		});

		let mut winners: Vec<_> = snapshot
			.current_winners
			.values()
			.filter_map(|result| {
				if let EffectiveResult::File(provider) = result {
					Some(provider.clone())
				} else {
					None
				}
			})
			.collect();
		winners.sort_by(|left, right| {
			left.original_path()
				.comparison_key()
				.cmp(right.original_path().comparison_key())
		});

		let mut visible_files = Vec::with_capacity(winners.len());
		let mut file_lengths = Vec::with_capacity(winners.len());
		for winner in &winners {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let provider = providers
				.iter()
				.find(|provider| provider.identity == winner.identity())
				.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
			let details = snapshot
				.file_details
				.get(winner.original_path().comparison_key())
				.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
			file_lengths.push(details.length);
			visible_files.push(ExecutionVisibleFile {
				path: winner.original_path().clone(),
				physical_path: provider.root.join(winner.original_path().as_str()),
				modified: details.modified,
			});
		}

		let root_directory =
			SafeDir::open_absolute(root.as_path()).context(ErrorMarker::environment_invalid(None))?;
		let profile = root_directory
			.open_dir("profile")
			.context(ErrorMarker::environment_invalid(None))?;
		let profile_directory = root.as_path().join("profile");
		let mut consumed_bytes = Vec::new();
		let mut profile_files = Vec::new();
		for name in PROFILE_FILES.into_iter().chain(["modlist.txt"]) {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if !profile.exists(name).context(ErrorMarker::environment_invalid(None))? {
				continue;
			}
			let bytes = read_bounded(
				&profile,
				name,
				MAX_PROFILE_BYTES,
				ErrorMarker::environment_invalid(None),
				cancellation,
			)?;
			if name != "modlist.txt" {
				let text = match name {
					"plugins.txt" => decode_active_code_page(&bytes)?,
					"loadorder.txt" => from_utf8(&bytes)
						.context(ErrorMarker::environment_invalid(None))?
						.to_owned(),
					_ => decode(&bytes).context(ErrorMarker::environment_invalid(None))?.0,
				};
				profile_files.push(ExecutionProfileText { name, text });
			}
			consumed_bytes.push((profile_directory.join(name), bytes));
		}

		let manifest = read_bounded(
			&root_directory,
			"mods.toml",
			MAX_PROFILE_BYTES,
			ErrorMarker::environment_invalid(None),
			cancellation,
		)?;
		consumed_bytes.push((root.as_path().join("mods.toml"), manifest));

		consumed_bytes.extend(snapshot.provider_metadata);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		Ok(PreparedExecution {
			game_binding: effective_binding.clone(),
			providers,
			winners,
			visible_files,
			profile_files,
			profile_directory,
			data_directory,
			cache_directory: root.as_path().join("cache"),
			consumed_state: InstallationState {
				game_binding: snapshot.game_binding,
				installed_mods: snapshot.installed_mods,
				current_winners: snapshot.current_winners,
				file_dependencies: snapshot.file_dependencies,
			},
			consumed_bytes,
			file_lengths,
		})
	}

	/// Refuses changed inputs immediately before process creation.
	///
	/// # Errors
	///
	/// Returns an environment error if any consumed configuration or file metadata changed.
	pub fn revalidate_execution(
		&self,
		root: &EnvironmentRoot,
		prepared: &PreparedExecution,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		let current = self.prepare_execution(root, &prepared.game_binding, cancellation)?;
		if current != *prepared {
			return Err(report!(ErrorMarker::environment_invalid(Some(
				"execution_state_changed"
			))));
		}
		Ok(())
	}

	/// Accepts only the currently owned capture directory, not arbitrary pending work.
	///
	/// # Errors
	///
	/// Refuses other temporary entries and invalid retained environment state.
	pub fn check_execution_with_spool(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		owned_spool: Option<&TempDir>,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		self.prepare_execution_with_spool(root, effective_binding, owned_spool, cancellation)?;

		Ok(())
	}

	/// Validates retained post-run state without rollback, regardless of child status.
	///
	/// # Errors
	///
	/// Reports invalid retained state. Call after Job drain with an uncancelled token.
	pub fn check_execution(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		self.prepare_execution(root, effective_binding, cancellation)?;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::snapshot::INVENTORY_IO;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileSource;
	use domain::GameInstallationPath;
	use domain::ProviderReference;
	use domain::SteamBuildId;
	use std::env::current_dir;
	use std::fs;
	use tempfile::TempDir;

	fn fixture() -> Result<(TempDir, EnvironmentRoot, GameBinding), ErrorMarker> {
		let temp = TempDir::new_in(current_dir().context(ErrorMarker::io_failure())?)
			.context(ErrorMarker::io_failure())?;
		let root = EnvironmentRoot::new(temp.path().join("environment"))
			.context(ErrorMarker::environment_root_unsafe())?;
		let game = temp.path().join("game");
		fs::create_dir_all(game.join("Data")).context(ErrorMarker::io_failure())?;
		fs::write(game.join("Data/FalloutNV.esm"), b"fixture").context(ErrorMarker::io_failure())?;
		let binding = GameBinding::new(
			GameInstallationPath::new(game).context(ErrorMarker::game_install_invalid())?,
			SteamBuildId::new(1).context(ErrorMarker::game_install_invalid())?,
		);
		EnvironmentAdapter.publish(
			&root,
			InitializationPlan {
				game_binding: binding.clone(),
				profile_sources: InitializationProfileSources {
					files: PROFILE_FILES
						.into_iter()
						.map(|name| ProfileSource { name, contents: None })
						.collect(),
					fallout_default_ini: b"[Archive]\r\nsArchiveList=Fallout - Meshes.bsa\r\n"
						.to_vec(),
				},
			},
			&CancellationToken::new(),
		)?;
		Ok((temp, root, binding))
	}

	#[test]
	fn execution_collects_each_provider_once() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		for name in ["Enabled", "Disabled"] {
			let directory = root.as_path().join("mods").join(name);
			fs::create_dir(&directory).context(ErrorMarker::io_failure())?;
			fs::write(directory.join("meta.toml"), b"schema_version = 1\n")
				.context(ErrorMarker::io_failure())?;
			fs::write(directory.join("test.txt"), name).context(ErrorMarker::io_failure())?;
		}
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Enabled\r\n-Disabled\r\n")
			.context(ErrorMarker::io_failure())?;
		INVENTORY_IO.with(|count| count.set((0, 0)));
		let prepared = EnvironmentAdapter.prepare_execution(&root, &binding, &CancellationToken::new())?;
		let counts = INVENTORY_IO.with(|count| count.get());
		assert_eq!(prepared.winners.len(), 2);
		assert_eq!(counts, (4, 2));

		EnvironmentAdapter.revalidate_execution(&root, &prepared, &CancellationToken::new())?;
		assert_eq!(INVENTORY_IO.with(|count| count.get()), (8, 4));
		Ok(())
	}

	#[test]
	fn inventories_preserve_priority_tombstones_and_fresh_revalidation() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let data = binding.game_directory().as_path().join("Data");
		fs::create_dir(data.join("Textures")).context(ErrorMarker::io_failure())?;
		fs::write(data.join("Textures/hidden.txt"), b"base").context(ErrorMarker::io_failure())?;
		fs::write(data.join("Textures/restored.txt"), b"base").context(ErrorMarker::io_failure())?;
		fs::write(data.join("exact.txt"), b"base").context(ErrorMarker::io_failure())?;
		for name in ["Low", "High", "Disabled"] {
			fs::create_dir(root.as_path().join("mods").join(name)).context(ErrorMarker::io_failure())?;
		}
		fs::write(
			root.as_path().join("mods/Low/meta.toml"),
			b"schema_version = 1\n[tombstones]\nfiles = ['exact.txt']\ndirectories = ['textures']\n",
		)
		.context(ErrorMarker::io_failure())?;
		fs::create_dir(root.as_path().join("mods/High/TEXTURES")).context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("mods/High/TEXTURES/RESTORED.txt"), b"high")
			.context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("mods/High/winner.txt"), b"high").context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("mods/Disabled/winner.txt"), b"disabled")
			.context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("overwrite/WINNER.txt"), b"overwrite")
			.context(ErrorMarker::io_failure())?;
		fs::write(
			root.as_path().join("profile/modlist.txt"),
			b"+Low\r\n+High\r\n-Disabled\r\n",
		)
		.context(ErrorMarker::io_failure())?;

		let cancellation = CancellationToken::new();
		let prepared = EnvironmentAdapter.prepare_execution(&root, &binding, &cancellation)?;
		let visible: Vec<_> = prepared.visible_files.iter().map(|file| file.path.as_str()).collect();
		assert_eq!(visible, ["FalloutNV.esm", "TEXTURES/RESTORED.txt", "WINNER.txt"]);
		assert!(matches!(prepared.winners[1], ProviderReference::DataMod { .. }));
		assert!(matches!(prepared.winners[2], ProviderReference::Overwrite { .. }));
		assert_eq!(prepared.file_lengths, [7, 4, 9]);
		EnvironmentAdapter.revalidate_execution(&root, &prepared, &cancellation)?;

		fs::write(
			root.as_path().join("mods/High/TEXTURES/RESTORED.txt"),
			b"changed length",
		)
		.context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.revalidate_execution(&root, &prepared, &cancellation)
			.is_err());
		let prepared = EnvironmentAdapter.prepare_execution(&root, &binding, &cancellation)?;
		fs::write(root.as_path().join("mods/Disabled/meta.toml"), b"schema_version = 1\n")
			.context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.revalidate_execution(&root, &prepared, &cancellation)
			.is_err());
		fs::write(root.as_path().join("mods/Disabled/meta.toml"), b"schema_version = 2\n")
			.context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.prepare_execution(&root, &binding, &cancellation)
			.is_err());
		Ok(())
	}

	#[test]
	fn folder_sets_reject_mismatches_duplicates_and_spelling_changes() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		fs::create_dir(root.as_path().join("mods/Present")).context(ErrorMarker::io_failure())?;
		for modlist in [
			"+Missing\n",
			"+Present\n+Present\n",
			"+Present\n-present\n",
			"+present\n",
			"",
		] {
			fs::write(root.as_path().join("profile/modlist.txt"), modlist)
				.context(ErrorMarker::io_failure())?;
			assert!(
				EnvironmentAdapter
					.prepare_execution(&root, &binding, &CancellationToken::new())
					.is_err(),
				"{modlist:?}"
			);
		}
		fs::write(root.as_path().join("profile/modlist.txt"), b"-Present\n")
			.context(ErrorMarker::io_failure())?;
		EnvironmentAdapter.prepare_execution(&root, &binding, &CancellationToken::new())?;
		Ok(())
	}

	#[test]
	fn inventory_rejects_cross_provider_file_directory_collision() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		fs::create_dir(root.as_path().join("overwrite/FalloutNV.esm")).context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.prepare_execution(&root, &binding, &CancellationToken::new())
			.is_err());
		Ok(())
	}

	#[test]
	fn live_owned_spool_is_not_pending_mutation_but_other_entries_are() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let adapter = EnvironmentAdapter;
		let cancellation = CancellationToken::new();
		adapter.prepare_execution(&root, &binding, &cancellation)?;
		let spool = TempDir::new_in(root.as_path().join("temp")).context(ErrorMarker::io_failure())?;

		adapter.check_execution_with_spool(&root, &binding, Some(&spool), &cancellation)?;
		assert!(adapter.check_execution(&root, &binding, &cancellation).is_err());
		let other = TempDir::new_in(root.as_path().join("temp")).context(ErrorMarker::io_failure())?;
		assert!(adapter
			.check_execution_with_spool(&root, &binding, Some(&spool), &cancellation)
			.is_err());
		drop(other);
		adapter.check_execution_with_spool(&root, &binding, Some(&spool), &cancellation)?;
		drop(spool);

		adapter.check_execution(&root, &binding, &cancellation)?;
		Ok(())
	}

	#[test]
	fn missing_plugin_lists_and_opaque_save_contents_are_allowed() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		for name in ["plugins.txt", "loadorder.txt"] {
			fs::remove_file(root.as_path().join("profile").join(name)).context(ErrorMarker::io_failure())?;
		}
		fs::create_dir_all(root.as_path().join("profile/saves/arbitrary.nvse/folder"))
			.context(ErrorMarker::io_failure())?;
		fs::write(
			root.as_path().join("profile/saves/arbitrary.nvse/folder/unpaired"),
			b"opaque",
		)
		.context(ErrorMarker::io_failure())?;
		let prepared = EnvironmentAdapter.prepare_execution(&root, &binding, &CancellationToken::new())?;
		assert_eq!(prepared.winners.len(), 1);
		let archive = fs::read(prepared.cache_directory.join("Fallout - Invalidation.bsa"))
			.context(ErrorMarker::io_failure())?;
		assert_eq!(archive.len(), 36);
		assert_eq!(&archive[..12], &[66, 83, 65, 0, 104, 0, 0, 0, 36, 0, 0, 0]);
		assert!(!prepared.profile_files.iter().any(|file| file.name == "plugins.txt"));
		assert!(!root.as_path().join("profile/plugins.txt").exists());
		Ok(())
	}

	#[test]
	fn invalid_postrun_plugin_entries_are_reported_without_repair() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let plugins = root.as_path().join("profile/plugins.txt");
		fs::write(&plugins, b"Unsupported.esl\r\n").context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.check_execution(&root, &binding, &CancellationToken::new())
			.is_err());
		assert_eq!(
			fs::read(&plugins).context(ErrorMarker::io_failure())?,
			b"Unsupported.esl\r\n"
		);
		Ok(())
	}

	#[test]
	fn pending_work_is_refused_without_cleanup() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let pending = root.as_path().join("temp/unfinished");
		fs::create_dir(&pending).context(ErrorMarker::io_failure())?;
		let result = EnvironmentAdapter.prepare_execution(&root, &binding, &CancellationToken::new());
		assert!(
			matches!(result, Err(error) if error.current_context() == &ErrorMarker::manual_cleanup_required())
		);
		assert!(pending.is_dir());
		Ok(())
	}

	#[test]
	fn changed_consumed_state_is_refused_but_valid_postrun_edits_persist() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let prepared = EnvironmentAdapter.prepare_execution(&root, &binding, &CancellationToken::new())?;
		let plugins = root.as_path().join("profile/plugins.txt");
		fs::write(&plugins, b"# retained edit\r\nMissing.esp\r\n").context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.revalidate_execution(&root, &prepared, &CancellationToken::new())
			.is_err());
		EnvironmentAdapter.check_execution(&root, &binding, &CancellationToken::new())?;
		assert_eq!(
			fs::read(&plugins).context(ErrorMarker::io_failure())?,
			b"# retained edit\r\nMissing.esp\r\n"
		);
		Ok(())
	}
}
