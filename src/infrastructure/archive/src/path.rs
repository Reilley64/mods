use crate::error::ArchiveError;
use crate::limits::MAX_ARCHIVE_PATH_BYTES;
use crate::limits::MAX_ARCHIVE_PATH_COMPONENT_UTF16;
use crate::limits::MAX_ARCHIVE_PATH_COMPONENTS;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::report;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

// std::path follows the host platform, so it cannot enforce Windows component rules during cross-platform indexing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SafeArchivePath {
	normalized: String,
	components: Vec<String>,
}

impl SafeArchivePath {
	pub(crate) fn new(raw: &str) -> Result<Self, ArchiveError> {
		if raw.is_empty()
			|| raw.len() > MAX_ARCHIVE_PATH_BYTES
			|| raw.contains('\0') || raw.starts_with('/')
			|| raw.starts_with('\\')
			|| raw.starts_with("//")
			|| raw.starts_with("\\\\")
		{
			return Err(report!(ArchiveError::UnsafePath));
		}

		let replaced = raw.replace('\\', "/");
		if replaced.starts_with('/') || replaced.ends_with("//") {
			return Err(report!(ArchiveError::UnsafePath));
		}

		let mut components = Vec::new();
		for component in replaced.split('/') {
			if component.is_empty() || component == "." || component == ".." {
				return Err(report!(ArchiveError::UnsafePath));
			}
			if component.contains(':')
				|| component.ends_with('.') || component.ends_with(' ')
				|| component.encode_utf16().count() > MAX_ARCHIVE_PATH_COMPONENT_UTF16
				|| component.chars().any(is_forbidden_windows_character)
			{
				return Err(report!(ArchiveError::UnsafePath));
			}

			if is_windows_device_alias(component) {
				return Err(report!(ArchiveError::UnsafePath));
			}

			components.push(component.to_owned());
		}
		if components.is_empty() || components.len() > MAX_ARCHIVE_PATH_COMPONENTS {
			return Err(report!(ArchiveError::UnsafePath));
		}
		if components[0].encode_utf16().count() == 2 && components[0].as_bytes().get(1) == Some(&b':') {
			return Err(report!(ArchiveError::UnsafePath));
		}

		Ok(Self {
			normalized: components.join("/"),
			components,
		})
	}

	pub(crate) fn as_str(&self) -> &str {
		&self.normalized
	}

	pub(crate) fn components(&self) -> &[String] {
		&self.components
	}

	pub(crate) fn starts_with_components(&self, prefix: &[String]) -> bool {
		self.components.len() >= prefix.len()
			&& self.components
				.iter()
				.zip(prefix)
				.all(|(component, expected)| case_fold_key(component) == case_fold_key(expected))
	}

	pub(crate) fn strip_prefix(&self, prefix: &[String]) -> Option<String> {
		self.starts_with_components(prefix)
			.then(|| self.components[prefix.len()..].join("/"))
	}
}

pub(crate) fn normalization_alias_key(value: &str) -> String {
	case_fold_key(&value.nfkc().collect::<String>())
}

pub(crate) fn validate_unique_paths<'a>(
	paths: impl IntoIterator<Item = &'a SafeArchivePath>,
) -> Result<(), ArchiveError> {
	let mut identity_keys = HashSet::new();
	let mut normalization_alias_keys = HashSet::new();
	for path in paths {
		if !identity_keys.insert(case_fold_key(path.as_str()))
			|| !normalization_alias_keys.insert(normalization_alias_key(path.as_str()))
		{
			return Err(report!(ArchiveError::DuplicatePath));
		}
	}
	Ok(())
}

fn is_forbidden_windows_character(character: char) -> bool {
	character <= '\u{1f}' || matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
}

fn is_windows_device_alias(component: &str) -> bool {
	let stem = component.split('.').next().unwrap_or_default();
	let upper = stem.to_ascii_uppercase();
	if matches!(
		upper.as_str(),
		"CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
	) {
		return true;
	}

	let Some(port_number) = upper.strip_prefix("COM").or_else(|| upper.strip_prefix("LPT")) else {
		return false;
	};
	matches!(
		port_number,
		"1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
	)
}

#[cfg(test)]
mod tests {
	use super::SafeArchivePath;
	use super::validate_unique_paths;
	use crate::error::ArchiveError;
	use rootcause::Result;

	#[test]
	fn rejects_windows_escape_and_alias_paths() {
		for path in [
			"../evil",
			"/absolute",
			r"C:\evil",
			"safe/CON.txt",
			"safe/file.txt:stream",
			"safe./file",
			"safe//file",
		] {
			assert!(SafeArchivePath::new(path).is_err(), "accepted {path:?}");
		}
	}

	#[test]
	fn rejects_windows_device_aliases() {
		for path in [
			"devices/CONIN$",
			"devices/conin$.txt",
			"devices/ConOut$",
			"devices/cOnOuT$.log",
			"devices/COM¹",
			"devices/com².txt",
			"devices/CoM³.bin",
			"devices/LPT¹",
			"devices/lpt².txt",
			"devices/LpT³.bin",
		] {
			assert!(SafeArchivePath::new(path).is_err(), "accepted {path:?}");
		}
	}

	#[test]
	fn rejects_case_and_unicode_normalization_aliases() -> Result<(), ArchiveError> {
		let case = [
			SafeArchivePath::new("Data/File.txt")?,
			SafeArchivePath::new("data/file.txt")?,
		];
		let unicode_case = [SafeArchivePath::new("Data/Σ.txt")?, SafeArchivePath::new("data/ς.txt")?];
		let canonical = [
			SafeArchivePath::new("Data/É.txt")?,
			SafeArchivePath::new("data/E\u{301}.txt")?,
		];
		let compatibility = [
			SafeArchivePath::new("Data/ﬃ.txt")?,
			SafeArchivePath::new("data/ffi.txt")?,
		];

		assert!(validate_unique_paths(&case).is_err());
		assert!(validate_unique_paths(&unicode_case).is_err());
		assert!(validate_unique_paths(&canonical).is_err());
		assert!(validate_unique_paths(&compatibility).is_err());
		Ok(())
	}
}
