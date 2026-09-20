use super::io::read_required_text;
use super::manifest;
use crate::fs_access;
use application::ErrorMarker;
use cap_std::fs::Dir;
use domain::GameBinding;
use domain::GameInstallationPath;
use domain::SteamBuildId;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::Path;

pub(crate) fn validate(path: &Path) -> Result<GameBinding, ErrorMarker> {
	Ok(open_validated(path)?.binding)
}

pub(crate) fn reopen(expected: &GameBinding) -> Result<Dir, ErrorMarker> {
	let validated = open_validated(expected.game_directory().as_path())?;
	if validated.binding.observed_build_id() != expected.observed_build_id() {
		return Err(report!(ErrorMarker::game_build_mismatch(
			expected.observed_build_id().get(),
			validated.binding.observed_build_id().get(),
		)));
	}
	Ok(validated.directory)
}

struct ValidatedGameInstallation {
	binding: GameBinding,
	directory: Dir,
}

fn open_validated(path: &Path) -> Result<ValidatedGameInstallation, ErrorMarker> {
	let game_name = path
		.file_name()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	let common_path = path
		.parent()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	if !has_name(common_path, "common") {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	let steamapps_path = common_path
		.parent()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	if !has_name(steamapps_path, "steamapps") {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}

	let (steamapps, _) = fs_access::open_ambient_dir(steamapps_path).context(ErrorMarker::game_install_invalid())?;
	let common_name = common_path
		.file_name()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	let common =
		fs_access::open_dir(&steamapps, Path::new(common_name)).context(ErrorMarker::game_install_invalid())?;
	let game = fs_access::open_dir(&common, Path::new(game_name)).map_err(|error| {
		let marker = if error.current_context().kind() == ErrorKind::NotFound {
			ErrorMarker::game_install_not_found()
		} else {
			ErrorMarker::game_install_invalid()
		};
		error.context(marker)
	})?;
	let (named_game, canonical_game) =
		fs_access::open_ambient_dir(path).context(ErrorMarker::game_install_invalid())?;
	if !fs_access::same_dir(&game, &named_game).context(ErrorMarker::game_install_invalid())? {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}

	fs_access::open_regular(&game, Path::new("FalloutNV.exe")).context(ErrorMarker::game_install_invalid())?;
	fs_access::open_regular(&game, Path::new("Fallout_default.ini")).context(ErrorMarker::game_install_invalid())?;
	let data = fs_access::open_dir(&game, Path::new("Data")).context(ErrorMarker::game_install_invalid())?;
	for entry in data.entries().context(ErrorMarker::game_install_invalid())? {
		let entry = entry.context(ErrorMarker::game_install_invalid())?;
		if os_eq_ignore_ascii_case(&entry.file_name(), "Fallout - Invalidation.bsa") {
			return Err(report!(ErrorMarker::game_install_invalid()));
		}
	}

	let text = read_required_text(&steamapps, Path::new("appmanifest_22380.acf"))?;
	let (app_id, install_dir, build) = manifest::fields(&text).context(ErrorMarker::game_install_invalid())?;
	if app_id != 22_380
		|| !manifest::is_install_directory_name(&install_dir)
		|| !os_eq_ignore_ascii_case(game_name, &install_dir)
	{
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	let build = SteamBuildId::new(build).context(ErrorMarker::game_install_invalid())?;
	let game_path = GameInstallationPath::new(canonical_game).context(ErrorMarker::game_install_invalid())?;
	Ok(ValidatedGameInstallation {
		binding: GameBinding::new(game_path, build),
		directory: game,
	})
}

fn has_name(path: &Path, expected: &str) -> bool {
	path.file_name()
		.is_some_and(|name| os_eq_ignore_ascii_case(name, expected))
}

fn os_eq_ignore_ascii_case(value: &OsStr, expected: &str) -> bool {
	value.to_str().is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests {
	use super::validate;
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

	#[test]
	fn validates_structured_manifest_executable_and_nonzero_build() -> Result<()> {
		let (_temp, game) = game_fixture(42)?;
		assert_eq!(validate(&game)?.observed_build_id().get(), 42);
		Ok(())
	}

	#[test]
	fn rejects_reserved_base_archive_zero_build_and_stray_manifest_keys() -> Result<()> {
		let (_temp, game) = game_fixture(0)?;
		assert!(validate(&game).is_err());
		let manifest = game
			.parent()
			.and_then(Path::parent)
			.ok_or_else(|| IoError::other("missing steamapps"))?
			.join("appmanifest_22380.acf");
		fs::write(
			&manifest,
			"\"appid\" \"22380\"\n\"buildid\" \"42\"\n\"installdir\" \"Fallout New Vegas\"",
		)?;
		assert!(validate(&game).is_err());
		fs::write(
			&manifest,
			"\"AppState\" { \"appid\" \"22380\" \"buildid\" \"42\" \"installdir\" \"Fallout New Vegas\" }",
		)?;
		fs::write(game.join("Data/Fallout - Invalidation.bsa"), b"conflict")?;
		assert!(validate(&game).is_err());
		Ok(())
	}

	#[test]
	fn rejects_manifest_outside_a_steamapps_parent() -> Result<()> {
		let (temp, game) = game_fixture(42)?;
		let arbitrary = temp.path().join("arbitrary");
		fs::rename(temp.path().join("steamapps"), &arbitrary)?;
		let relocated = arbitrary
			.join("common")
			.join(game.file_name().ok_or_else(|| IoError::other("missing game name"))?);
		assert!(validate(&relocated).is_err());
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn rejects_symlinked_game_files_directories_and_manifest() -> Result<()> {
		let (_temp, game) = game_fixture(42)?;
		let executable = game.join("FalloutNV.exe");
		let executable_target = game.join("FalloutNV.real.exe");
		fs::rename(&executable, &executable_target)?;
		symlink_file(&executable_target, &executable)?;
		assert!(validate(&game).is_err());

		let (_temp, game) = game_fixture(42)?;
		let data = game.join("Data");
		let data_target = game.join("RealData");
		fs::rename(&data, &data_target)?;
		symlink_dir(&data_target, &data)?;
		assert!(validate(&game).is_err());

		let (_temp, game) = game_fixture(42)?;
		let manifest = game
			.parent()
			.and_then(Path::parent)
			.ok_or_else(|| IoError::other("missing steamapps"))?
			.join("appmanifest_22380.acf");
		let manifest_target = manifest.with_extension("real.acf");
		fs::rename(&manifest, &manifest_target)?;
		symlink_file(&manifest_target, &manifest)?;
		assert!(validate(&game).is_err());
		Ok(())
	}

	#[cfg(any(unix, windows))]
	#[test]
	fn rejects_symlinked_game_ancestor_and_directory() -> Result<()> {
		let (fixture, game) = game_fixture(42)?;
		let holder = TempDir::new()?;
		let linked_ancestor = holder.path().join("linked-library");
		symlink_dir(fixture.path(), &linked_ancestor)?;
		assert!(validate(&linked_ancestor.join("steamapps/common/Fallout New Vegas")).is_err());
		drop(game);

		let (_temp, game) = game_fixture(42)?;
		let target = game.with_file_name("Real Fallout New Vegas");
		fs::rename(&game, &target)?;
		symlink_dir(&target, &game)?;
		assert!(validate(&game).is_err());
		Ok(())
	}

	fn game_fixture(build: u64) -> Result<(TempDir, PathBuf)> {
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
	fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
		symlink(source, destination)
	}

	#[cfg(windows)]
	fn symlink_file(source: &Path, destination: &Path) -> IoResult<()> {
		windows_symlink_file(source, destination)
	}

	#[cfg(unix)]
	fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
		symlink(source, destination)
	}

	#[cfg(windows)]
	fn symlink_dir(source: &Path, destination: &Path) -> IoResult<()> {
		windows_symlink_dir(source, destination)
	}
}
