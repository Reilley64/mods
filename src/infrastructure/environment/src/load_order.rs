use crate::EnvironmentAdapter;
use crate::PreparedLaunch;
use crate::derived_profile::selected_archive_list;
use crate::files::set_modified;
use application::ErrorMarker;
use application::ports::EnvironmentPlan;
use application::ports::LoadOrderFile;
use application::ports::LoadOrderTarget;
use application::ports::PortFuture;
use application::ports::SetLoadOrderTimes;
use domain::DataRelativePath;
use domain::LoadOrderCandidate;
use domain::ProviderIdentity;
use domain::derived_archive_list;
use domain::load_order_times;
use domain::takes_load_order_time;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::metadata;
use tokio_util::sync::CancellationToken;

const INVALIDATION_ARCHIVE: &str = "Fallout - Invalidation.bsa";
const INVALIDATION_KEY: &str = "fallout - invalidation.bsa";

/// A Data-root winner that may get a load-order time.
struct RootFile {
	path: DataRelativePath,
	physical_path: PathBuf,
	/// Export copies the game's own Data files only with `include_game_data`.
	game_data: bool,
}

/// The plan inputs for load-order times, copied so the future does not borrow the plan.
struct LoadOrderInputs {
	files: Vec<RootFile>,
	load_order: String,
	archive_list: String,
}

impl LoadOrderInputs {
	fn new(prepared: &PreparedLaunch) -> Result<Self, ErrorMarker> {
		let text = |name: &str| {
			prepared.profile_files
				.iter()
				.find(|file| file.name == name)
				.map(|file| file.text.as_str())
		};

		let invalidation = RootFile {
			path: DataRelativePath::new(INVALIDATION_ARCHIVE.to_owned())
				.context(ErrorMarker::invalid_data_path())?,
			physical_path: prepared.cache_directory.join(INVALIDATION_ARCHIVE),
			game_data: false,
		};

		let files = prepared
			.winners
			.iter()
			.zip(&prepared.visible_files)
			// The generated invalidation archive replaces any Data copy of it.
			.filter(|(_, file)| {
				takes_load_order_time(&file.path) && file.path.comparison_key() != INVALIDATION_KEY
			})
			.map(|(winner, file)| RootFile {
				path: file.path.clone(),
				physical_path: file.physical_path.clone(),
				game_data: winner.identity() == ProviderIdentity::SteamData,
			})
			.chain([invalidation])
			.collect();

		Ok(Self {
			files,
			load_order: text("loadorder.txt").unwrap_or_default().to_owned(),
			archive_list: selected_archive_list(text("Fallout.ini"), text("FalloutCustom.ini")).to_owned(),
		})
	}

	async fn apply(self, target: LoadOrderTarget, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
		let mut candidates = Vec::with_capacity(self.files.len());
		for file in self.files {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

			let modified = metadata(&file.physical_path)
				.await
				.and_then(|metadata| metadata.modified())
				.context(ErrorMarker::io_failure())
				.map_err(|error| naming(error, &file.physical_path))?;

			candidates.push(LoadOrderCandidate {
				path: file.path.clone(),
				modified,
				file: (file, modified),
			});
		}

		let archive_list = derived_archive_list(&self.archive_list);
		let timed = load_order_times(candidates, &self.load_order, &archive_list);

		for ((file, modified), time) in timed {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

			let path = match &target {
				LoadOrderTarget::Sources if modified == time => continue,
				LoadOrderTarget::Sources => file.physical_path,
				LoadOrderTarget::Export {
					include_game_data: false,
					..
				} if file.game_data => continue,
				LoadOrderTarget::Export { output, .. } => output.join("Data").join(file.path.as_str()),
			};
			set_modified(&path, time)
				.await
				.context(ErrorMarker::io_failure())
				.map_err(|error| naming(error, &path))?;
		}
		Ok(())
	}
}

/// Attaches the file that failed, so the user can find it.
fn naming(mut error: Report<ErrorMarker>, path: &Path) -> Report<ErrorMarker> {
	error.children_mut()
		.push(report!(LoadOrderFile { path: path.to_owned() })
			.into_dynamic()
			.into_cloneable());
	error
}

impl EnvironmentAdapter {
	/// Sets the times on the winning files for exec, or on the output copies for
	/// export. Exec skips files that already have their time. Export skips the
	/// game's own Data files unless it copied them.
	pub fn set_load_order_times_port(&self) -> SetLoadOrderTimes {
		Arc::new(|plan: &EnvironmentPlan, target, cancellation| {
			let inputs = plan
				.state
				.downcast_ref::<PreparedLaunch>()
				.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))
				.and_then(LoadOrderInputs::new);

			Box::pin(async move { inputs?.apply(target, &cancellation).await }) as PortFuture<_>
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ExecutionProfileText;
	use crate::LaunchVisibleFile;
	use application::ports::AdapterState;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ProviderReference;
	use std::fs;
	#[cfg(unix)]
	use std::os::unix::fs::PermissionsExt;
	use std::path::Path;
	use std::time::Duration;
	use std::time::SystemTime;
	use std::time::UNIX_EPOCH;
	use tempfile::TempDir;

	fn modified(path: &Path) -> Result<SystemTime, ErrorMarker> {
		fs::metadata(path)
			.context(ErrorMarker::io_failure())?
			.modified()
			.context(ErrorMarker::io_failure())
	}

	/// Makes a file read-only, or writable again so the temporary directory can be
	/// removed on Windows.
	fn set_read_only(path: &Path, read_only: bool) -> Result<(), ErrorMarker> {
		let mut permissions = fs::metadata(path).context(ErrorMarker::io_failure())?.permissions();
		#[cfg(unix)]
		permissions.set_mode(if read_only { 0o444 } else { 0o644 });
		#[cfg(not(unix))]
		permissions.set_readonly(read_only);

		fs::set_permissions(path, permissions).context(ErrorMarker::io_failure())
	}

	/// A plan whose winners come from the game's Data folder and Overwrite.
	fn plan(root: &Path, winners: &[(&str, bool)]) -> Result<EnvironmentPlan, ErrorMarker> {
		let game = root.join("game");
		let mut references = Vec::new();
		let mut visible_files = Vec::new();
		for (name, base) in winners {
			let path =
				DataRelativePath::new((*name).to_owned()).context(ErrorMarker::invalid_data_path())?;
			let (reference, directory) = if *base {
				(
					ProviderReference::SteamData {
						original_path: path.clone(),
					},
					game.join("Data"),
				)
			} else {
				(
					ProviderReference::Overwrite {
						original_path: path.clone(),
					},
					root.join("overwrite"),
				)
			};
			let physical_path = directory.join(name);

			fs::create_dir_all(physical_path.parent().unwrap_or(&directory))
				.context(ErrorMarker::io_failure())?;
			fs::write(&physical_path, name).context(ErrorMarker::io_failure())?;
			references.push(reference);
			visible_files.push(LaunchVisibleFile { path, physical_path });
		}
		fs::create_dir_all(root.join("cache")).context(ErrorMarker::io_failure())?;
		fs::write(root.join("cache").join(INVALIDATION_ARCHIVE), b"bsa").context(ErrorMarker::io_failure())?;

		let prepared = PreparedLaunch {
			game_binding: GameBinding::new(
				GameInstallationPath::new(game.clone()).context(ErrorMarker::game_install_invalid())?,
			),
			providers: Vec::new(),
			winners: references,
			visible_files,
			profile_files: vec![
				ExecutionProfileText {
					name: "Fallout.ini",
					text: "[Archive]\r\nsArchiveList=Fallout - Misc.bsa\r\n".to_owned(),
				},
				ExecutionProfileText {
					name: "loadorder.txt",
					text: "FalloutNV.esm\r\nMod.esp\r\n".to_owned(),
				},
			],
			profile_directory: root.join("profile"),
			data_directory: game.join("Data"),
			cache_directory: root.join("cache"),
		};
		Ok(EnvironmentPlan {
			providers: Vec::new(),
			state: AdapterState::new(prepared),
		})
	}

	#[tokio::test]
	async fn exec_times_the_winning_files_where_they_are() -> Result<(), ErrorMarker> {
		let temp = TempDir::new().context(ErrorMarker::io_failure())?;
		let root = temp.path();
		let plan = plan(
			root,
			&[
				("FalloutNV.esm", true),
				("Fallout - Misc.bsa", true),
				("Mod.esp", false),
				("Mod - Main.bsa", false),
				("Textures/Mod.dds", false),
				("Fallout - Invalidation.bsa", true),
			],
		)?;
		let plugin = root.join("overwrite/Mod.esp");
		let texture = root.join("overwrite/Textures/Mod.dds");
		let texture_time = modified(&texture)?;
		let game_invalidation = root.join("game/Data").join(INVALIDATION_ARCHIVE);
		let game_invalidation_time = modified(&game_invalidation)?;
		set_read_only(&plugin, true)?;

		let timed = EnvironmentAdapter
			.set_load_order_times_port()
			.call((&plan, LoadOrderTarget::Sources, CancellationToken::new()))
			.await;

		let first = UNIX_EPOCH + Duration::from_secs(946_684_800);
		let checked = timed.and_then(|()| {
			// The invalidation archive and the listed archive come first, then the
			// plugins; the Mod archive loads with Mod.esp.
			for (path, position) in [
				(root.join("cache").join(INVALIDATION_ARCHIVE), 0),
				(root.join("game/Data/Fallout - Misc.bsa"), 1),
				(root.join("game/Data/FalloutNV.esm"), 2),
				(plugin.clone(), 3),
				(root.join("overwrite/Mod - Main.bsa"), 3),
			] {
				assert_eq!(
					modified(&path)?,
					first + Duration::from_secs(60 * position),
					"{}",
					path.display()
				);
			}
			assert_eq!(modified(&texture)?, texture_time);
			// The generated archive replaces a game Data copy, which keeps its time.
			assert_eq!(modified(&game_invalidation)?, game_invalidation_time);
			Ok(())
		});
		set_read_only(&plugin, false)?;
		checked
	}

	#[tokio::test]
	async fn a_second_exec_run_changes_no_times() -> Result<(), ErrorMarker> {
		let temp = TempDir::new().context(ErrorMarker::io_failure())?;
		let root = temp.path();
		let plan = plan(
			root,
			&[("FalloutNV.esm", true), ("Mod.esp", false), ("Mod - Main.bsa", false)],
		)?;
		let files = [
			root.join("game/Data/FalloutNV.esm"),
			root.join("overwrite/Mod.esp"),
			root.join("overwrite/Mod - Main.bsa"),
			root.join("cache").join(INVALIDATION_ARCHIVE),
		];
		let port = EnvironmentAdapter.set_load_order_times_port();
		port.call((&plan, LoadOrderTarget::Sources, CancellationToken::new()))
			.await?;
		let first = files.iter().map(|path| modified(path)).collect::<Result<Vec<_>, _>>()?;

		// Without read access, opening a file to set its time fails on Unix, so the
		// second run must not open any file that already has its time.
		#[cfg(unix)]
		for path in &files {
			fs::set_permissions(path, fs::Permissions::from_mode(0o000))
				.context(ErrorMarker::io_failure())?;
		}
		let second = port
			.call((&plan, LoadOrderTarget::Sources, CancellationToken::new()))
			.await;
		#[cfg(unix)]
		for path in &files {
			fs::set_permissions(path, fs::Permissions::from_mode(0o644))
				.context(ErrorMarker::io_failure())?;
		}

		second?;
		let again = files.iter().map(|path| modified(path)).collect::<Result<Vec<_>, _>>()?;
		assert_eq!(again, first);
		Ok(())
	}

	#[tokio::test]
	async fn a_failure_names_the_file_and_other_root_files_are_not_read() -> Result<(), ErrorMarker> {
		let temp = TempDir::new().context(ErrorMarker::io_failure())?;
		let root = temp.path();
		let plan = plan(root, &[("Mod.esp", false), ("Readme.txt", false)])?;
		let plugin = root.join("overwrite/Mod.esp");
		fs::remove_file(root.join("overwrite/Readme.txt")).context(ErrorMarker::io_failure())?;
		fs::remove_file(&plugin).context(ErrorMarker::io_failure())?;
		let port = EnvironmentAdapter.set_load_order_times_port();

		let Err(error) = port
			.call((&plan, LoadOrderTarget::Sources, CancellationToken::new()))
			.await
		else {
			return Err(report!(ErrorMarker::io_failure()));
		};

		let named = error
			.iter_reports()
			.find_map(|cause| cause.downcast_current_context::<LoadOrderFile>())
			.map(|file| file.path.clone());
		assert_eq!(named, Some(plugin.clone()));

		// The missing readme is not a plugin or archive, so it is never read.
		fs::write(&plugin, b"plugin").context(ErrorMarker::io_failure())?;
		port.call((&plan, LoadOrderTarget::Sources, CancellationToken::new()))
			.await
	}
}
