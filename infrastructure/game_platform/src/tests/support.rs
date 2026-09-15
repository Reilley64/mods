use crate::GamePlatformAdapter;
use crate::adapter::test_support::adapter;
use rootcause::Result;
use std::fs;
use std::io::Error as IoError;
use std::io::Result as IoResult;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(windows)]
use std::os::windows::fs::symlink_dir as windows_symlink_dir;
#[cfg(windows)]
use std::os::windows::fs::symlink_file as windows_symlink_file;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;

pub(super) fn fixture() -> Result<(TempDir, PathBuf)> {
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
		"\"AppState\"\n{\n\"appid\" \"22380\"\n\"buildid\" \"88\"\n\"installdir\" \"Fallout New Vegas\"\n}",
	)?;
	Ok((temp, game))
}

pub(super) fn adapter_without_sources() -> GamePlatformAdapter {
	adapter(Vec::new(), Vec::new(), PathBuf::new(), PathBuf::new())
}

pub(super) fn adapter_with_discovery(steam: Vec<PathBuf>, bethesda: Vec<PathBuf>) -> GamePlatformAdapter {
	adapter(steam, bethesda, PathBuf::new(), PathBuf::new())
}

pub(super) fn adapter_with_profiles(documents: PathBuf, local_app_data: PathBuf) -> GamePlatformAdapter {
	adapter(Vec::new(), Vec::new(), documents, local_app_data)
}

#[cfg(unix)]
pub(super) fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
	symlink(source, destination)
}

#[cfg(windows)]
pub(super) fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
	windows_symlink_file(source, destination)
}

#[cfg(unix)]
pub(super) fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
	symlink(source, destination)
}

#[cfg(windows)]
pub(super) fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
	windows_symlink_dir(source, destination)
}
