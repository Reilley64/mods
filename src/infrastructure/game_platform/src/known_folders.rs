use application::ErrorMarker;
use rootcause::Result;
#[cfg(windows)]
use rootcause::prelude::ResultExt;
#[cfg(windows)]
use std::ffi::OsString;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
#[cfg(windows)]
use std::slice::from_raw_parts;
#[cfg(windows)]
use windows::Win32::System::Com::CoTaskMemFree;
#[cfg(windows)]
use windows::Win32::UI::Shell::FOLDERID_Documents;
#[cfg(windows)]
use windows::Win32::UI::Shell::FOLDERID_LocalAppData;
#[cfg(windows)]
use windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG;
#[cfg(windows)]
use windows::Win32::UI::Shell::SHGetKnownFolderPath;
#[cfg(windows)]
use windows::core::GUID;

#[derive(Debug, Clone, Default)]
pub(crate) struct KnownFolderPaths {
	pub(crate) documents: PathBuf,
	pub(crate) local_app_data: PathBuf,
}

#[cfg(windows)]
pub(crate) fn current() -> Result<KnownFolderPaths, ErrorMarker> {
	Ok(KnownFolderPaths {
		documents: known_folder(&FOLDERID_Documents)?,
		local_app_data: known_folder(&FOLDERID_LocalAppData)?,
	})
}

#[cfg(windows)]
fn known_folder(id: *const GUID) -> Result<PathBuf, ErrorMarker> {
	// SAFETY: `id` refers to a static known-folder GUID, the optional access token is absent,
	// and the returned allocation remains owned by this function until `CoTaskMemFree` below.
	let pointer = unsafe { SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None) }
		.context(ErrorMarker::game_install_invalid())?;
	let path = {
		// SAFETY: On success, `SHGetKnownFolderPath` returns a non-null, suitably aligned CoTaskMem
		// allocation containing initialized UTF-16 code units through a terminating NUL. The scan
		// therefore stays in that allocation. Windows path limits keep its byte length below
		// `isize::MAX`. `length` excludes the terminator, and the resulting shared slice is used only
		// to copy the path before the allocation is freed; no mutation occurs during that lifetime.
		let slice = unsafe {
			let mut length = 0usize;
			while *pointer.0.add(length) != 0 {
				length += 1;
			}
			from_raw_parts(pointer.0, length)
		};
		PathBuf::from(OsString::from_wide(slice))
	};
	// SAFETY: `pointer` is the allocation returned by `SHGetKnownFolderPath`, is still live, and is
	// released exactly once with its required allocator after the copied result no longer borrows it.
	unsafe { CoTaskMemFree(Some(pointer.0.cast())) };
	Ok(path)
}

#[cfg(not(windows))]
pub(crate) fn current() -> Result<KnownFolderPaths, ErrorMarker> {
	Ok(KnownFolderPaths::default())
}

#[cfg(all(test, windows))]
mod tests {
	use super::current;
	use rootcause::Result;

	#[test]
	fn windows_known_folder_api_returns_absolute_profile_roots() -> Result<()> {
		let folders = current()?;
		assert!(folders.documents.is_absolute());
		assert!(folders.local_app_data.is_absolute());
		Ok(())
	}
}
