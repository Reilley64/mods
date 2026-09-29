use application::ErrorMarker;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio_util::sync::CancellationToken;

/// Reads a file, or returns `None` when it does not exist.
pub(crate) async fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, io::Error> {
	match read(path).await {
		Ok(contents) => Ok(Some(contents)),
		Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(report!(error)),
	}
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
