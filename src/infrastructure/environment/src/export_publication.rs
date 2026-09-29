use rootcause::Result;
#[cfg(windows)]
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::io;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::MOVE_FILE_FLAGS;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::MoveFileExW;
#[cfg(windows)]
use windows::core::PCWSTR;

#[cfg(windows)]
pub(crate) fn publish_no_replace(source: &Path, destination: &Path) -> Result<(), io::Error> {
	let source: Vec<_> = source.as_os_str().encode_wide().chain([0]).collect();
	let destination: Vec<_> = destination.as_os_str().encode_wide().chain([0]).collect();
	if source[..source.len() - 1].contains(&0) || destination[..destination.len() - 1].contains(&0) {
		return Err(report!(io::Error::new(
			io::ErrorKind::InvalidInput,
			"NUL in export path"
		)));
	}

	// SAFETY: both paths are NUL-terminated and live for the call. Zero flags
	// deliberately excludes replacement and cross-volume copy fallback.
	unsafe {
		MoveFileExW(
			PCWSTR(source.as_ptr()),
			PCWSTR(destination.as_ptr()),
			MOVE_FILE_FLAGS(0),
		)
	}
	.map_err(io::Error::other)
	.into_report()
}

#[cfg(not(windows))]
pub(crate) fn publish_no_replace(_source: &Path, _destination: &Path) -> Result<(), io::Error> {
	Err(report!(io::Error::new(
		io::ErrorKind::Unsupported,
		"export publication requires Windows"
	)))
}

#[cfg(all(test, windows))]
mod tests {
	use super::*;
	use std::fs;
	use tempfile::TempDir;

	#[test]
	fn publication_never_replaces_a_racing_destination() -> Result<(), io::Error> {
		let temp = TempDir::new().into_report()?;
		let stage = temp.path().join("stage");
		let output = temp.path().join("output");
		fs::create_dir(&stage).into_report()?;
		fs::write(stage.join("payload"), b"staged").into_report()?;
		fs::create_dir(&output).into_report()?;
		assert!(publish_no_replace(&stage, &output).is_err());
		assert!(stage.join("payload").is_file());
		assert!(!output.join("payload").exists());
		fs::remove_dir(&output).into_report()?;
		publish_no_replace(&stage, &output)?;
		assert!(!stage.exists());
		assert_eq!(fs::read(output.join("payload")).into_report()?, b"staged");
		Ok(())
	}
}
