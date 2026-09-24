use crate::EnvironmentAdapter;
use crate::active_code_page::decode as decode_active_code_page;
use crate::profile::MAX_PROFILE_BYTES;
use crate::profile::PROFILE_FILES;
use crate::profile::decode;
use crate::safe_fs::SafeDir;
use crate::safe_fs::read_bounded;
use crate::snapshot::MAX_PROVIDER_METADATA_BYTES;
use crate::snapshot::load_execution;
use application::ErrorMarker;
use application::installation::InstallationState;
use domain::DataRelativePath;
use domain::EffectiveResult;
use domain::EnvironmentRoot;
use domain::GameBinding;
use domain::ProviderIdentity;
use domain::ProviderReference;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::PathBuf;
use std::str::from_utf8;
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProvider {
	pub identity: ProviderIdentity,
	pub root: PathBuf,
	pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionVisibleFile {
	pub path: DataRelativePath,
	pub physical_path: PathBuf,
	pub modified: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProfileText {
	pub name: &'static str,
	pub text: String,
}

/// Validated, read-only inputs for managed execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedExecution {
	pub game_binding: GameBinding,
	pub providers: Vec<ExecutionProvider>,
	pub winners: Vec<ProviderReference>,
	pub visible_files: Vec<ExecutionVisibleFile>,
	pub profile_files: Vec<ExecutionProfileText>,
	pub profile_directory: PathBuf,
	pub data_directory: PathBuf,
	pub cache_directory: PathBuf,
	consumed_state: InstallationState,
	consumed_bytes: Vec<(PathBuf, Vec<u8>)>,
	file_lengths: Vec<u64>,
}

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
		let snapshot = load_execution(root.as_path(), effective_binding, cancellation)?;
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
			let mut directory = SafeDir::open_absolute(&provider.root)
				.context(ErrorMarker::environment_invalid(None))?;
			let mut components = winner.original_path().components().peekable();
			while let Some(component) = components.next() {
				if components.peek().is_some() {
					directory = directory
						.open_dir(component)
						.context(ErrorMarker::environment_invalid(None))?;
					continue;
				}
				let file = directory
					.open_regular(component)
					.context(ErrorMarker::environment_invalid(None))?;
				let metadata = file.metadata().context(ErrorMarker::environment_invalid(None))?;
				file_lengths.push(metadata.len());
				visible_files.push(ExecutionVisibleFile {
					path: winner.original_path().clone(),
					physical_path: provider.root.join(winner.original_path().as_str()),
					modified: metadata
						.modified()
						.context(ErrorMarker::environment_invalid(None))?
						.into_std(),
				});
			}
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

		for provider in &providers {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if provider.identity == ProviderIdentity::SteamData {
				continue;
			}
			let directory = SafeDir::open_absolute(&provider.root)
				.context(ErrorMarker::environment_invalid(None))?;
			if directory
				.exists("meta.toml")
				.context(ErrorMarker::environment_invalid(None))?
			{
				let bytes = read_bounded(
					&directory,
					"meta.toml",
					MAX_PROVIDER_METADATA_BYTES,
					ErrorMarker::environment_invalid(None),
					cancellation,
				)?;
				consumed_bytes.push((provider.root.join("meta.toml"), bytes));
			}
		}
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
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileSource;
	use domain::GameInstallationPath;
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
