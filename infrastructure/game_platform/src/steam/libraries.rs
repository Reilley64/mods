use super::io::read_optional_text;
use super::model::Value;
use super::model::exactly_one_object;
use super::model::exactly_one_text;
use super::parser::parse_key_values;
use crate::fs_access;
use application::ErrorMarker;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub(super) fn libraries(steam_root: &Path, cancellation: &CancellationToken) -> Result<Vec<PathBuf>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mut paths = vec![steam_root.to_path_buf()];
	let (root, _) = match fs_access::open_ambient_dir(steam_root) {
		Ok(opened) => opened,
		Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(paths),
		Err(error) => return Err(error.context(ErrorMarker::game_install_invalid())),
	};
	let steamapps = match fs_access::open_dir(&root, Path::new("steamapps")) {
		Ok(directory) => directory,
		Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(paths),
		Err(error) => return Err(error.context(ErrorMarker::game_install_invalid())),
	};
	let Some(text) = read_optional_text(&steamapps, Path::new("libraryfolders.vdf"))? else {
		return Ok(paths);
	};
	let parsed = parse_key_values(&text).context(ErrorMarker::game_install_invalid())?;
	let folders = exactly_one_object(&parsed, "libraryfolders")
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	for entry in folders {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let Value::Object(properties) = &entry.value else {
			continue;
		};
		let Some(path) = exactly_one_text(properties, "path") else {
			continue;
		};
		let path = PathBuf::from(path);
		if !paths.iter().any(|existing| existing == &path) {
			paths.push(path);
		}
	}
	Ok(paths)
}
