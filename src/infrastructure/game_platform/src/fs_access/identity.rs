use cap_std::ambient_authority;
use cap_std::fs::Dir;
use rootcause::Result;
#[cfg(windows)]
use rootcause::report;
use std::io::Error as IoError;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::GetFileInformationByHandle;

pub(crate) fn same_dir(left: &Dir, right: &Dir) -> Result<bool, IoError> {
	Ok(identity(left)? == identity(right)?)
}

pub(crate) fn is_ancestor_of(ancestor: &Dir, child: &Dir) -> Result<bool, IoError> {
	let ancestor = identity(ancestor)?;
	let mut current = child.try_clone()?;
	loop {
		let current_identity = identity(&current)?;
		if ancestor == current_identity {
			return Ok(true);
		}
		let parent = current.open_parent_dir(ambient_authority())?;
		if identity(&parent)? == current_identity {
			return Ok(false);
		}
		current = parent;
	}
}

#[cfg(unix)]
fn identity(dir: &Dir) -> Result<(u64, u64), IoError> {
	let metadata = dir.try_clone()?.into_std_file().metadata()?;
	Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn identity(dir: &Dir) -> Result<(u32, u32, u32), IoError> {
	let file = dir.try_clone()?.into_std_file();
	let mut information = BY_HANDLE_FILE_INFORMATION::default();
	// SAFETY: `file` owns a valid directory handle for the call's duration. `information` is valid,
	// aligned writable storage of the required type, and the API initializes it before success.
	unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
		.map_err(|error| report!(error.clone()).context(IoError::other(error)))?;
	Ok((
		information.dwVolumeSerialNumber,
		information.nFileIndexHigh,
		information.nFileIndexLow,
	))
}

#[cfg(not(any(unix, windows)))]
fn identity(_dir: &Dir) -> Result<(), IoError> {
	Ok(())
}
