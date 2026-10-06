use super::LaunchInputError;
use super::encode_command_line;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsHandle;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::BorrowedHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::path::Prefix;
use windows::Win32::Foundation::DUPLICATE_SAME_ACCESS;
use windows::Win32::Foundation::DuplicateHandle;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::GetStdHandle;
use windows::Win32::System::Console::STD_ERROR_HANDLE;
use windows::Win32::System::Console::STD_INPUT_HANDLE;
use windows::Win32::System::Console::STD_OUTPUT_HANDLE;
use windows::Win32::System::Threading::GetCurrentProcess;

/// Immutable caller lookup state. Child cwd never participates in lookup.
#[derive(Clone)]
pub struct CallerSnapshot {
	directory: PathBuf,
	path: Vec<PathBuf>,
	environment: Vec<(OsString, OsString)>,
}

/// Caller-resolved paths; Windows process creation checks access and executable format.
pub struct ResolvedLaunch {
	pub application: PathBuf,
	pub directory: PathBuf,
	pub command_line: OsString,
}

impl CallerSnapshot {
	/// Captures the caller directory and inherited environment before resolution.
	///
	/// # Errors
	/// Returns a snapshot failure if the caller directory cannot be read.
	pub fn capture() -> Result<Self, LaunchInputError> {
		let directory = env::current_dir().context(LaunchInputError::Snapshot)?;
		Ok(Self::new(directory))
	}

	/// Captures inherited PATH with an already captured caller startup directory.
	pub fn new(directory: PathBuf) -> Self {
		let environment: Vec<_> = env::vars_os().collect();
		let path = environment
			.iter()
			.find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case("PATH"))
			.map(|(_, value)| {
				env::split_paths(value)
					.filter(|path| !path.as_os_str().is_empty())
					.collect()
			})
			.unwrap_or_default();
		Self {
			directory,
			path,
			environment,
		}
	}

	/// Resolves an executable independently of the requested child directory.
	///
	/// # Errors
	/// Returns typed lookup, path validation, encoding, or directory failures.
	pub fn resolve(
		&self,
		program: &OsStr,
		arguments: &[OsString],
		cwd: Option<&Path>,
	) -> Result<ResolvedLaunch, LaunchInputError> {
		let units: Vec<_> = program.encode_wide().collect();
		if units.is_empty() || units.contains(&0) {
			return Err(report!(LaunchInputError::InvalidString));
		}
		let path = Path::new(program);
		let path_like = units.iter().any(|unit| [47, 92, 58].contains(unit));
		let roots = if path_like {
			vec![self.absolute(path)?]
		} else {
			self.path
				.iter()
				.map(|entry| self.absolute(&entry.join(path)))
				.collect::<Result<Vec<_>, _>>()?
		};
		let mut candidates = roots.clone();
		if path.extension().is_none() {
			candidates.extend(roots.into_iter().map(|entry| {
				let mut value = entry.into_os_string();
				value.push(".exe");
				PathBuf::from(value)
			}));
		}
		let mut resolved = None;
		for candidate in candidates {
			if fs::metadata(&candidate).is_err_and(|error| error.kind() == ErrorKind::NotFound) {
				continue;
			}

			// A lookup access error must not hide this PATH candidate. Windows launch
			// reports access and format errors; no validation handle is retained.
			resolved = Some(candidate);
			break;
		}

		let application = resolved.ok_or_else(|| report!(LaunchInputError::NotFound))?;
		let directory = self.absolute(cwd.unwrap_or(&self.directory))?;

		let mut encoded_arguments = vec![application.as_os_str().encode_wide().collect()];
		encoded_arguments.extend(arguments.iter().map(|argument| argument.encode_wide().collect()));
		let command_line = OsString::from_wide(&encode_command_line(&encoded_arguments)?);

		Ok(ResolvedLaunch {
			application,
			directory,
			command_line,
		})
	}

	fn absolute(&self, path: &Path) -> Result<PathBuf, LaunchInputError> {
		if path.as_os_str().encode_wide().any(|unit| unit == 0) {
			return Err(report!(LaunchInputError::InvalidString));
		}
		if path.is_absolute() {
			return Ok(path.to_owned());
		}
		if let Some(Component::Prefix(prefix)) = path.components().next() {
			let Prefix::Disk(drive) = prefix.kind() else {
				return Err(report!(LaunchInputError::InvalidTarget));
			};
			let remainder: PathBuf = path.components().skip(1).collect();
			let current_drive = self.directory.components().next();
			if current_drive.is_some_and(
				|component| matches!(component, Component::Prefix(prefix) if matches!(prefix.kind(), Prefix::Disk(current) if current.eq_ignore_ascii_case(&drive))),
			) {
				return Ok(self.directory.join(remainder));
			}
			let key = format!("={}:", char::from(drive));
			let base = self
				.environment
				.iter()
				.find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case(&key))
				.map(|(_, value)| PathBuf::from(value))
				.unwrap_or_else(|| PathBuf::from(format!("{}:\\", char::from(drive))));
			return Ok(base.join(remainder));
		}
		Ok(self.directory.join(path))
	}
}

/// Inheritable duplicates preserve redirected streams without changing caller flags.
pub struct InheritedStreams {
	handles: [OwnedHandle; 3],
}
impl InheritedStreams {
	/// Duplicates the invoking process's three standard handles for child inheritance.
	///
	/// # Errors
	/// Returns a standard-stream failure if a stream is missing or duplication fails.
	pub fn capture() -> Result<Self, LaunchInputError> {
		let mut handles = Vec::new();
		for selector in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
			// SAFETY: selector is one of the three SDK-defined standard handle selectors.
			let source = unsafe { GetStdHandle(selector) }.context(LaunchInputError::StandardStreams)?;
			if source.is_invalid() {
				return Err(report!(LaunchInputError::StandardStreams));
			}
			// SAFETY: this pseudo handle always names the current live process.
			let process = unsafe { GetCurrentProcess() };
			let mut duplicate = HANDLE::default();
			// SAFETY: source is borrowed, both process handles name this process, and
			// duplicate is a writable output. Success transfers one owned handle.
			unsafe {
				DuplicateHandle(
					process,
					source,
					process,
					&mut duplicate,
					0,
					true,
					DUPLICATE_SAME_ACCESS,
				)
			}
			.context(LaunchInputError::StandardStreams)?;
			// SAFETY: successful duplication returned an exclusively owned valid handle.
			handles.push(unsafe { OwnedHandle::from_raw_handle(duplicate.0) });
		}
		let handles = handles
			.try_into()
			.map_err(|_| report!(LaunchInputError::StandardStreams))?;
		Ok(Self { handles })
	}

	pub(crate) fn duplicate(streams: [BorrowedHandle<'_>; 3]) -> Result<Self, LaunchInputError> {
		let mut handles = Vec::new();
		for source in streams {
			// SAFETY: GetCurrentProcess returns the live process pseudo handle.
			let process = unsafe { GetCurrentProcess() };
			let mut duplicate = HANDLE::default();
			// SAFETY: each source is borrowed for this call; duplication transfers
			// a distinct owned inheritable handle without changing the source flags.
			unsafe {
				DuplicateHandle(
					process,
					HANDLE(source.as_raw_handle()),
					process,
					&mut duplicate,
					0,
					true,
					DUPLICATE_SAME_ACCESS,
				)
			}
			.context(LaunchInputError::StandardStreams)?;
			// SAFETY: DuplicateHandle succeeded and transferred this valid handle.
			handles.push(unsafe { OwnedHandle::from_raw_handle(duplicate.0) });
		}

		let handles = handles
			.try_into()
			.map_err(|_| report!(LaunchInputError::StandardStreams))?;

		Ok(Self { handles })
	}

	pub fn borrowed(&self) -> [BorrowedHandle<'_>; 3] {
		[
			self.handles[0].as_handle(),
			self.handles[1].as_handle(),
			self.handles[2].as_handle(),
		]
	}
}

#[cfg(test)]
mod tests {
	use super::CallerSnapshot;
	use std::ffi::OsStr;
	use std::ffi::OsString;
	use std::path::Path;
	use std::path::PathBuf;

	#[test]
	fn relative_and_drive_relative_inputs_use_snapshot_not_child_cwd() {
		let snapshot = CallerSnapshot {
			directory: PathBuf::from(r"C:\caller"),
			path: vec![],
			environment: vec![(OsString::from("=D:"), OsString::from(r"D:\other"))],
		};
		for (input, expected) in [
			(r"tools\tool.exe", r"C:\caller\tools\tool.exe"),
			(r"C:tool.exe", r"C:\caller\tool.exe"),
			(r"D:tool.exe", r"D:\other\tool.exe"),
			(r"\tool.exe", r"C:\tool.exe"),
			(r"\\server\share\tool.exe", r"\\server\share\tool.exe"),
		] {
			assert_eq!(snapshot.absolute(Path::new(input)).ok(), Some(PathBuf::from(expected)));
		}
	}

	#[test]
	fn empty_path_does_not_implicitly_search_caller_directory() {
		let snapshot = CallerSnapshot {
			directory: PathBuf::from(r"C:\Windows\System32"),
			path: vec![],
			environment: vec![],
		};
		assert!(snapshot.resolve(OsStr::new("cmd.exe"), &[], None).is_err());
	}
}
