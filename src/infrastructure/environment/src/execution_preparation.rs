mod inventory;

use self::inventory::ExecutionInventory;
use crate::EnvironmentAdapter;
use crate::ExecutionInis;
use crate::active_code_page::decode as decode_active_code_page;
use crate::profile::MAX_PROFILE_BYTES;
use crate::profile::PROFILE_FILES;
use crate::profile::decode;
use crate::profile::validate_plugin_text;
use crate::safe_fs::SafeDir;
use crate::safe_fs::read_bounded;
use crate::snapshot::load_execution;
use application::ErrorMarker;
use application::installation::InstallationState;
use domain::DataRelativePath;
use domain::EffectiveResult;
use domain::EnvironmentRoot;
use domain::GameBinding;
use domain::ProviderIdentity;
use domain::ProviderReference;
use domain::canonical_profile_routing_valid;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::from_utf8;
use std::time::SystemTime;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProvider {
	pub identity: ProviderIdentity,
	pub root: PathBuf,
	pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchVisibleFile {
	pub path: DataRelativePath,
	pub physical_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProfileText {
	pub name: &'static str,
	pub text: String,
}

/// Inputs for managed execution. Enabled mods receive missing default metadata.
/// Winners and visible files describe the analytical Data projection, not observed
/// runtime visibility. Profile texts come from canonical Profile State.
/// This is not a coherent concurrent filesystem snapshot and is not revalidated before launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedLaunch {
	pub game_binding: GameBinding,
	pub providers: Vec<ExecutionProvider>,
	pub winners: Vec<ProviderReference>,
	pub visible_files: Vec<LaunchVisibleFile>,
	pub profile_files: Vec<ExecutionProfileText>,
	pub profile_directory: PathBuf,
	pub data_directory: PathBuf,
	pub cache_directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionVisibleFile {
	pub path: DataRelativePath,
	pub physical_path: PathBuf,
	pub modified: SystemTime,
}

/// Strict preparation retained for export comparisons and read-only inventories.
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

	/// Reads execution inputs and creates missing enabled-mod metadata.
	/// The binding is retained without verifying its Steam installation or build.
	/// Cancellation is cooperative between operations; a synchronous whole-file read cannot be interrupted.
	///
	/// # Errors
	///
	/// Refuses pending work, invalid state, unsafe paths, and cancellation.
	pub fn prepare_launch(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<PreparedLaunch, ErrorMarker> {
		self.prepare_launch_with_spool(root, effective_binding, None, cancellation)
	}

	fn prepare_launch_with_spool(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		owned_spool: Option<&TempDir>,
		cancellation: &CancellationToken,
	) -> Result<PreparedLaunch, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let root_directory =
			SafeDir::open_absolute(root.as_path()).context(ErrorMarker::environment_invalid(None))?;
		if root_directory
			.exists("temp")
			.context(ErrorMarker::environment_invalid(None))?
		{
			let temp = root_directory
				.open_dir("temp")
				.context(ErrorMarker::environment_invalid(None))?;
			for entry in temp.entries().context(ErrorMarker::environment_invalid(None))? {
				let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
				if owned_spool.is_some_and(|spool| {
					spool.path().parent() == Some(root.as_path().join("temp").as_path())
						&& spool.path().file_name() == Some(entry.file_name().as_os_str())
				}) {
					continue;
				}
				return Err(report!(ErrorMarker::manual_cleanup_required()));
			}
		}

		let profile = root_directory
			.open_dir("profile")
			.context(ErrorMarker::environment_invalid(None))?;
		profile.open_dir("saves")
			.context(ErrorMarker::environment_invalid(None))?;
		let profile_directory = root.as_path().join("profile");
		let mut profile_files = Vec::new();
		for name in PROFILE_FILES {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if name != "Fallout.ini"
				&& !profile.exists(name).context(ErrorMarker::environment_invalid(None))?
			{
				continue;
			}
			let bytes = profile.read(name).context(ErrorMarker::environment_invalid(None))?;
			let text = match name {
				"plugins.txt" => decode_active_code_page(&bytes)?,
				"loadorder.txt" => from_utf8(&bytes)
					.context(ErrorMarker::environment_invalid(None))?
					.to_owned(),
				_ => decode(&bytes).context(ErrorMarker::environment_invalid(None))?.0,
			};
			if ["Fallout.ini", "FalloutPrefs.ini", "FalloutCustom.ini"].contains(&name)
				&& !canonical_profile_routing_valid(name, &text)
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
			if matches!(name, "plugins.txt" | "loadorder.txt") {
				validate_plugin_text(&text, false)?;
			}

			profile_files.push(ExecutionProfileText { name, text });
		}

		let data_directory = effective_binding.game_directory().as_path().join("Data");
		let inventory = ExecutionInventory::read(root.as_path(), &data_directory, cancellation)?;
		let mut providers = vec![ExecutionProvider {
			identity: ProviderIdentity::SteamData,
			root: data_directory.clone(),
			enabled: true,
		}];
		for installed in inventory.installed {
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

		let provider_roots: HashMap<_, _> = providers
			.iter()
			.map(|provider| (provider.identity.clone(), &provider.root))
			.collect();
		let winners: Vec<_> = inventory.winners.into_values().collect();
		let mut visible_files = Vec::with_capacity(winners.len());
		for winner in &winners {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let provider = provider_roots
				.get(&winner.identity())
				.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
			visible_files.push(LaunchVisibleFile {
				path: winner.original_path().clone(),
				physical_path: provider.join(winner.original_path().as_str()),
			});
		}

		Ok(PreparedLaunch {
			game_binding: effective_binding.clone(),
			providers,
			winners,
			visible_files,
			profile_files,
			profile_directory,
			data_directory,
			cache_directory: root.as_path().join("cache"),
		})
	}

	/// # Errors
	/// Retains partial INI derivation on failure. Caller owns the files through Job drain.
	pub fn derive_execution_inis(
		&self,
		root: &EnvironmentRoot,
		prepared: &PreparedLaunch,
		cancellation: &CancellationToken,
	) -> Result<ExecutionInis, ErrorMarker> {
		ExecutionInis::create(
			&prepared.profile_directory,
			prepared.game_binding.game_directory().as_path(),
			&root.as_path().join("temp"),
			cancellation,
		)
	}

	/// Accepts only the currently owned capture directory, not arbitrary pending work.
	///
	/// # Errors
	///
	/// Refuses other temporary entries and invalid retained environment state.
	pub fn check_launch_with_spool(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		owned_spool: Option<&TempDir>,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		self.prepare_launch_with_spool(root, effective_binding, owned_spool, cancellation)?;

		Ok(())
	}

	/// Validates retained post-run state without rollback, regardless of child status.
	///
	/// # Errors
	///
	/// Reports invalid retained state. Call after Job drain with an uncancelled token.
	pub fn check_launch(
		&self,
		root: &EnvironmentRoot,
		effective_binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<(), ErrorMarker> {
		self.prepare_launch(root, effective_binding, cancellation)?;
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
	use std::cell::Cell;
	use std::env::current_dir;
	use std::fs;
	use std::io;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	use std::path::Path;
	use std::time::Instant;
	use tempfile::TempDir;

	thread_local! {
	    static CREATE_CONCURRENT_METADATA: Cell<bool> = const { Cell::new(false) };
	}

	pub(super) fn before_metadata_creation(root: &Path) -> Result<(), io::Error> {
		if CREATE_CONCURRENT_METADATA.with(|flag| flag.replace(false)) {
			fs::write(root.join("meta.toml"), b"schema_version = 1\n# concurrent\n").into_report()?;
		}
		Ok(())
	}

	#[test]
	fn preservation_rejects_hard_linked_inis_and_retains_child_edits() -> Result<(), ErrorMarker> {
		for linked_copy in ["canonical", "child"] {
			let (temp, root, binding) = fixture()?;
			let canonical = root.as_path().join("profile/Fallout.ini");
			let original = fs::read(&canonical).context(ErrorMarker::io_failure())?;
			let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
			let inis = EnvironmentAdapter.derive_execution_inis(
				&root,
				&prepared,
				&CancellationToken::new(),
			)?;
			let retained = inis.path().to_owned();
			let child = retained.join("Fallout.ini");
			let mut edited = fs::read(&child).context(ErrorMarker::io_failure())?;
			edited.extend_from_slice(b"\r\n[Display]\r\nreview_child_edit=1\r\n");
			fs::write(&child, &edited).context(ErrorMarker::io_failure())?;
			let linked = if linked_copy == "canonical" { &canonical } else { &child };
			fs::hard_link(linked, temp.path().join("linked-ini")).context(ErrorMarker::io_failure())?;

			let error = inis
				.preserve()
				.err()
				.ok_or_else(|| report!(ErrorMarker::io_failure()))?;

			assert!(retained.exists());
			assert_eq!(fs::read(&child).context(ErrorMarker::io_failure())?, edited);
			assert_eq!(fs::read(&canonical).context(ErrorMarker::io_failure())?, original);
			assert!(error
				.iter_reports()
				.any(|cause| cause.downcast_current_context::<io::Error>().is_some()));
			assert!(error.iter_reports().any(|cause| cause
				.downcast_current_context::<application::execution::RetainedExecutionInis>()
				.is_some_and(|state| state.path == retained)));
		}
		Ok(())
	}

	#[cfg(unix)]
	#[test]
	fn launch_rejects_a_symlinked_required_fallout_ini() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let profile = root.as_path().join("profile");
		fs::rename(profile.join("Fallout.ini"), profile.join("linked-fallout.ini"))
			.context(ErrorMarker::io_failure())?;
		symlink("linked-fallout.ini", profile.join("Fallout.ini")).context(ErrorMarker::io_failure())?;

		assert!(EnvironmentAdapter
			.prepare_launch(&root, &binding, &CancellationToken::new())
			.is_err());
		Ok(())
	}

	#[test]
	fn metadata_creation_is_enabled_only_and_never_overwrites_concurrent_files() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		for name in ["Enabled", "Disabled"] {
			fs::create_dir(root.as_path().join("mods").join(name)).context(ErrorMarker::io_failure())?;
		}
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Enabled\r\n-Disabled\r\n")
			.context(ErrorMarker::io_failure())?;
		EnvironmentAdapter.prepare_execution(&root, &binding, &CancellationToken::new())?;
		assert!(!root.as_path().join("mods/Enabled/meta.toml").exists());

		EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		let metadata = root.as_path().join("mods/Enabled/meta.toml");
		assert_eq!(
			fs::read(&metadata).context(ErrorMarker::io_failure())?,
			b"schema_version = 1\n"
		);
		assert!(!root.as_path().join("mods/Disabled/meta.toml").exists());
		assert!(!root.as_path().join("overwrite/meta.toml").exists());
		assert!(!binding.game_directory().as_path().join("Data/meta.toml").exists());

		fs::remove_file(&metadata).context(ErrorMarker::io_failure())?;
		CREATE_CONCURRENT_METADATA.with(|flag| flag.set(true));
		EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		assert_eq!(
			fs::read(&metadata).context(ErrorMarker::io_failure())?,
			b"schema_version = 1\n# concurrent\n"
		);
		fs::write(&metadata, b"invalid").context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.prepare_launch(&root, &binding, &CancellationToken::new())
			.is_err());
		assert_eq!(fs::read(&metadata).context(ErrorMarker::io_failure())?, b"invalid");
		Ok(())
	}

	#[test]
	fn launch_ignores_extra_entries_and_does_not_validate_generated_bsa() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		for path in ["unrelated", "cache/unrelated", "profile/unrelated"] {
			fs::create_dir(root.as_path().join(path)).context(ErrorMarker::io_failure())?;
		}
		let archive = root.as_path().join("cache/Fallout - Invalidation.bsa");
		fs::write(&archive, b"corrupt fixture").context(ErrorMarker::io_failure())?;
		EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		EnvironmentAdapter.check_launch(&root, &binding, &CancellationToken::new())?;
		assert!(EnvironmentAdapter
			.prepare_execution(&root, &binding, &CancellationToken::new())
			.is_err());
		assert_eq!(
			fs::read(&archive).context(ErrorMarker::io_failure())?,
			b"corrupt fixture"
		);
		Ok(())
	}

	#[test]
	fn exec_has_no_steam_prerequisites_or_configuration_size_limit() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let data = binding.game_directory().as_path().join("Data");
		fs::write(data.join("Fallout - Invalidation.bsa"), b"existing").context(ErrorMarker::io_failure())?;
		let mut contents =
			fs::read(root.as_path().join("profile/Fallout.ini")).context(ErrorMarker::io_failure())?;
		contents.extend_from_slice(format!(";{}\n", "x".repeat(crate::profile::MAX_PROFILE_BYTES)).as_bytes());
		fs::write(root.as_path().join("profile/Fallout.ini"), &contents).context(ErrorMarker::io_failure())?;
		let mut manifest = fs::read(root.as_path().join("mods.toml")).context(ErrorMarker::io_failure())?;
		manifest.extend_from_slice(
			format!("#{}\n", "x".repeat(crate::manifest::MAX_MANIFEST_BYTES)).as_bytes(),
		);
		fs::write(root.as_path().join("mods.toml"), manifest).context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.prepare_execution(&root, &binding, &CancellationToken::new())
			.is_err());
		let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		assert_eq!(prepared.winners.len(), 2);
		let inis = EnvironmentAdapter.derive_execution_inis(&root, &prepared, &CancellationToken::new())?;
		inis.preserve()?;
		assert_eq!(
			fs::read(root.as_path().join("profile/Fallout.ini")).context(ErrorMarker::io_failure())?,
			contents
		);
		Ok(())
	}

	#[cfg(unix)]
	#[test]
	fn links_are_followed_but_cycles_remain_bounded_and_disabled_cycles_are_skipped() -> Result<(), ErrorMarker> {
		let (temp, root, binding) = fixture()?;
		let external = temp.path().join("external");
		fs::create_dir(&external).context(ErrorMarker::io_failure())?;
		fs::write(external.join("linked.txt"), b"target").context(ErrorMarker::io_failure())?;
		symlink(&external, root.as_path().join("mods/Linked")).context(ErrorMarker::io_failure())?;
		fs::create_dir(root.as_path().join("mods/Disabled")).context(ErrorMarker::io_failure())?;
		symlink(".", root.as_path().join("mods/Disabled/cycle")).context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("mods/Disabled/meta.toml"), b"invalid")
			.context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("profile/modlist.txt"), b"+Linked\r\n-Disabled\r\n")
			.context(ErrorMarker::io_failure())?;
		fs::hard_link(external.join("linked.txt"), external.join("hard.txt"))
			.context(ErrorMarker::io_failure())?;
		let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		assert_eq!(prepared.winners.len(), 3);
		symlink(".", external.join("cycle")).context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.prepare_launch(&root, &binding, &CancellationToken::new())
			.is_err());
		Ok(())
	}

	#[test]
	fn priority_and_subtree_reinstatement_do_not_depend_on_mod_directory_creation_order() -> Result<(), ErrorMarker>
	{
		for names in [["Low", "High", "Middle"], ["Middle", "High", "Low"]] {
			let (_temp, root, binding) = fixture()?;
			for name in names {
				fs::create_dir(root.as_path().join("mods").join(name))
					.context(ErrorMarker::io_failure())?;
			}
			fs::create_dir_all(root.as_path().join("mods/Low/Textures"))
				.context(ErrorMarker::io_failure())?;
			fs::write(root.as_path().join("mods/Low/Textures/hidden.txt"), b"low")
				.context(ErrorMarker::io_failure())?;
			fs::write(
				root.as_path().join("mods/Middle/meta.toml"),
				b"schema_version=1\n[tombstones]\ndirectories=['textures']\n",
			)
			.context(ErrorMarker::io_failure())?;
			fs::create_dir_all(root.as_path().join("mods/High/TEXTURES"))
				.context(ErrorMarker::io_failure())?;
			fs::write(root.as_path().join("mods/High/TEXTURES/restored.txt"), b"high")
				.context(ErrorMarker::io_failure())?;
			fs::write(
				root.as_path().join("profile/modlist.txt"),
				b"+Low\r\n+Middle\r\n+High\r\n",
			)
			.context(ErrorMarker::io_failure())?;
			let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
			assert!(prepared
				.winners
				.iter()
				.all(|winner| winner.original_path().comparison_key() != "textures/hidden.txt"));
			assert!(prepared
				.winners
				.iter()
				.any(|winner| winner.original_path().comparison_key() == "textures/restored.txt"));
		}
		Ok(())
	}

	fn fixture() -> Result<(TempDir, EnvironmentRoot, GameBinding), ErrorMarker> {
		let temp = TempDir::new_in(current_dir().context(ErrorMarker::io_failure())?)
			.context(ErrorMarker::io_failure())?;
		let root = EnvironmentRoot::new(temp.path().join("environment"))
			.context(ErrorMarker::environment_root_unsafe())?;
		let game = temp.path().join("game");
		fs::create_dir_all(game.join("Data")).context(ErrorMarker::io_failure())?;
		fs::write(game.join("Data/FalloutNV.esm"), b"fixture").context(ErrorMarker::io_failure())?;
		let binding =
			GameBinding::new(GameInstallationPath::new(game).context(ErrorMarker::game_install_invalid())?);
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
		crate::snapshot::INVENTORY_IO.with(|count| count.set((0, 0)));
		let started = Instant::now();
		let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		let counts = crate::snapshot::INVENTORY_IO.with(|count| count.get());
		eprintln!("provider inventory: {counts:?}, elapsed {:?}", started.elapsed());
		assert_eq!(prepared.winners.len(), 2);
		assert_eq!(counts, (3, 1));

		Ok(())
	}

	#[test]
	fn inventories_preserve_priority_tombstones_and_skip_disabled_metadata() -> Result<(), ErrorMarker> {
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
		let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &cancellation)?;
		let mut visible: Vec<_> = prepared.visible_files.iter().map(|file| file.path.as_str()).collect();
		visible.sort();
		assert_eq!(visible, ["FalloutNV.esm", "TEXTURES/RESTORED.txt", "WINNER.txt"]);
		assert!(prepared
			.winners
			.iter()
			.any(|winner| matches!(winner, ProviderReference::DataMod { .. })));
		assert!(prepared
			.winners
			.iter()
			.any(|winner| matches!(winner, ProviderReference::Overwrite { .. })));
		fs::write(root.as_path().join("mods/Disabled/meta.toml"), b"schema_version = 2\n")
			.context(ErrorMarker::io_failure())?;
		EnvironmentAdapter.prepare_launch(&root, &binding, &cancellation)?;
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
					.prepare_launch(&root, &binding, &CancellationToken::new())
					.is_err(),
				"{modlist:?}"
			);
		}
		fs::write(root.as_path().join("profile/modlist.txt"), b"-Present\n")
			.context(ErrorMarker::io_failure())?;
		EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		Ok(())
	}

	#[test]
	fn inventory_rejects_cross_provider_file_directory_collision() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		fs::create_dir(root.as_path().join("overwrite/FalloutNV.esm")).context(ErrorMarker::io_failure())?;
		assert!(EnvironmentAdapter
			.prepare_launch(&root, &binding, &CancellationToken::new())
			.is_err());
		Ok(())
	}

	#[test]
	fn live_owned_spool_is_not_pending_mutation_but_other_entries_are() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let adapter = EnvironmentAdapter;
		let cancellation = CancellationToken::new();
		adapter.prepare_launch(&root, &binding, &cancellation)?;
		let spool = TempDir::new_in(root.as_path().join("temp")).context(ErrorMarker::io_failure())?;

		adapter.check_launch_with_spool(&root, &binding, Some(&spool), &cancellation)?;
		assert!(adapter.check_launch(&root, &binding, &cancellation).is_err());
		let other = TempDir::new_in(root.as_path().join("temp")).context(ErrorMarker::io_failure())?;
		assert!(adapter
			.check_launch_with_spool(&root, &binding, Some(&spool), &cancellation)
			.is_err());
		drop(other);
		adapter.check_launch_with_spool(&root, &binding, Some(&spool), &cancellation)?;
		drop(spool);

		adapter.check_launch(&root, &binding, &cancellation)?;
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
		let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
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
			.check_launch(&root, &binding, &CancellationToken::new())
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
		let result = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new());
		assert!(
			matches!(result, Err(error) if error.current_context() == &ErrorMarker::manual_cleanup_required())
		);
		assert!(pending.is_dir());
		Ok(())
	}

	#[test]
	fn valid_postrun_edits_persist_without_prelaunch_revalidation() -> Result<(), ErrorMarker> {
		let (_temp, root, binding) = fixture()?;
		let prepared = EnvironmentAdapter.prepare_launch(&root, &binding, &CancellationToken::new())?;
		let plugins = root.as_path().join("profile/plugins.txt");
		fs::write(&plugins, b"# retained edit\r\nMissing.esp\r\n").context(ErrorMarker::io_failure())?;
		assert!(prepared
			.profile_files
			.iter()
			.any(|file| file.name == "plugins.txt" && file.text.is_empty()));
		EnvironmentAdapter.check_launch(&root, &binding, &CancellationToken::new())?;
		assert_eq!(
			fs::read(&plugins).context(ErrorMarker::io_failure())?,
			b"# retained edit\r\nMissing.esp\r\n"
		);
		Ok(())
	}
}
