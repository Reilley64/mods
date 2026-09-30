use application::ErrorMarker;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::time::SystemTime;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;

/// Reads a file, or returns `None` when it does not exist.
pub(crate) async fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, io::Error> {
	match read(path).await {
		Ok(contents) => Ok(Some(contents)),
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(report!(error)),
	}
}

/// Sets a file's modification time without write access to its contents.
///
/// Mod files and their copies can be read-only, so the file is opened only with
/// the right to change its times: `FILE_WRITE_ATTRIBUTES` on Windows, and a
/// read-only handle elsewhere, where the owner may set times without write access.
pub(crate) async fn set_modified(path: &Path, modified: SystemTime) -> Result<(), io::Error> {
	let path = path.to_owned();

	spawn_blocking(move || {
		let mut options = OpenOptions::new();
		#[cfg(windows)]
		options.access_mode(FILE_WRITE_ATTRIBUTES.0);
		#[cfg(not(windows))]
		options.read(true);

		options.open(&path)?.set_modified(modified)
	})
	.await
	.map_err(io::Error::other)?
	.map_err(|error| report!(error))
}

/// Requires the directory to contain only the allowed entry names.
pub(crate) async fn validate_exact_entries(
	directory: &Path,
	allowed: &[&str],
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	let mut remaining = allowed.iter().copied().collect::<HashSet<_>>();
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let name = entry.file_name();
		let text = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if !remaining.remove(text) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}
