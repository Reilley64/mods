use crate::GamePlatformAdapter;
use crate::adapter::KnownFolderSource;
use crate::fs_access;
use crate::known_folders;
use application::ErrorMarker;
use application::ports::InitializationProfileSources;
use application::ports::ProfileSource;
use cap_std::fs::Dir;
use domain::GameBinding;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

impl GamePlatformAdapter {
	/// Resolves the game's Documents and Local AppData destinations using Known Folders.
	///
	/// # Errors
	/// Returns a platform error when either destination cannot be resolved.
	pub fn execution_profile_directories(&self) -> Result<(PathBuf, PathBuf), ErrorMarker> {
		let folders = match &self.known_folders {
			KnownFolderSource::System => known_folders::current()?,
			#[cfg(test)]
			KnownFolderSource::Fixed(folders) => folders.clone(),
		};
		if !folders.documents.is_absolute() || !folders.local_app_data.is_absolute() {
			return Err(report!(ErrorMarker::game_install_invalid()));
		}
		Ok((
			folders.documents.join("My Games").join("FalloutNV"),
			folders.local_app_data.join("FalloutNV"),
		))
	}

	pub(crate) fn load_profile_sources(
		&self,
		binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<InitializationProfileSources, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let system_folders;
		let folders = match &self.known_folders {
			KnownFolderSource::System => {
				system_folders = known_folders::current()?;
				&system_folders
			}
			#[cfg(test)]
			KnownFolderSource::Fixed(folders) => folders,
		};
		let documents = open_optional_directory(&folders.documents, &["My Games", "FalloutNV"])?;
		let local = open_optional_directory(&folders.local_app_data, &["FalloutNV"])?;
		let specifications = [
			("Fallout.ini", documents.as_ref()),
			("FalloutPrefs.ini", documents.as_ref()),
			("FalloutCustom.ini", documents.as_ref()),
			("GECKCustom.ini", documents.as_ref()),
			("GECKPrefs.ini", documents.as_ref()),
			("plugins.txt", local.as_ref()),
			("loadorder.txt", local.as_ref()),
			("Plugins.fnvviewsettings", local.as_ref()),
		];
		let mut files = Vec::with_capacity(specifications.len());
		for (name, directory) in specifications {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

			let contents = directory
				.map(|directory| read_optional_file(directory, Path::new(name)))
				.transpose()?
				.flatten();
			files.push(ProfileSource { name, contents });
		}
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let (game, _) = fs_access::open_ambient_dir(binding.game_directory().as_path())
			.context(ErrorMarker::game_install_invalid())?;
		let fallout_default_ini = read_optional_file(&game, Path::new("Fallout_default.ini"))?
			.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
		Ok(InitializationProfileSources {
			files,
			fallout_default_ini,
		})
	}
}

fn open_optional_directory(base: &Path, components: &[&str]) -> Result<Option<Dir>, ErrorMarker> {
	if base.as_os_str().is_empty() {
		return Ok(None);
	}
	let (mut directory, _) = match fs_access::open_ambient_dir(base) {
		Ok(opened) => opened,
		Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(error.context(ErrorMarker::game_install_invalid())),
	};
	for component in components {
		directory = match fs_access::open_dir(&directory, Path::new(component)) {
			Ok(directory) => directory,
			Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(None),
			Err(error) => return Err(error.context(ErrorMarker::game_install_invalid())),
		};
	}
	Ok(Some(directory))
}

fn read_optional_file(directory: &Dir, path: &Path) -> Result<Option<Vec<u8>>, ErrorMarker> {
	let mut file = match fs_access::open_regular(directory, path) {
		Ok(file) => file,
		Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(error.context(ErrorMarker::game_install_invalid())),
	};
	let mut contents = Vec::new();
	file.read_to_end(&mut contents)
		.context(ErrorMarker::game_install_invalid())?;
	Ok(Some(contents))
}

#[cfg(test)]
mod tests {
	use super::GamePlatformAdapter;
	use crate::adapter::KnownFolderSource;
	use crate::known_folders::KnownFolderPaths;
	use crate::steam;
	use application::ErrorCode;
	use rootcause::Result;
	use std::collections::BTreeMap;
	use std::fs;
	use std::io::Error as IoError;
	use std::io::Result as IoResult;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	#[cfg(windows)]
	use std::os::windows::fs::symlink_file as windows_symlink_file;
	use std::path::Path;
	use std::path::PathBuf;
	use std::sync::Arc;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[cfg(not(windows))]
	#[test]
	fn empty_non_windows_system_known_folders_mean_no_profile_sources() -> Result<()> {
		let (_fixture, game) = fixture()?;
		let binding = steam::validate(&game)?;
		let sources =
			GamePlatformAdapter::system().load_profile_sources(&binding, &CancellationToken::new())?;
		assert!(sources.files.iter().all(|source| source.contents.is_none()));
		assert_eq!(sources.fallout_default_ini, b"[Archive]\n");
		Ok(())
	}

	#[test]
	fn profile_sources_are_read_from_known_folder_capabilities() -> Result<()> {
		let (game_fixture, game) = fixture()?;
		let profile_fixture = TempDir::new()?;
		let profile_root = fs::canonicalize(profile_fixture.path())?;
		let documents = profile_root.join("Documents");
		let local_app_data = profile_root.join("LocalAppData");
		fs::create_dir_all(documents.join("My Games/FalloutNV"))?;
		fs::create_dir_all(local_app_data.join("FalloutNV"))?;
		fs::write(documents.join("My Games/FalloutNV/Fallout.ini"), b"[Archive]\n")?;
		fs::write(local_app_data.join("FalloutNV/plugins.txt"), b"Example.esp\r\n")?;
		fs::create_dir_all(documents.join("My Games/FalloutNV/Saves"))?;
		fs::write(documents.join("My Games/FalloutNV/Saves/existing.fos"), b"save fixture")?;
		let profile_before = file_inventory(&profile_root)?;
		let game_before = file_inventory(&game)?;

		let binding = steam::validate(&game)?;
		let sources = adapter_with_profiles(documents, local_app_data)
			.load_profile_sources(&binding, &CancellationToken::new())?;
		assert_eq!(sources.files[0].contents, Some(b"[Archive]\n".to_vec()));
		assert_eq!(sources.files[5].contents, Some(b"Example.esp\r\n".to_vec()));
		assert_eq!(sources.fallout_default_ini, b"[Archive]\n");
		assert_eq!(file_inventory(&profile_root)?, profile_before);
		assert_eq!(file_inventory(&game)?, game_before);
		drop(game_fixture);
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn symlinked_profile_source_is_rejected() -> Result<()> {
		let (_game_fixture, game) = fixture()?;
		let profile_fixture = TempDir::new()?;
		let profile_root = fs::canonicalize(profile_fixture.path())?;
		let documents = profile_root.join("Documents");
		let local_app_data = profile_root.join("LocalAppData");
		let fallout = documents.join("My Games/FalloutNV");
		fs::create_dir_all(&fallout)?;
		fs::create_dir_all(local_app_data.join("FalloutNV"))?;
		let target = profile_root.join("outside.ini");
		fs::write(&target, b"outside")?;
		symlink_file(&target, &fallout.join("Fallout.ini"))?;
		let binding = steam::validate(&game)?;
		assert!(adapter_with_profiles(documents, local_app_data)
			.load_profile_sources(&binding, &CancellationToken::new())
			.is_err());
		Ok(())
	}

	#[test]
	fn cancelled_profile_load_stops_before_reads() -> Result<()> {
		let (_temp, game) = fixture()?;
		let binding = steam::validate(&game)?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = adapter_with_profiles(PathBuf::new(), PathBuf::new())
			.load_profile_sources(&binding, &cancellation);
		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::OperationCancelled),
		);
		Ok(())
	}

	fn file_inventory(directory: &Path) -> IoResult<BTreeMap<PathBuf, Option<Vec<u8>>>> {
		let mut inventory = BTreeMap::new();
		for entry in fs::read_dir(directory)? {
			let entry = entry?;
			let path = entry.path();
			if entry.file_type()?.is_dir() {
				inventory.insert(path.clone(), None);
				inventory.extend(file_inventory(&path)?);
			} else {
				inventory.insert(path.clone(), Some(fs::read(&path)?));
			}
		}
		Ok(inventory)
	}

	fn fixture() -> Result<(TempDir, PathBuf)> {
		let temp = TempDir::new()?;
		let game = fs::canonicalize(temp.path())?.join("steam/steamapps/common/Fallout New Vegas");
		fs::create_dir_all(game.join("Data"))?;
		fs::write(game.join("FalloutNV.exe"), b"exe")?;
		fs::write(game.join("Fallout_default.ini"), b"[Archive]\n")?;
		fs::write(
			game.parent()
				.and_then(Path::parent)
				.ok_or_else(|| IoError::other("missing steamapps"))?
				.join("appmanifest_22380.acf"),
			concat!(
				"\"AppState\"\n{\n",
				"\"appid\" \"22380\"\n",
				"\"buildid\" \"88\"\n",
				"\"installdir\" \"Fallout New Vegas\"\n}"
			),
		)?;
		Ok((temp, game))
	}

	fn adapter_with_profiles(documents: PathBuf, local_app_data: PathBuf) -> GamePlatformAdapter {
		GamePlatformAdapter {
			steam_roots: Arc::new(Vec::new()),
			bethesda_hints: Arc::new(Vec::new()),
			known_folders: KnownFolderSource::Fixed(KnownFolderPaths {
				documents,
				local_app_data,
			}),
		}
	}

	#[cfg(unix)]
	fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
		symlink(source, destination)
	}

	#[cfg(windows)]
	fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
		windows_symlink_file(source, destination)
	}
}
