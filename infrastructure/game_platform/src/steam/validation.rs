use super::io::read_required_text;
use super::manifest;
use crate::fs_access;
use application::ErrorMarker;
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
	Ok(GameBinding::new(game_path, build))
}

fn has_name(path: &Path, expected: &str) -> bool {
	path.file_name()
		.is_some_and(|name| os_eq_ignore_ascii_case(name, expected))
}

fn os_eq_ignore_ascii_case(value: &OsStr, expected: &str) -> bool {
	value.to_str().is_some_and(|value| value.eq_ignore_ascii_case(expected))
}
