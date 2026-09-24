use crate::ModName;
use rootcause::Result;
use rootcause::report;
use std::error::Error;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
	Overwrite,
	DataMod(ModName),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidExecutionValue;

impl fmt::Display for InvalidExecutionValue {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("execution value is empty or contains NUL")
	}
}
impl Error for InvalidExecutionValue {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program(OsString);
impl Program {
	pub fn new(value: OsString) -> Result<Self, InvalidExecutionValue> {
		if value.is_empty() || value.as_encoded_bytes().contains(&0) {
			return Err(report!(InvalidExecutionValue));
		}

		Ok(Self(value))
	}
	pub fn as_os_str(&self) -> &OsStr {
		&self.0
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramArgument(OsString);
impl ProgramArgument {
	pub fn new(value: OsString) -> Result<Self, InvalidExecutionValue> {
		if value.as_encoded_bytes().contains(&0) {
			return Err(report!(InvalidExecutionValue));
		}

		Ok(Self(value))
	}
	pub fn as_os_str(&self) -> &OsStr {
		&self.0
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingDirectory(PathBuf);
impl WorkingDirectory {
	pub fn new(value: PathBuf) -> Result<Self, InvalidExecutionValue> {
		if value.as_os_str().is_empty() || value.as_os_str().as_encoded_bytes().contains(&0) {
			return Err(report!(InvalidExecutionValue));
		}

		Ok(Self(value))
	}
	pub fn as_path(&self) -> &Path {
		&self.0
	}
}

/// The complete Windows root-process status; a nonzero value is still a child result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessStatus {
	value: u32,
	origin: ProcessStatusOrigin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatusOrigin {
	Child,
}
impl ProcessStatus {
	pub const fn new(value: u32) -> Self {
		Self {
			value,
			origin: ProcessStatusOrigin::Child,
		}
	}
	pub const fn value(self) -> u32 {
		self.value
	}
	pub const fn origin(self) -> ProcessStatusOrigin {
		self.origin
	}
}

#[cfg(test)]
mod tests {
	use super::InvalidExecutionValue;
	use super::ProcessStatus;
	use super::ProcessStatusOrigin;
	use super::Program;
	use super::ProgramArgument;
	use super::WorkingDirectory;
	use rootcause::Result;
	use std::ffi::OsString;
	use std::path::PathBuf;

	#[test]
	fn launch_values_preserve_arguments_and_full_status() -> Result<(), InvalidExecutionValue> {
		assert!(Program::new(OsString::new()).is_err());
		assert!(Program::new(OsString::from("bad\0name")).is_err());
		assert!(WorkingDirectory::new(PathBuf::new()).is_err());
		for value in ["", "two words", "quote\"", "tail\\", "--", "雪"] {
			let argument = ProgramArgument::new(OsString::from(value))?;
			assert_eq!(argument.as_os_str(), value);
		}
		assert_eq!(ProcessStatus::new(259).value(), 259);
		assert_eq!(ProcessStatus::new(259).origin(), ProcessStatusOrigin::Child);
		assert_eq!(ProcessStatus::new(0xC000_013A).value(), 0xC000_013A);
		Ok(())
	}
}
