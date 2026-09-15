use cap_std::ambient_authority;
use cap_std::fs::Dir;
use cap_std::fs::File;
use cap_std::fs::Metadata;
#[cfg(windows)]
use cap_std::fs::MetadataExt as _;
use cap_std::fs::OpenOptions;
#[cfg(any(unix, windows))]
use cap_std::fs::OpenOptionsExt as _;
use rootcause::Result;
use rootcause::report;
use std::io::Error as IoError;
use std::path::Component;
use std::path::MAIN_SEPARATOR_STR;
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use windows::Win32::Foundation::GENERIC_WRITE;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE;

pub(crate) fn open_ambient_dir(path: &Path) -> Result<Dir, IoError> {
	let (mut directory, mut components) = open_filesystem_root(path)?;
	for component in &mut components {
		match component {
			Component::Normal(name) => {
				directory = open_dir(&directory, Path::new(name))?;
			}
			Component::CurDir => {}
			Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
				return Err(report!(IoError::other("path contains an unsafe component")));
			}
		}
	}
	Ok(directory)
}

fn open_filesystem_root(path: &Path) -> Result<(Dir, impl Iterator<Item = Component<'_>>), IoError> {
	let mut components = path.components();
	let mut root = PathBuf::new();
	match components.next() {
		#[cfg(windows)]
		Some(Component::Prefix(prefix)) => {
			root.push(prefix.as_os_str());
			match components.next() {
				Some(Component::RootDir) => root.push(Path::new(r"\")),
				_ => {
					return Err(report!(IoError::other("path is not absolute")));
				}
			}
		}
		Some(Component::RootDir) => root.push(Path::new(MAIN_SEPARATOR_STR)),
		_ => {
			return Err(report!(IoError::other("path is not absolute")));
		}
	}
	let file = File::open_ambient_with(&root, &directory_options(), ambient_authority())?;
	require_directory(&file.metadata()?)?;
	Ok((Dir::from_std_file(file.into_std()), components))
}

pub(crate) fn open_dir(dir: &Dir, path: &Path) -> Result<Dir, IoError> {
	let file = dir.open_with(path, &directory_options())?;
	require_directory(&file.metadata()?)?;
	Ok(Dir::from_std_file(file.into_std()))
}

pub(crate) fn open_regular(dir: &Dir, path: &Path) -> Result<File, IoError> {
	let file = dir.open_with(path, &regular_options(false))?;
	require_regular(&file.metadata()?)?;
	Ok(file)
}

pub(crate) fn create_dir(dir: &Dir, path: &Path) -> Result<Dir, IoError> {
	dir.create_dir(path)?;
	sync_dir(dir)?;
	open_dir(dir, path)
}

pub(crate) fn create_new_regular(dir: &Dir, path: &Path) -> Result<File, IoError> {
	let file = dir.open_with(path, &regular_options(true))?;
	require_regular(&file.metadata()?)?;
	Ok(file)
}

#[cfg(not(windows))]
pub(crate) fn sync_dir(dir: &Dir) -> Result<(), IoError> {
	dir.try_clone()?.into_std_file().sync_all()?;
	Ok(())
}

#[cfg(windows)]
pub(crate) fn sync_dir(dir: &Dir) -> Result<(), IoError> {
	let mut options = OpenOptions::new();
	options.access_mode(GENERIC_WRITE.0)
		.share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0);
	nofollow(&mut options, true);
	let file = dir.open_with(Path::new("."), &options)?;
	require_directory(&file.metadata()?)?;
	// FlushFileBuffers needs write access. This capability-relative handle makes a failed
	// flush observable, but Windows does not document it as a directory-entry durability barrier.
	file.into_std().sync_all()?;
	Ok(())
}

fn regular_options(create_new: bool) -> OpenOptions {
	let mut options = OpenOptions::new();
	options.read(true).write(create_new).create_new(create_new);
	nofollow(&mut options, false);
	options
}

fn directory_options() -> OpenOptions {
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

fn require_directory(metadata: &Metadata) -> Result<(), IoError> {
	if metadata.is_dir() && !is_reparse(metadata) {
		Ok(())
	} else {
		Err(report!(IoError::other("path is not a safe directory")))
	}
}

#[cfg(windows)]
pub(crate) fn is_reparse(metadata: &Metadata) -> bool {
	metadata.file_type().is_symlink() || attributes_are_reparse(metadata.file_attributes())
}

#[cfg(windows)]
fn attributes_are_reparse(attributes: u32) -> bool {
	attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
pub(crate) fn is_reparse(metadata: &Metadata) -> bool {
	metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use tempfile::TempDir;

	#[test]
	fn directory_sync_keeps_the_original_capability_usable() -> Result<(), IoError> {
		let fixture = TempDir::new()?;
		fs::write(fixture.path().join("before"), b"before")?;
		let directory = Dir::open_ambient_dir(fixture.path(), ambient_authority())?;

		sync_dir(&directory)?;

		open_regular(&directory, Path::new("before"))?;
		Ok(())
	}

	#[cfg(windows)]
	#[test]
	fn raw_reparse_attribute_is_rejected() {
		assert!(attributes_are_reparse(FILE_ATTRIBUTE_REPARSE_POINT.0));
		assert!(!attributes_are_reparse(0));
	}
}
