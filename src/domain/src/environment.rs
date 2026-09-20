use rootcause::Result;
use rootcause::report;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEnvironmentRoot;

impl fmt::Display for InvalidEnvironmentRoot {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("environment root must be an absolute path")
	}
}
impl Error for InvalidEnvironmentRoot {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvironmentRoot(PathBuf);

impl EnvironmentRoot {
	pub fn new(path: PathBuf) -> Result<Self, InvalidEnvironmentRoot> {
		if is_absolute_external(&path) && !path.as_os_str().is_empty() {
			Ok(Self(path))
		} else {
			Err(report!(InvalidEnvironmentRoot))
		}
	}

	pub fn as_path(&self) -> &Path {
		&self.0
	}

	pub fn into_path_buf(self) -> PathBuf {
		self.0
	}
}

impl fmt::Display for EnvironmentRoot {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.display().fmt(formatter)
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEnvironmentName;

impl fmt::Display for InvalidEnvironmentName {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("environment name must be nonempty and contain no control characters")
	}
}
impl Error for InvalidEnvironmentName {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnvironmentName(String);

impl EnvironmentName {
	pub fn new(value: String) -> Result<Self, InvalidEnvironmentName> {
		if value.trim().is_empty() || value.chars().any(char::is_control) {
			Err(report!(InvalidEnvironmentName))
		} else {
			Ok(Self(value))
		}
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvironmentSchemaVersion(u32);

impl EnvironmentSchemaVersion {
	pub const CURRENT: Self = Self(1);

	pub fn new(value: u32) -> Result<Self, UnsupportedEnvironmentSchemaVersion> {
		if value == Self::CURRENT.0 {
			Ok(Self(value))
		} else {
			Err(report!(UnsupportedEnvironmentSchemaVersion(value)))
		}
	}

	pub const fn get(self) -> u32 {
		self.0
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedEnvironmentSchemaVersion(pub u32);
impl fmt::Display for UnsupportedEnvironmentSchemaVersion {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "unsupported environment schema version: {}", self.0)
	}
}
impl Error for UnsupportedEnvironmentSchemaVersion {}

pub(crate) fn is_absolute_external(path: &Path) -> bool {
	if path.is_absolute() {
		return true;
	}
	#[cfg(all(test, not(windows)))]
	{
		let Some(text) = path.to_str() else {
			return false;
		};
		let bytes = text.as_bytes();
		if bytes.len() >= 3
			&& bytes[0].is_ascii_alphabetic()
			&& bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/')
		{
			return true;
		}
		text.strip_prefix("\\\\").is_some_and(|unc| {
			unc.split(['\\', '/']).filter(|component| !component.is_empty()).count() >= 2
		})
	}
	#[cfg(not(all(test, not(windows))))]
	false
}

#[cfg(test)]
mod tests {
	use super::EnvironmentName;
	use super::EnvironmentRoot;
	use super::EnvironmentSchemaVersion;
	use super::InvalidEnvironmentName;
	use rootcause::Result;
	use std::path::PathBuf;

	#[test]
	fn environment_root_requires_an_absolute_path() {
		assert!(EnvironmentRoot::new(PathBuf::from("relative")).is_err());
		assert!(EnvironmentRoot::new(PathBuf::from("C:\\mods\\default")).is_ok());
	}

	#[test]
	fn environment_name_preserves_valid_spelling() -> Result<(), InvalidEnvironmentName> {
		let name = EnvironmentName::new("  Mojave  ".to_owned())?;
		assert_eq!(name.as_str(), "  Mojave  ");
		assert!(EnvironmentName::new("   ".to_owned()).is_err());
		assert!(EnvironmentName::new("bad\nname".to_owned()).is_err());
		Ok(())
	}

	#[test]
	fn only_schema_one_is_supported() {
		assert!(matches!(
			EnvironmentSchemaVersion::new(1).map(EnvironmentSchemaVersion::get),
			Ok(1)
		));
		assert!(EnvironmentSchemaVersion::new(0).is_err());
		assert!(EnvironmentSchemaVersion::new(2).is_err());
	}
}
