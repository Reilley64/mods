use crate::fs_access;
use application::ErrorMarker;
use cap_std::fs::Dir;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::io::ErrorKind;
use std::io::Read;
use std::path::Path;

pub(super) fn read_optional_text(directory: &Dir, path: &Path) -> Result<Option<String>, ErrorMarker> {
	let mut file = match fs_access::open_regular(directory, path) {
		Ok(file) => file,
		Err(error) if error.current_context().kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(error.context(ErrorMarker::game_install_invalid())),
	};
	let mut text = String::new();
	file.read_to_string(&mut text)
		.context(ErrorMarker::game_install_invalid())?;
	Ok(Some(text))
}

pub(super) fn read_required_text(directory: &Dir, path: &Path) -> Result<String, ErrorMarker> {
	read_optional_text(directory, path)?.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))
}
