use crate::error::ArchiveError;
use rootcause::Result;
use rootcause::report;

const FILE_ATTRIBUTE_DEVICE: u64 = 0x40;
const FILE_ATTRIBUTE_REPARSE_POINT: u64 = 0x400;
const UNIX_FILE_TYPE_MASK: u64 = 0o170_000;
const UNIX_REGULAR_FILE: u64 = 0o100_000;
const UNIX_DIRECTORY: u64 = 0o040_000;

pub(crate) fn validate_entry_kind(
	is_directory: bool,
	windows_attributes: Option<u64>,
	unix_mode: Option<u64>,
) -> Result<(), ArchiveError> {
	if windows_attributes
		.is_some_and(|attributes| attributes & (FILE_ATTRIBUTE_DEVICE | FILE_ATTRIBUTE_REPARSE_POINT) != 0)
	{
		return Err(report!(ArchiveError::UnsafeEntryKind));
	}

	let Some(unix_mode) = unix_mode else {
		return Ok(());
	};
	let file_type = unix_mode & UNIX_FILE_TYPE_MASK;
	let expected = if is_directory {
		UNIX_DIRECTORY
	} else {
		UNIX_REGULAR_FILE
	};
	if file_type != 0 && file_type != expected {
		return Err(report!(ArchiveError::UnsafeEntryKind));
	}
	Ok(())
}
