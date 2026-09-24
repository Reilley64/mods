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
	let mut first_invalid = None;
	for steam_root in steam_roots {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

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
	if let Some(error) = first_invalid {
		return Err(error);
	}
	Ok(None)
}

#[cfg(test)]
mod tests {
	use super::discover;
	use super::libraries;
	use application::ErrorCode;
	use rootcause::Result;
	use std::fs;
	use std::io::Error as IoError;
	use std::thread;
	use std::time::Instant;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn invalid_root_does_not_mask_later_library_discovery_cancellation() -> Result<()> {
		let temp = TempDir::new()?;
		let temp_root = fs::canonicalize(temp.path())?;
		let invalid_root = temp_root.join("invalid-root");
		fs::write(&invalid_root, b"not a directory")?;

		let cancelling_root = temp_root.join("cancelling-root");
		fs::create_dir_all(cancelling_root.join("steamapps"))?;
		let entry = "\"1\"\n{\n\"path\" \"/unused\"\n}\n";
		fs::write(
			cancelling_root.join("steamapps/libraryfolders.vdf"),
			format!("\"libraryfolders\"\n{{\n{}\n}}\n", entry.repeat(100_000)),
		)?;

		let started = Instant::now();
		libraries(&cancelling_root, &CancellationToken::new())?;
		let cancellation_delay = started.elapsed() / 2;
		let cancellation = CancellationToken::new();
		let cancellation_for_thread = cancellation.clone();
		let canceller = thread::spawn(move || {
			thread::sleep(cancellation_delay);
			cancellation_for_thread.cancel();
		});

		let result = discover(&[invalid_root, cancelling_root], &cancellation);
		canceller
			.join()
			.map_err(|_| IoError::other("cancellation thread panicked"))?;

		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::OperationCancelled),
		);
		Ok(())
	}
}
