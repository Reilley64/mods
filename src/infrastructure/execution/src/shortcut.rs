use crate::launch_inputs::encode_command_line;
use crate::usvfs::native_path_wide;
use application::ErrorMarker;
use application::shortcut::ShortcutDefinition;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::ffi::OsString;
use std::fs::canonicalize;
use std::fs::symlink_metadata;
use std::io::ErrorKind;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;
use windows::Win32::System::Com::CoCreateInstance;
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::CoUninitialize;
use windows::Win32::System::Com::IPersistFile;
use windows::Win32::System::Com::STGM_READ;
use windows::Win32::UI::Shell::FOLDERID_Desktop;
use windows::Win32::UI::Shell::IShellLinkW;
use windows::Win32::UI::Shell::KF_FLAG_DEFAULT;
use windows::Win32::UI::Shell::SHGetKnownFolderPath;
use windows::Win32::UI::Shell::ShellLink;
use windows::core::Interface;
use windows::core::PCWSTR;

struct ComApartment;
impl Drop for ComApartment {
	fn drop(&mut self) {
		// SAFETY: this guard is created only after successful initialization, stays
		// on this thread, and outlives every COM interface created by the operation.
		unsafe { CoUninitialize() };
	}
}

/// Prepares the complete Shell Link before atomically replacing a same-name link.
///
/// # Errors
/// Rejects non-link destinations, unsupported strings, COM failures, and failed publication.
pub fn persist_shortcut(definition: ShortcutDefinition) -> Result<(), ErrorMarker> {
	// SAFETY: reserved pointer is null; this synchronous function keeps all COM
	// interfaces and the matching uninitialization on the calling thread.
	unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
		.ok()
		.context(ErrorMarker::shortcut_failed())?;
	let _apartment = ComApartment;

	let destination = if let Some(destination) = definition.destination {
		destination
	} else {
		// SAFETY: the known-folder GUID is valid. Success transfers task-allocated
		// terminated UTF-16 storage, copied then freed exactly once below.
		let desktop = unsafe { SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None) }
			.context(ErrorMarker::shortcut_destination_invalid())?;
		// SAFETY: successful SHGetKnownFolderPath supplies a terminated string.
		let path = PathBuf::from(OsString::from_wide(unsafe { desktop.as_wide() }));
		// SAFETY: desktop is the exact allocation returned by the shell allocator.
		unsafe { CoTaskMemFree(Some(desktop.0.cast())) };
		path
	};
	let destination = canonicalize(destination).context(ErrorMarker::shortcut_destination_invalid())?;
	if !destination.is_dir() {
		return Err(report!(ErrorMarker::shortcut_destination_invalid()));
	}
	let final_path = destination.join(format!("{}.lnk", definition.name));

	let encoded: Vec<Vec<u16>> = definition
		.arguments
		.iter()
		.map(|value| value.encode_wide().collect())
		.collect();
	let mut arguments = encode_command_line(&encoded).context(ErrorMarker::shortcut_arguments_too_long())?;
	// IShellLink's argument field has a 1024 UTF-16-unit compatibility limit,
	// including the terminator; do not silently publish a truncated launch.
	if arguments.len() >= 1024 {
		return Err(report!(ErrorMarker::shortcut_arguments_too_long()));
	}
	arguments.push(0);
	let mut paths = Vec::new();
	for path in [&definition.launcher, &definition.working_directory, &definition.icon] {
		if !path.is_absolute() {
			return Err(report!(ErrorMarker::shortcut_launch_invalid()));
		}

		// Shell Link setters reject verbatim roots from canonicalize. The existing
		// native conversion also rejects components whose DOS meaning would change.
		paths.push(native_path_wide(path.as_os_str()).context(ErrorMarker::shortcut_launch_invalid())?);
	}

	// SAFETY: COM is initialized, no aggregation is requested, and the requested
	// interface belongs to the in-process ShellLink class.
	let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
		.context(ErrorMarker::shortcut_failed())?;
	// SAFETY: every input is NUL terminated and lives through its copying call.
	unsafe {
		link.SetPath(PCWSTR(paths[0].as_ptr()))
			.context(ErrorMarker::shortcut_failed())?;
		link.SetWorkingDirectory(PCWSTR(paths[1].as_ptr()))
			.context(ErrorMarker::shortcut_failed())?;
		link.SetIconLocation(PCWSTR(paths[2].as_ptr()), 0)
			.context(ErrorMarker::shortcut_failed())?;
		link.SetArguments(PCWSTR(arguments.as_ptr()))
	}
	.context(ErrorMarker::shortcut_failed())?;
	let persistence: IPersistFile = link.cast().context(ErrorMarker::shortcut_failed())?;

	let staged = NamedTempFile::new_in(&destination)
		.context(ErrorMarker::shortcut_failed())?
		.into_temp_path();
	let staged_path: Vec<_> = staged.as_os_str().encode_wide().chain([0]).collect();
	// SAFETY: staged_path is terminated and stable; Save copies it synchronously.
	unsafe { persistence.Save(PCWSTR(staged_path.as_ptr()), true) }.context(ErrorMarker::shortcut_failed())?;

	// Reload the persisted representation before touching any previous shortcut.
	// SAFETY: COM is initialized and no aggregation is requested.
	let verified: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
		.context(ErrorMarker::shortcut_failed())?;
	let reader: IPersistFile = verified.cast().context(ErrorMarker::shortcut_failed())?;
	// SAFETY: the existing staged path is terminated and lives through Load.
	unsafe { reader.Load(PCWSTR(staged_path.as_ptr()), STGM_READ) }.context(ErrorMarker::shortcut_failed())?;
	let mut stored = vec![0u16; arguments.len() + 1];
	// SAFETY: the mutable slice is valid and its length fits the API's i32 count.
	unsafe { verified.GetArguments(&mut stored) }.context(ErrorMarker::shortcut_failed())?;
	if stored[..arguments.len()] != arguments {
		return Err(report!(ErrorMarker::shortcut_arguments_too_long()));
	}

	match symlink_metadata(&final_path) {
		Ok(metadata) => {
			if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
				return Err(report!(ErrorMarker::shortcut_destination_invalid()));
			}
			let existing_path: Vec<_> = final_path.as_os_str().encode_wide().chain([0]).collect();
			// SAFETY: existing_path is terminated. Loading only checks the existing
			// file's format; it does not resolve or execute the shortcut target.
			unsafe { reader.Load(PCWSTR(existing_path.as_ptr()), STGM_READ) }
				.context(ErrorMarker::shortcut_destination_invalid())?;
		}
		Err(error) if error.kind() == ErrorKind::NotFound => {}
		Err(error) => return Err(report!(error).context(ErrorMarker::shortcut_destination_invalid())),
	}

	// tempfile uses the platform's single atomic rename/replace operation. The
	// staging file shares the destination filesystem; no delete-first gap exists.
	staged.persist(&final_path).context(ErrorMarker::shortcut_failed())?;

	Ok(())
}

#[cfg(test)]
mod tests {
	use super::ComApartment;
	use super::persist_shortcut;
	use application::shortcut::ShortcutDefinition;
	use rootcause::compat::boxed_error::IntoBoxedError;
	use std::env::current_exe;
	use std::error::Error;
	use std::ffi::OsString;
	use std::fs::canonicalize;
	use std::fs::create_dir;
	use std::fs::read;
	use std::fs::write;
	use std::os::windows::ffi::OsStrExt;
	use std::os::windows::ffi::OsStringExt;
	use std::path::Path;
	use std::path::PathBuf;
	use std::ptr::null_mut;
	use tempfile::TempDir;
	use windows::Win32::System::Com::CLSCTX_INPROC_SERVER;
	use windows::Win32::System::Com::COINIT_APARTMENTTHREADED;
	use windows::Win32::System::Com::CoCreateInstance;
	use windows::Win32::System::Com::CoInitializeEx;
	use windows::Win32::System::Com::IPersistFile;
	use windows::Win32::System::Com::STGM_READ;
	use windows::Win32::UI::Shell::IShellLinkW;
	use windows::Win32::UI::Shell::ShellLink;
	use windows::core::Interface;
	use windows::core::PCWSTR;

	fn fixture(directory: &Path) -> Result<ShortcutDefinition, Box<dyn Error>> {
		Ok(ShortcutDefinition {
			launcher: current_exe()?,
			arguments: vec!["--hidden".into()],
			working_directory: directory.to_owned(),
			icon: current_exe()?,
			name: "Native test".into(),
			destination: Some(directory.to_owned()),
		})
	}

	fn inspect(path: &Path) -> Result<(PathBuf, OsString, PathBuf, PathBuf), Box<dyn Error>> {
		// SAFETY: the test uses COM synchronously on one thread; the guard balances
		// successful initialization after all local interfaces have been dropped.
		unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
		let _apartment = ComApartment;
		// SAFETY: COM is initialized and the class supports the requested interface.
		let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
		let persistence: IPersistFile = link.cast()?;
		let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
		// SAFETY: path is a terminated string that remains valid through Load.
		unsafe { persistence.Load(PCWSTR(path.as_ptr()), STGM_READ) }?;
		let mut target = vec![0u16; 32768];
		let mut arguments = vec![0u16; 32768];
		let mut directory = vec![0u16; 32768];
		let mut icon = vec![0u16; 32768];
		let mut icon_index = -1;
		// SAFETY: buffers are writable and within i32 bounds. GetPath permits a null
		// optional find-data output; icon_index is a live writable integer.
		unsafe {
			link.GetPath(&mut target, null_mut(), 0)?;
			link.GetArguments(&mut arguments)?;
			link.GetWorkingDirectory(&mut directory)?;
			link.GetIconLocation(&mut icon, &mut icon_index)?;
		}
		assert_eq!(icon_index, 0);
		let text = |units: Vec<u16>| {
			OsString::from_wide(&units[..units.iter().position(|unit| *unit == 0).unwrap_or(units.len())])
		};
		Ok((
			text(target).into(),
			text(arguments),
			text(directory).into(),
			text(icon).into(),
		))
	}

	#[test]
	fn native_shortcut_preserves_fields_arguments_and_replaces_only_after_preparation() -> Result<(), Box<dyn Error>>
	{
		let temp = TempDir::new()?;
		let mut definition = fixture(temp.path())?;
		definition.arguments = vec![
			"".into(),
			"a\"b".into(),
			"雪".into(),
			"--".into(),
			OsString::from_wide(&[0xd800]),
		];
		persist_shortcut(definition.clone()).map_err(|error| -> Box<dyn Error> { error.into_boxed_error() })?;
		let path = temp.path().join("Native test.lnk");
		let (target, arguments, directory, icon) = inspect(&path)?;
		assert_eq!(target, definition.launcher);
		assert_eq!(directory, definition.working_directory);
		assert_eq!(icon, definition.icon);
		let mut expected: Vec<_> = "\"\" \"a\\\"b\" \"雪\" \"--\" \"".encode_utf16().collect();
		expected.extend([0xd800, 34]);
		assert_eq!(arguments, OsString::from_wide(&expected));

		let previous = read(&path)?;
		definition.arguments = vec!["a".repeat(1022).into()];
		assert!(persist_shortcut(definition.clone()).is_err());
		assert_eq!(read(&path)?, previous);

		definition.arguments = vec!["replacement".into()];
		persist_shortcut(definition).map_err(|error| -> Box<dyn Error> { error.into_boxed_error() })?;
		assert_eq!(inspect(&path)?.1, "\"replacement\"");
		Ok(())
	}

	#[test]
	fn native_shortcut_accepts_canonicalized_paths() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let mut definition = fixture(temp.path())?;
		definition.launcher = canonicalize(&definition.launcher)?;
		definition.working_directory = canonicalize(&definition.working_directory)?;
		definition.icon = canonicalize(&definition.icon)?;

		persist_shortcut(definition.clone()).map_err(|error| -> Box<dyn Error> { error.into_boxed_error() })?;
		let (target, _, directory, icon) = inspect(&temp.path().join("Native test.lnk"))?;
		assert_eq!(canonicalize(target)?, definition.launcher);
		assert_eq!(canonicalize(directory)?, definition.working_directory);
		assert_eq!(canonicalize(icon)?, definition.icon);
		Ok(())
	}

	#[test]
	fn native_shortcut_refuses_directory_and_non_link_files() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let definition = fixture(temp.path())?;
		let path = temp.path().join("Native test.lnk");
		write(&path, b"not a Shell Link")?;
		assert!(persist_shortcut(definition.clone()).is_err());
		assert_eq!(read(&path)?, b"not a Shell Link");

		let other = TempDir::new()?;
		let directory = other.path().join("Native test.lnk");
		create_dir(&directory)?;
		assert!(persist_shortcut(fixture(other.path())?).is_err());
		assert!(directory.is_dir());
		Ok(())
	}
}
