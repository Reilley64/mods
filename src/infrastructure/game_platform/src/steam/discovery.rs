use super::io::read_optional_text;
use super::libraries::libraries;
use super::manifest;
use super::validate;
use crate::fs_access;
use application::ErrorCode;
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
	discover_with_before_libraries(|_| {}, steam_roots, cancellation)
}

fn discover_with_before_libraries(
	mut before_libraries: impl FnMut(&Path),
	steam_roots: &[PathBuf],
	cancellation: &CancellationToken,
) -> Result<Option<GameBinding>, ErrorMarker> {
	let mut first_invalid = None;
	for steam_root in steam_roots {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		before_libraries(steam_root);
		let libraries = match libraries(steam_root, cancellation) {
			Ok(libraries) => libraries,
			Err(error) if error.current_context().code() == ErrorCode::OperationCancelled => {
				return Err(error);
			}
			Err(error) => {
				if first_invalid.is_none() {
					first_invalid = Some(error);
				}
				continue;
			}
		};
		for library in libraries {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

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
			let (_, install_dir) = match manifest::fields(&text) {
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
	if let Some(error) = first_invalid {
		return Err(error);
	}
	Ok(None)
}

#[cfg(test)]
mod tests {
	use super::discover;
	use super::discover_with_before_libraries;
	use application::ErrorCode;
	use rootcause::Result;
	use std::fs;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn invalid_root_does_not_mask_later_library_discovery_cancellation() -> Result<()> {
		let temp = TempDir::new()?;
		let invalid_root = temp.path().join("invalid-root");
		fs::write(&invalid_root, b"not a directory")?;
		let cancelling_root = temp.path().join("cancelling-root");
		let steam_roots = [invalid_root, cancelling_root.clone()];

		let without_cancellation = discover(&steam_roots, &CancellationToken::new());
		assert_eq!(
			without_cancellation
				.as_ref()
				.err()
				.map(|error| error.current_context().code()),
			Some(ErrorCode::GameInstallInvalid),
		);

		let cancellation = CancellationToken::new();
		let result = discover_with_before_libraries(
			|steam_root| {
				if steam_root == cancelling_root.as_path() {
					cancellation.cancel();
				}
			},
			&steam_roots,
			&cancellation,
		);
		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::OperationCancelled),
		);
		Ok(())
	}
}
