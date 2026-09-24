use cap_std::fs::Dir;
use cap_std::fs::File;
use cap_std::fs::Metadata;
#[cfg(windows)]
use cap_std::fs::MetadataExt;
use cap_std::fs::OpenOptions;
#[cfg(any(unix, windows))]
use cap_std::fs::OpenOptionsExt;
use rootcause::Result;
use rootcause::report;
use std::io::Error as IoError;
use std::path::Path;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

pub(crate) fn open_dir(dir: &Dir, path: &Path) -> Result<Dir, IoError> {
	let file = dir.open_with(path, &directory_options())?;
	require_directory(&file.metadata()?)?;
	Ok(Dir::from_std_file(file.into_std()))
}

pub(crate) fn open_regular(dir: &Dir, path: &Path) -> Result<File, IoError> {
	let file = dir.open_with(path, &regular_options())?;
	require_regular(&file.metadata()?)?;
	Ok(file)
}

fn regular_options() -> OpenOptions {
	let mut options = OpenOptions::new();
	options.read(true);
	nofollow(&mut options, false);
	options
}

pub(super) fn directory_options() -> OpenOptions {
	let mut options = OpenOptions::new();
	options.read(true);
	nofollow(&mut options, true);
	options
}

#[cfg(unix)]
fn nofollow(options: &mut OpenOptions, directory: bool) {
	let flags = libc::O_NOFOLLOW | if directory { libc::O_DIRECTORY } else { 0 };
	options.custom_flags(flags);
}

#[cfg(windows)]
fn nofollow(options: &mut OpenOptions, directory: bool) {
	let mut flags = FILE_FLAG_OPEN_REPARSE_POINT.0;
	if directory {
		flags |= FILE_FLAG_BACKUP_SEMANTICS.0;
	}
	options.custom_flags(flags);
}

#[cfg(not(any(unix, windows)))]
fn nofollow(_options: &mut OpenOptions, _directory: bool) {}

fn require_regular(metadata: &Metadata) -> Result<(), IoError> {
	if metadata.is_file() && !is_reparse(metadata) {
		Ok(())
	} else {
		Err(report!(IoError::other("path is not a safe regular file")))
	}
}

pub(super) fn require_directory(metadata: &Metadata) -> Result<(), IoError> {
	if metadata.is_dir() && !is_reparse(metadata) {
		Ok(())
	} else {
		Err(report!(IoError::other("path is not a safe directory")))
	}
}

#[cfg(windows)]
fn is_reparse(metadata: &Metadata) -> bool {
	metadata.file_type().is_symlink() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &Metadata) -> bool {
	metadata.file_type().is_symlink()
}
