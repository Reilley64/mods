use crate::fs_access;
use application::ErrorMarker;
use domain::EnvironmentRoot;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub(super) fn prove_separation(root: &EnvironmentRoot, game: &GameInstallationPath) -> Result<(), ErrorMarker> {
	let (game, _) = fs_access::open_ambient_dir(game.as_path()).context(ErrorMarker::game_install_invalid())?;
	let (root_ancestor, root_exists) =
		fs_access::open_existing_ancestor(root.as_path()).context(ErrorMarker::environment_root_unsafe())?;

	if fs_access::is_ancestor_of(&game, &root_ancestor).context(ErrorMarker::environment_root_unsafe())? {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	if root_exists
		&& fs_access::is_ancestor_of(&root_ancestor, &game).context(ErrorMarker::environment_root_unsafe())?
	{
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::prove_separation;
	use domain::EnvironmentRoot;
	use domain::GameInstallationPath;
	use rootcause::Result;
	#[cfg(windows)]
	use std::ffi::OsString;
	use std::fs;
	use std::io::Error as IoError;
	use std::io::Result as IoResult;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	#[cfg(windows)]
	use std::os::windows::ffi::OsStrExt;
	#[cfg(windows)]
	use std::os::windows::ffi::OsStringExt;
	#[cfg(windows)]
	use std::os::windows::fs::symlink_dir as windows_symlink_dir;
	use std::path::Path;
	use std::path::PathBuf;
	use tempfile::TempDir;

	#[test]
	fn rejects_missing_environment_root_inside_game() -> Result<()> {
		let (_temp, game) = fixture()?;
		let root = EnvironmentRoot::new(game.join("mods-environment"))?;
		let path = GameInstallationPath::new(game)?;
		assert!(prove_separation(&root, &path).is_err());
		Ok(())
	}

	#[test]
	fn rejects_existing_environment_root_inside_game() -> Result<()> {
		let (_temp, game) = fixture()?;
		let environment = game.join("mods-environment");
		fs::create_dir(&environment)?;
		let root = EnvironmentRoot::new(environment)?;
		let path = GameInstallationPath::new(game)?;
		assert!(prove_separation(&root, &path).is_err());
		Ok(())
	}

	#[test]
	fn rejects_game_inside_environment_root() -> Result<()> {
		let (temp, game) = fixture()?;
		let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?)?;
		let path = GameInstallationPath::new(game)?;
		assert!(prove_separation(&root, &path).is_err());
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn rejects_symlinked_environment_root_during_containment_proof() -> Result<()> {
		let (_fixture, game) = fixture()?;
		let real_root = game.join("real-environment");
		fs::create_dir(&real_root)?;
		let holder = TempDir::new()?;
		let linked_root = holder.path().join("linked-environment");
		symlink_dir(&real_root, &linked_root)?;
		let root = EnvironmentRoot::new(linked_root)?;
		let path = GameInstallationPath::new(game)?;
		assert!(prove_separation(&root, &path).is_err());
		Ok(())
	}

	#[cfg(windows)]
	#[test]
	fn rejects_case_alias_of_environment_ancestor() -> Result<()> {
		let (temp, game) = fixture()?;
		let alias = alternate_ascii_case(&fs::canonicalize(temp.path())?);
		assert!(alias.exists());
		let root = EnvironmentRoot::new(alias)?;
		let path = GameInstallationPath::new(game)?;
		assert!(prove_separation(&root, &path).is_err());
		Ok(())
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

	#[cfg(unix)]
	fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
		symlink(source, destination)
	}

	#[cfg(windows)]
	fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
		windows_symlink_dir(source, destination)
	}

	#[cfg(windows)]
	fn alternate_ascii_case(path: &Path) -> PathBuf {
		let units = path.as_os_str().encode_wide().map(|unit| match unit {
			97..=122 => unit - 32,
			65..=90 => unit + 32,
			_ => unit,
		});
		PathBuf::from(OsString::from_wide(&units.collect::<Vec<_>>()))
	}
}
