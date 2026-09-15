use crate::GamePlatformAdapter;
use crate::adapter::KnownFolderSource;
use crate::cancellation::check_cancelled;
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
use tokio_util::sync::CancellationToken;

impl GamePlatformAdapter {
	pub(crate) fn load_profile_sources(
		&self,
		binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<InitializationProfileSources, ErrorMarker> {
		check_cancelled(cancellation)?;
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
			check_cancelled(cancellation)?;
			let contents = directory
				.map(|directory| read_optional_file(directory, Path::new(name)))
				.transpose()?
				.flatten();
			files.push(ProfileSource { name, contents });
		}
		check_cancelled(cancellation)?;
		let (game, _) = fs_access::open_ambient_dir(binding.game_directory().as_path())
			.context(ErrorMarker::game_install_invalid())?;
		let fallout_default_ini = read_required_file(&game, Path::new("Fallout_default.ini"))?;
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

fn read_required_file(directory: &Dir, path: &Path) -> Result<Vec<u8>, ErrorMarker> {
	read_optional_file(directory, path)?.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))
}
