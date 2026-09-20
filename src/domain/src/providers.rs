use crate::DataRelativePath;
use crate::ModName;
use crate::ModPriority;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderClass {
	SteamData,
	DataMod,
	Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProviderRank {
	Base,
	Regular(ModPriority),
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
pub enum ProviderIdentity {
	SteamData,
	DataMod { mod_name: ModName, priority: ModPriority },
	Overwrite,
}

impl ProviderIdentity {
	pub const fn class(&self) -> ProviderClass {
		match self {
			Self::SteamData => ProviderClass::SteamData,
			Self::DataMod { .. } => ProviderClass::DataMod,
			Self::Overwrite => ProviderClass::Overwrite,
		}
	}

	pub const fn rank(&self) -> ProviderRank {
		match self {
			Self::SteamData => ProviderRank::Base,
			Self::DataMod { priority, .. } => ProviderRank::Regular(*priority),
			Self::Overwrite => ProviderRank::Overwrite,
		}
	}
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

	pub const fn rank(&self) -> ProviderRank {
		match self {
			Self::SteamData { .. } => ProviderRank::Base,
			Self::DataMod { priority, .. } => ProviderRank::Regular(*priority),
			Self::Overwrite { .. } => ProviderRank::Overwrite,
		}
	}

	pub fn identity(&self) -> ProviderIdentity {
		match self {
			Self::SteamData { .. } => ProviderIdentity::SteamData,
			Self::DataMod { mod_name, priority, .. } => ProviderIdentity::DataMod {
				mod_name: mod_name.clone(),
				priority: *priority,
			},
			Self::Overwrite { .. } => ProviderIdentity::Overwrite,
		}
	}

	pub const fn participation_reason(&self) -> ParticipationReason {
		match self {
			Self::SteamData { .. } => ParticipationReason::SteamBase,
			Self::DataMod {
				participation_reason, ..
			} => *participation_reason,
			Self::Overwrite { .. } => ParticipationReason::Overwrite,
		}
	}

	pub fn with_participation_reason(&self, participation_reason: ParticipationReason) -> Self {
		match self {
			Self::SteamData { original_path } => Self::SteamData {
				original_path: original_path.clone(),
			},
			Self::DataMod {
				mod_name,
				priority,
				original_path,
				..
			} => Self::DataMod {
				mod_name: mod_name.clone(),
				priority: *priority,
				original_path: original_path.clone(),
				participation_reason,
			},
			Self::Overwrite { original_path } => Self::Overwrite {
				original_path: original_path.clone(),
			},
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

#[cfg(test)]
mod tests {
	use super::ParticipationReason;
	use super::ProviderIdentity;
	use super::ProviderRank;
	use super::ProviderReference;
	use crate::DataRelativePath;
	use crate::ModName;
	use crate::ModPriority;
	use std::error::Error;
	use std::result::Result as StdResult;

	#[test]
	fn provider_ranks_follow_effective_priority() {
		assert!(ProviderRank::Base < ProviderRank::Regular(ModPriority::new(0)));
		assert!(ProviderRank::Regular(ModPriority::new(u32::MAX)) < ProviderRank::Overwrite);
	}

	#[test]
	fn provider_reference_recreates_a_data_mod_with_new_participation() -> StdResult<(), Box<dyn Error>> {
		let provider = ProviderReference::DataMod {
			mod_name: ModName::new("Example".to_owned()).map_err(|_| "invalid mod name")?,
			priority: ModPriority::new(7),
			original_path: DataRelativePath::new("textures/example.dds".to_owned())
				.map_err(|_| "invalid path")?,
			participation_reason: ParticipationReason::DisabledMod,
		};

		let hypothetical = provider.with_participation_reason(ParticipationReason::HypotheticalEnabledMod);

		assert_eq!(hypothetical.identity(), provider.identity());
		assert_eq!(
			hypothetical.participation_reason(),
			ParticipationReason::HypotheticalEnabledMod
		);
		assert!(matches!(
			hypothetical.identity(),
			ProviderIdentity::DataMod { priority, .. } if priority == ModPriority::new(7)
		));
		Ok(())
	}
}
