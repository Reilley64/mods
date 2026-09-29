use crate::environment::is_absolute_external;
use rootcause::Result;
use rootcause::report;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidGameInstallationPath;
impl fmt::Display for InvalidGameInstallationPath {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("game installation path must be nonempty and absolute")
	}
}
impl Error for InvalidGameInstallationPath {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameInstallationPath(PathBuf);
impl GameInstallationPath {
	pub fn new(path: PathBuf) -> Result<Self, InvalidGameInstallationPath> {
		if is_absolute_external(&path) && !path.as_os_str().is_empty() {
			Ok(Self(path))
		} else {
			Err(report!(InvalidGameInstallationPath))
		}
	}
	pub fn as_path(&self) -> &Path {
		&self.0
	}
	pub fn into_path_buf(self) -> PathBuf {
		self.0
	}
}
impl fmt::Display for GameInstallationPath {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.display().fmt(formatter)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SteamAppId(u32);
impl SteamAppId {
	pub const FALLOUT_NEW_VEGAS: Self = Self(22_380);
	pub fn new(value: u32) -> Result<Self, UnsupportedSteamAppId> {
		if value == Self::FALLOUT_NEW_VEGAS.0 {
			Ok(Self(value))
		} else {
			Err(report!(UnsupportedSteamAppId(value)))
		}
	}
	pub const fn get(self) -> u32 {
		self.0
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedSteamAppId(pub u32);
impl fmt::Display for UnsupportedSteamAppId {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "unsupported Steam App ID: {}", self.0)
	}
}
impl Error for UnsupportedSteamAppId {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameBinding {
	game_directory: GameInstallationPath,
}
impl GameBinding {
	pub fn new(game_directory: GameInstallationPath) -> Self {
		Self { game_directory }
	}
	pub fn game_directory(&self) -> &GameInstallationPath {
		&self.game_directory
	}
}

#[cfg(test)]
mod tests {
	use super::GameInstallationPath;
	use super::SteamAppId;
	use std::path::PathBuf;

	#[test]
	fn game_path_must_be_absolute() {
		assert!(GameInstallationPath::new(PathBuf::from("Fallout New Vegas")).is_err());
		assert!(GameInstallationPath::new(PathBuf::from("C:\\Games\\Fallout New Vegas")).is_ok());
	}

	#[test]
	fn app_id_is_fixed_to_fallout_new_vegas() {
		assert!(matches!(SteamAppId::new(22_380).map(SteamAppId::get), Ok(22_380)));
		assert!(SteamAppId::new(0).is_err());
	}
}
