use crate::DataRelativePath;
use crate::ProviderIdentity;
use crate::ProviderReference;
use crate::Sha256Digest;
use crate::Tombstone;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolutionStatus {
	Exact,
	Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Participation {
	Active,
	Inactive,
	Hypothetical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectiveResult {
	File(ProviderReference),
	Absent { controlling_tombstone: Option<Tombstone> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentState {
	NotCompared,
	SameSha256,
	DifferentSha256,
	Unavailable,
	Unstable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentComparison {
	NotCompared {
		winner: ProviderReference,
		loser: ProviderReference,
	},
	SameSha256 {
		winner: ProviderReference,
		loser: ProviderReference,
		winner_sha256: Sha256Digest,
		loser_sha256: Sha256Digest,
	},
	DifferentSha256 {
		winner: ProviderReference,
		loser: ProviderReference,
		winner_sha256: Sha256Digest,
		loser_sha256: Sha256Digest,
	},
	Unavailable {
		winner: ProviderReference,
		loser: ProviderReference,
	},
	Unstable {
		winner: ProviderReference,
		loser: ProviderReference,
	},
}

impl ContentComparison {
	pub const fn state(&self) -> ContentState {
		match self {
			Self::NotCompared { .. } => ContentState::NotCompared,
			Self::SameSha256 { .. } => ContentState::SameSha256,
			Self::DifferentSha256 { .. } => ContentState::DifferentSha256,
			Self::Unavailable { .. } => ContentState::Unavailable,
			Self::Unstable { .. } => ContentState::Unstable,
		}
	}

	pub fn winner(&self) -> &ProviderReference {
		match self {
			Self::NotCompared { winner, .. }
			| Self::SameSha256 { winner, .. }
			| Self::DifferentSha256 { winner, .. }
			| Self::Unavailable { winner, .. }
			| Self::Unstable { winner, .. } => winner,
		}
	}

	pub fn loser(&self) -> &ProviderReference {
		match self {
			Self::NotCompared { loser, .. }
			| Self::SameSha256 { loser, .. }
			| Self::DifferentSha256 { loser, .. }
			| Self::Unavailable { loser, .. }
			| Self::Unstable { loser, .. } => loser,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictRow {
	OrdinaryConflict {
		normalized_key: String,
		display_path: DataRelativePath,
		participation: Participation,
		effective_file: ProviderReference,
		losing_files: Vec<ProviderReference>,
		content_comparisons: Vec<ContentComparison>,
	},
	Tombstone {
		normalized_key: String,
		display_path: DataRelativePath,
		participation: Participation,
		controlling_tombstone: Tombstone,
		suppressed_entries: Vec<ProviderReference>,
	},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderState {
	Invalid,
	Inactive,
	SuppressionOnly,
	Empty,
	FullyOverridden,
	Mixed,
	WinningConflicts,
	LosingConflicts,
	Uncontested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSummary {
	pub ordinary_file_count: u64,
	pub effective_file_count: u64,
	pub file_conflict_win_count: u64,
	pub file_conflict_loss_count: u64,
	pub owned_file_tombstone_count: u64,
	pub owned_directory_tombstone_count: u64,
	pub lower_file_entries_suppressed_count: u64,
	pub own_file_entries_suppressed_count: u64,
	pub state: ProviderState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictProblemKind {
	ModlistInvalid,
	ProviderMissing,
	InternalKeyCollision,
	FileDirectoryCollision,
	OrdinaryTombstoneCollision,
	DirectoryTombstoneDescendantCollision,
	InvalidTombstoneMetadata,
	InvalidTombstonePath,
	ReparsePoint,
	HardLink,
	UnsupportedEntryType,
	ContainmentEscape,
	NonLosslessName,
	ReservedPath,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProblemScope {
	Global,
	Modlist,
	Provider(ProviderIdentity),
	Path {
		normalized_key: String,
		display_path: DataRelativePath,
	},
	Subtree {
		normalized_key: String,
		display_path: DataRelativePath,
	},
}

pub type ConflictProblemScope = ProblemScope;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConflictProblem {
	pub kind: ConflictProblemKind,
	pub scope: ProblemScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TombstoneEffect {
	Controlling {
		tombstone: Tombstone,
		suppressed_entries: Vec<ProviderReference>,
	},
	ShadowedByTombstone {
		tombstone: Tombstone,
		controlling_tombstone: Tombstone,
	},
	OverriddenByFile {
		tombstone: Tombstone,
		overriding_file: ProviderReference,
	},
	Orphan {
		tombstone: Tombstone,
	},
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionReason {
	EffectiveFile {
		provider: ProviderReference,
	},
	LowerPriorityFile {
		provider: ProviderReference,
		winner: ProviderReference,
	},
	SuppressedByTombstone {
		provider: ProviderReference,
		controlling_tombstone: Tombstone,
	},
	AbsentNoEntry,
	AbsentByTombstone {
		controlling_tombstone: Tombstone,
	},
	ShadowedTombstone {
		tombstone: Tombstone,
		controlling_tombstone: Tombstone,
	},
	TombstoneOverriddenByFile {
		tombstone: Tombstone,
		overriding_file: ProviderReference,
	},
	NamespaceInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictView {
	pub resolution_status: ResolutionStatus,
	pub rows: Vec<ConflictRow>,
	pub problems: Vec<ConflictProblem>,
}

#[cfg(test)]
mod tests {
	use super::ContentComparison;
	use super::ContentState;
	use super::EffectiveResult;
	use crate::DataRelativePath;
	use crate::ProviderReference;
	use std::error::Error;
	use std::result::Result as StdResult;

	fn steam_file(path: &str) -> StdResult<ProviderReference, Box<dyn Error>> {
		Ok(ProviderReference::SteamData {
			original_path: DataRelativePath::new(path.to_owned()).map_err(|_| "invalid path")?,
		})
	}

	#[test]
	fn content_comparison_reports_its_closed_state() -> StdResult<(), Box<dyn Error>> {
		let comparison = ContentComparison::Unavailable {
			winner: steam_file("textures/winner.dds")?,
			loser: steam_file("textures/loser.dds")?,
		};

		assert_eq!(comparison.state(), ContentState::Unavailable);
		assert_eq!(comparison.winner().original_path().as_str(), "textures/winner.dds");
		assert_eq!(comparison.loser().original_path().as_str(), "textures/loser.dds");
		Ok(())
	}

	#[test]
	fn absent_result_distinguishes_unknown_paths_from_tombstone_control() {
		let unknown = EffectiveResult::Absent {
			controlling_tombstone: None,
		};
		assert!(matches!(
			unknown,
			EffectiveResult::Absent {
				controlling_tombstone: None
			}
		));
	}
}
