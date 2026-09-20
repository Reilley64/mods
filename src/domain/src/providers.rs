use crate::DataRelativePath;
use crate::ModName;
use crate::ModPriority;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderClass {
	SteamData,
	DataMod,
	Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParticipationReason {
	SteamBase,
	EnabledMod,
	DisabledMod,
	HypotheticalEnabledMod,
	ProjectedDisabledMod,
	Overwrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderReference {
	SteamData {
		original_path: DataRelativePath,
	},
	DataMod {
		mod_name: ModName,
		priority: ModPriority,
		original_path: DataRelativePath,
		participation_reason: ParticipationReason,
	},
	Overwrite {
		original_path: DataRelativePath,
	},
}

impl ProviderReference {
	pub const fn class(&self) -> ProviderClass {
		match self {
			Self::SteamData { .. } => ProviderClass::SteamData,
			Self::DataMod { .. } => ProviderClass::DataMod,
			Self::Overwrite { .. } => ProviderClass::Overwrite,
		}
	}
	pub fn original_path(&self) -> &DataRelativePath {
		match self {
			Self::SteamData { original_path }
			| Self::DataMod { original_path, .. }
			| Self::Overwrite { original_path } => original_path,
		}
	}
}
