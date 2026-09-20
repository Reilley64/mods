use crate::case_fold_key;
use rootcause::Result;
use rootcause::report;
use std::error::Error;
use std::fmt;
use std::hash::Hash;
use std::hash::Hasher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidDataRelativePath;

impl fmt::Display for InvalidDataRelativePath {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("path must be a valid relative Windows Data path")
	}
}

impl Error for InvalidDataRelativePath {}

#[derive(Debug, Clone)]
pub struct DataRelativePath {
	display: String,
	comparison_key: String,
}

impl DataRelativePath {
	pub fn new(value: String) -> Result<Self, InvalidDataRelativePath> {
		if value.is_empty()
			|| value.starts_with(['/', '\\'])
			|| value.ends_with(['/', '\\'])
			|| value.as_bytes().get(1) == Some(&b':')
		{
			return Err(report!(InvalidDataRelativePath));
		}

		let components = value.split(['/', '\\']).collect::<Vec<_>>();
		if components
			.iter()
			.any(|component| !is_valid_windows_component(component))
		{
			return Err(report!(InvalidDataRelativePath));
		}

		let display = components.join("/");
		Ok(Self {
			comparison_key: case_fold_key(&display),
			display,
		})
	}

	pub fn as_str(&self) -> &str {
		&self.display
	}

	pub fn comparison_key(&self) -> &str {
		&self.comparison_key
	}

	pub fn components(&self) -> impl Iterator<Item = &str> {
		self.display.split('/')
	}
}

impl fmt::Display for DataRelativePath {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.display)
	}
}

impl PartialEq for DataRelativePath {
	fn eq(&self, other: &Self) -> bool {
		self.comparison_key == other.comparison_key
	}
}
impl Eq for DataRelativePath {}
impl Hash for DataRelativePath {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.comparison_key.hash(state);
	}
}

const MAX_WINDOWS_COMPONENT_UTF16: usize = 240;

// Archives target Windows on any host. std::path and general-purpose path crates do not enforce this narrow
// Data-component reserved-name policy.
pub(crate) fn is_valid_windows_component(component: &str) -> bool {
	if component.is_empty()
		|| matches!(component, "." | "..")
		|| component.encode_utf16().count() > MAX_WINDOWS_COMPONENT_UTF16
		|| component.ends_with([' ', '.'])
		|| component.chars().any(|character| {
			character.is_control()
				|| matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
		}) {
		return false;
	}

	!is_windows_device_alias(component)
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
	use super::DataRelativePath;
	use std::error::Error;
	use std::result::Result as StdResult;

	#[test]
	fn data_paths_normalize_separators_and_use_simple_unicode_case_folding() -> StdResult<(), Box<dyn Error>> {
		let first = DataRelativePath::new("Meshes\\Weapons/Rifle.NIF".to_owned())
			.map_err(|_| "invalid first path")?;
		let second = DataRelativePath::new("meshes/weapons/rifle.nif".to_owned())
			.map_err(|_| "invalid second path")?;
		assert_eq!(first, second);
		assert_eq!(first.as_str(), "Meshes/Weapons/Rifle.NIF");
		Ok(())
	}

	#[test]
	fn data_path_identity_does_not_normalize_unicode() -> StdResult<(), Box<dyn Error>> {
		let ligature =
			DataRelativePath::new("Textures/ﬃ.DDS".to_owned()).map_err(|_| "invalid ligature path")?;
		let letters =
			DataRelativePath::new("textures/ffi.dds".to_owned()).map_err(|_| "invalid letters path")?;
		let composed =
			DataRelativePath::new("Textures/Café.dds".to_owned()).map_err(|_| "invalid composed path")?;
		let decomposed = DataRelativePath::new("textures/Cafe\u{301}.dds".to_owned())
			.map_err(|_| "invalid decomposed path")?;

		let uppercase =
			DataRelativePath::new("Textures/ÉΣ.dds".to_owned()).map_err(|_| "invalid uppercase path")?;
		let lowercase =
			DataRelativePath::new("textures/éς.dds".to_owned()).map_err(|_| "invalid lowercase path")?;

		assert_ne!(ligature, letters);
		assert_ne!(composed, decomposed);
		assert_eq!(uppercase, lowercase);
		assert_eq!(ligature.comparison_key(), "textures/ﬃ.dds");
		Ok(())
	}

	#[test]
	fn data_paths_enforce_the_utf16_component_limit() -> StdResult<(), Box<dyn Error>> {
		let maximum = "😀".repeat(120);
		DataRelativePath::new(format!("textures/{maximum}")).map_err(|_| "maximum path rejected")?;
		assert!(DataRelativePath::new(format!("textures/{maximum}a")).is_err());
		Ok(())
	}

	#[test]
	fn data_paths_reject_windows_device_aliases() {
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
			assert!(
				DataRelativePath::new(path.to_owned()).is_err(),
				"{path:?} must be rejected"
			);
		}
	}

	#[test]
	fn data_paths_reject_non_relative_and_invalid_components() {
		for path in [
			"",
			"Data/",
			"/Data/file",
			"C:\\Data\\file",
			"../file",
			"a//b",
			"aux/file",
			"a:b",
		] {
			assert!(
				DataRelativePath::new(path.to_owned()).is_err(),
				"{path:?} must be rejected"
			);
		}
	}
}
