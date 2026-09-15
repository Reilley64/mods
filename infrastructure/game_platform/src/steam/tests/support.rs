use rootcause::Result;
use std::fs;
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

pub(super) fn game_fixture(build: u64) -> Result<(TempDir, PathBuf)> {
	let temp = TempDir::new()?;
	let steamapps = fs::canonicalize(temp.path())?.join("steamapps");
	let game = steamapps.join("common/Fallout New Vegas");
	fs::create_dir_all(game.join("Data"))?;
	fs::write(game.join("FalloutNV.exe"), b"fixture")?;
	fs::write(game.join("Fallout_default.ini"), b"[Archive]\n")?;
	fs::write(
		steamapps.join("appmanifest_22380.acf"),
		format!(
			concat!(
				"\"AppState\"\n{{\n\"appid\" \"22380\"\n",
				"\"buildid\" \"{}\"\n",
				"\"installdir\" \"Fallout New Vegas\"\n}}",
			),
			build,
		),
	)?;
	Ok((temp, game))
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
