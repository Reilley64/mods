use crate::error::ArchiveError;
use crate::limits::MAX_ARCHIVE_BYTES;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::fs::File;
use std::fs::Metadata;
use std::fs::OpenOptions;
use std::io::Error as IoError;
use std::io::Result as IoResult;
use std::io::Seek;
use std::io::SeekFrom;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::Path;
#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::GetFileInformationByHandle;

pub(crate) struct SourceFile {
	file: File,
}

impl SourceFile {
	pub(crate) fn open(path: &Path) -> Result<Self, ArchiveError> {
		let file = open_without_following(path).map_err(map_open_error)?;
		let metadata = file.metadata().context(ArchiveError::Io)?;
		if !metadata.file_type().is_file()
			|| metadata.len() > MAX_ARCHIVE_BYTES
			|| link_count(&file, &metadata)? != 1
		{
			return Err(report!(ArchiveError::UnsafePath));
		}

		Ok(Self { file })
	}

	pub(crate) fn duplicate(&self) -> Result<File, ArchiveError> {
		let mut file = self.file.try_clone().context(ArchiveError::Io)?;
		file.seek(SeekFrom::Start(0)).context(ArchiveError::Io)?;
		Ok(file)
	}

	pub(crate) fn len(&self) -> Result<u64, ArchiveError> {
		self.file
			.metadata()
			.context(ArchiveError::Io)
			.map(|metadata| metadata.len())
	}
}

impl Debug for SourceFile {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
		formatter.debug_struct("SourceFile").finish_non_exhaustive()
	}
}

fn map_open_error(error: IoError) -> Report<ArchiveError> {
	let context = if is_no_follow_error(&error) {
		ArchiveError::UnsafePath
	} else {
		ArchiveError::Io
	};
	report!(error).context(context)
}

#[cfg(unix)]
fn is_no_follow_error(error: &IoError) -> bool {
	#[cfg(any(target_os = "linux", target_os = "android"))]
	const TOO_MANY_LINKS: i32 = 40;
	#[cfg(any(
		target_os = "macos",
		target_os = "ios",
		target_os = "freebsd",
		target_os = "openbsd",
		target_os = "netbsd",
		target_os = "dragonfly"
	))]
	const TOO_MANY_LINKS: i32 = 62;
	#[cfg(any(target_os = "solaris", target_os = "illumos"))]
	const TOO_MANY_LINKS: i32 = 90;
	#[cfg(not(any(
		target_os = "linux",
		target_os = "android",
		target_os = "macos",
		target_os = "ios",
		target_os = "freebsd",
		target_os = "openbsd",
		target_os = "netbsd",
		target_os = "dragonfly",
		target_os = "solaris",
		target_os = "illumos"
	)))]
	const TOO_MANY_LINKS: i32 = -1;

	error.raw_os_error() == Some(TOO_MANY_LINKS)
}

#[cfg(windows)]
fn is_no_follow_error(_error: &IoError) -> bool {
	false
}

#[cfg(unix)]
fn open_without_following(path: &Path) -> IoResult<File> {
	#[cfg(any(
		target_os = "linux",
		target_os = "android",
		target_os = "solaris",
		target_os = "illumos"
	))]
	const NO_FOLLOW: i32 = 0x20_000;
	#[cfg(not(any(
		target_os = "linux",
		target_os = "android",
		target_os = "solaris",
		target_os = "illumos"
	)))]
	const NO_FOLLOW: i32 = 0x100;

	OpenOptions::new().read(true).custom_flags(NO_FOLLOW).open(path)
}

#[cfg(unix)]
fn link_count(_file: &File, metadata: &Metadata) -> Result<u64, ArchiveError> {
	Ok(metadata.nlink())
}

#[cfg(windows)]
fn open_without_following(path: &Path) -> IoResult<File> {
	OpenOptions::new()
		.read(true)
		.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
		.open(path)
}

#[cfg(windows)]
fn link_count(file: &File, metadata: &Metadata) -> Result<u64, ArchiveError> {
	if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
		return Err(report!(ArchiveError::UnsafePath));
	}

	let mut information = BY_HANDLE_FILE_INFORMATION::default();
	// SAFETY: `file` owns a valid kernel handle for the call's duration. `information` is valid,
	// aligned writable storage of the required type, and the API initializes it before success.
	unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
		.map_err(|error| report!(error.clone()).context(ArchiveError::Io))?;
	Ok(u64::from(information.nNumberOfLinks))
}

#[cfg(test)]
mod tests {
	use super::SourceFile;
	#[cfg(unix)]
	use crate::error::ArchiveError;
	use std::error::Error;
	#[cfg(unix)]
	use std::fs::File;
	use std::fs::hard_link;
	use std::fs::rename;
	use std::fs::write;
	#[cfg(unix)]
	use std::io::Error as IoError;
	use std::io::Read;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	use std::result::Result as StdResult;
	use tempfile::TempDir;

	type TestResult<T = ()> = StdResult<T, Box<dyn Error + Send + Sync>>;

	#[test]
	fn opened_handle_keeps_its_identity_when_the_path_is_replaced() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("archive.zip");
		let moved = temp.path().join("original.zip");
		write(&path, b"original")?;
		let source = SourceFile::open(&path)?;
		rename(&path, moved)?;
		write(&path, b"replacement")?;

		let mut duplicate = source.duplicate()?;
		let mut contents = String::new();
		duplicate.read_to_string(&mut contents)?;
		assert_eq!(contents, "original");
		Ok(())
	}

	#[test]
	fn rejects_a_source_with_multiple_hard_links() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("archive.zip");
		write(&path, b"archive")?;
		hard_link(&path, temp.path().join("alias.zip"))?;
		assert!(SourceFile::open(&path).is_err());
		Ok(())
	}

	#[cfg(unix)]
	#[test]
	fn rejects_a_final_component_symlink() -> TestResult {
		let temp = TempDir::new()?;
		let target = temp.path().join("target.zip");
		let link = temp.path().join("archive.zip");
		File::create(&target)?;
		symlink(target, &link)?;
		let Err(error) = SourceFile::open(&link) else {
			return Err("final symlink was accepted".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::UnsafePath);
		assert!(error
			.iter_reports()
			.any(|node| node.downcast_current_context::<IoError>().is_some()));
		Ok(())
	}
}
