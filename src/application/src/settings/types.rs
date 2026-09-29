use domain::GameBinding;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
	SchemaVersion,
	Name,
	GameDir,
}
impl SettingKey {
	pub const ALL: [Self; 3] = [Self::SchemaVersion, Self::Name, Self::GameDir];
	pub const fn manifest_path(self) -> &'static str {
		match self {
			Self::SchemaVersion => "schema_version",
			Self::Name => "name",
			Self::GameDir => "game_dir",
		}
	}
	pub const fn cli_name(self) -> &'static str {
		match self {
			Self::SchemaVersion => "schema-version",
			Self::Name => "name",
			Self::GameDir => "game-dir",
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingValue {
	Unset,
	String(String),
	Path(PathBuf),
	UnsignedInteger(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingSource {
	Manifest,
	Environment { variable: &'static str },
	Invocation { argument: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingRecord {
	pub key: SettingKey,
	pub value: SettingValue,
	pub source: SettingSource,
	pub manifest_value: SettingValue,
	pub manifest_path: &'static str,
	pub shadowed: bool,
	pub writable: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedSettings {
	pub settings: Vec<SettingRecord>,
	pub effective_binding: GameBinding,
	pub manifest_binding: GameBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveBinding {
	Valid,
	Invalid,
}
