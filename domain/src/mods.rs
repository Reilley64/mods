use crate::case_fold_key;
use crate::paths::is_valid_windows_component;
use rootcause::Result;
use rootcause::report;
use std::error::Error;
use std::fmt;
use std::hash::Hash;
use std::hash::Hasher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidModName;

impl fmt::Display for InvalidModName {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("mod name must be a valid Windows directory component")
	}
}
impl Error for InvalidModName {}

#[derive(Debug, Clone)]
pub struct ModName {
	display: String,
	comparison_key: String,
}

impl ModName {
	pub fn new(value: String) -> Result<Self, InvalidModName> {
		if value.trim().is_empty() || !is_valid_windows_component(&value) {
			return Err(report!(InvalidModName));
		}
		let comparison_key = case_fold_key(&value);
		if comparison_key == "overwrite" {
			return Err(report!(InvalidModName));
		}

		Ok(Self {
			comparison_key,
			display: value,
		})
	}

	pub fn as_str(&self) -> &str {
		&self.display
	}

	pub fn comparison_key(&self) -> &str {
		&self.comparison_key
	}
}

impl fmt::Display for ModName {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(&self.display)
	}
}
impl PartialEq for ModName {
	fn eq(&self, other: &Self) -> bool {
		self.comparison_key == other.comparison_key
	}
}
impl Eq for ModName {}
impl Hash for ModName {
	fn hash<H: Hasher>(&self, state: &mut H) {
		self.comparison_key.hash(state);
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModPriority(u32);
impl ModPriority {
	pub const fn new(value: u32) -> Self {
		Self(value)
	}
	pub const fn get(self) -> u32 {
		self.0
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledMod {
	pub name: ModName,
	pub priority: ModPriority,
	pub enabled: bool,
}

#[cfg(test)]
mod tests {
	use super::ModName;
	use std::error::Error;
	use std::result::Result as StdResult;

	#[test]
	fn mod_names_preserve_spelling_and_compare_with_simple_unicode_case_folding() -> StdResult<(), Box<dyn Error>> {
		let first = ModName::new("Mojave Textures".to_owned()).map_err(|_| "invalid first name")?;
		let second = ModName::new("mojave textures".to_owned()).map_err(|_| "invalid second name")?;
		assert_eq!(first, second);
		assert_eq!(first.as_str(), "Mojave Textures");
		Ok(())
	}

	#[test]
	fn mod_names_reject_reserved_or_unsafe_windows_components() {
		for name in [
			"",
			" ",
			".",
			"..",
			"Overwrite",
			"con.txt",
			"bad/name",
			"bad.",
			"bad ",
			"a:b",
		] {
			assert!(ModName::new(name.to_owned()).is_err(), "{name:?} must be rejected");
		}
	}
	#[test]
	fn mod_name_identity_does_not_normalize_unicode() -> StdResult<(), Box<dyn Error>> {
		let ligature = ModName::new("Oﬃcial Patch".to_owned()).map_err(|_| "invalid ligature name")?;
		let letters = ModName::new("official patch".to_owned()).map_err(|_| "invalid letters name")?;
		let composed = ModName::new("Café".to_owned()).map_err(|_| "invalid composed name")?;
		let decomposed = ModName::new("Cafe\u{301}".to_owned()).map_err(|_| "invalid decomposed name")?;

		let uppercase = ModName::new("ÉΣ Patch".to_owned()).map_err(|_| "invalid uppercase name")?;
		let lowercase = ModName::new("éς patch".to_owned()).map_err(|_| "invalid lowercase name")?;

		assert_ne!(ligature, letters);
		assert_ne!(composed, decomposed);
		assert_eq!(uppercase, lowercase);
		Ok(())
	}

	#[test]
	fn mod_names_enforce_the_utf16_component_limit() -> StdResult<(), Box<dyn Error>> {
		let maximum = "😀".repeat(120);
		ModName::new(maximum.clone()).map_err(|_| "maximum name rejected")?;
		assert!(ModName::new(format!("{maximum}a")).is_err());
		Ok(())
	}
}
