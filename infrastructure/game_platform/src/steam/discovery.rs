use super::io::read_optional_text;
use super::libraries::libraries;
use super::manifest;
use super::validate;
use crate::cancellation::check_cancelled;
use crate::fs_access;
use application::ErrorMarker;
use domain::GameBinding;
use rootcause::Result;
use rootcause::report;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub(crate) fn discover(
	steam_roots: &[PathBuf],
	cancellation: &CancellationToken,
) -> Result<Option<GameBinding>, ErrorMarker> {
	let mut first_invalid = None;
	for steam_root in steam_roots {
		check_cancelled(cancellation)?;
		let libraries = match libraries(steam_root, cancellation) {
			Ok(libraries) => libraries,
			Err(error) => {
				if first_invalid.is_none() {
					first_invalid = Some(error);
				}
				continue;
			}
		};
		for library in libraries {
			check_cancelled(cancellation)?;
			let steamapps_path = library.join("steamapps");
			let steamapps = match fs_access::open_ambient_dir(&steamapps_path) {
				Ok((directory, _)) => directory,
				Err(error) if error.current_context().kind() == ErrorKind::NotFound => continue,
				Err(error) => {
					if first_invalid.is_none() {
						first_invalid =
							Some(error.context(ErrorMarker::game_install_invalid()));
					}
					continue;
				}
			};
			let text = match read_optional_text(&steamapps, Path::new("appmanifest_22380.acf")) {
				Ok(Some(text)) => text,
				Ok(None) => continue,
				Err(error) => {
					if first_invalid.is_none() {
						first_invalid = Some(error);
					}
					continue;
				}
			};
			let (_, install_dir, _) = match manifest::fields(&text) {
				Ok(fields) => fields,
				Err(error) => {
					if first_invalid.is_none() {
						first_invalid =
							Some(error.context(ErrorMarker::game_install_invalid()));
					}
					continue;
				}
			};
			if !manifest::is_install_directory_name(&install_dir) {
				if first_invalid.is_none() {
					first_invalid = Some(report!(ErrorMarker::game_install_invalid()));
				}
				continue;
			}
			let candidate = steamapps_path.join("common").join(install_dir);
			match validate(&candidate) {
				Ok(binding) => return Ok(Some(binding)),
				Err(error) if first_invalid.is_none() => first_invalid = Some(error),
				Err(_) => {}
			}
		}
	}
	match first_invalid {
		Some(error) => Err(error),
		None => Ok(None),
	}
}
