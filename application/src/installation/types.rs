use domain::ArchiveIdentity;
use domain::DataRelativePath;
use domain::FileDependencyState;
use domain::FomodCardinality;
use domain::FomodCondition;
use domain::GameBinding;
use domain::InstallCandidate;
use domain::InstallationPhase;
use domain::InstalledMod;
use domain::ModName;
use domain::ModPriority;
use domain::ProviderReference;
use domain::ResolvedOptionType;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FomodFlagWrite {
	pub name: String,
	pub value: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FomodOptionTypePattern {
	pub condition: FomodCondition,
	pub option_type: ResolvedOptionType,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FomodOption {
	pub id: String,
	pub label: String,
	pub description: String,
	pub condition: FomodCondition,
	pub default_type: ResolvedOptionType,
	pub type_patterns: Vec<FomodOptionTypePattern>,
	pub flag_writes: Vec<FomodFlagWrite>,
	pub file_candidates: Vec<InstallCandidate>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FomodGroup {
	pub id: String,
	pub label: String,
	pub description: String,
	pub cardinality: FomodCardinality,
	pub condition: FomodCondition,
	pub options: Vec<FomodOption>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalCandidates {
	pub condition: FomodCondition,
	pub candidates: Vec<InstallCandidate>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FomodInstaller {
	pub schema_version: String,
	pub module_condition: FomodCondition,
	pub groups: Vec<FomodGroup>,
	pub required_candidates: Vec<InstallCandidate>,
	pub conditional_candidates: Vec<ConditionalCandidates>,
	pub warnings: Vec<InstallWarning>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexedInstaller {
	Plain {
		candidates: Vec<InstallCandidate>,
		warnings: Vec<InstallWarning>,
	},
	Fomod(FomodInstaller),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveIndex {
	pub identity: ArchiveIdentity,
	pub installer: IndexedInstaller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TombstoneScope {
	ExactFile,
	DirectorySubtree,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TombstoneReference {
	pub scope: TombstoneScope,
	pub owner: ProviderReference,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectiveResult {
	File(ProviderReference),
	Absent {
		controlling_tombstone: Option<TombstoneReference>,
	},
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDependencyKind {
	Plugin,
	OrdinaryDataFile,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileDependencyFact {
	pub kind: FileDependencyKind,
	pub state: FileDependencyState,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationState {
	pub game_binding: GameBinding,
	pub installed_mods: Vec<InstalledMod>,
	pub current_winners: HashMap<DataRelativePath, EffectiveResult>,
	pub file_dependencies: HashMap<String, FileDependencyFact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedChoice {
	pub group_id: String,
	pub option_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionOperator {
	And,
	Or,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionScope {
	Module,
	StepVisibility,
	OptionTypePattern,
	ConditionalFilePattern,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MalformedGroupRepair {
	SingleOptionExactlyOneToSelectAll,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallWarning {
	FomodCouldBeUsableSelected {
		group_id: String,
		option_id: String,
	},
	FomodConflictingFlagValues {
		flag_name: String,
		values: Vec<String>,
		resolved_value: String,
	},
	FomodEmptyConditionList {
		operator: ConditionOperator,
		scope: ConditionScope,
		group_id: Option<String>,
		option_id: Option<String>,
		pattern_order: Option<u64>,
	},
	FomodMalformedGroupRepaired {
		group_id: String,
		repair: MalformedGroupRepair,
	},
	FomodFommDependencyAssumedCompatible {
		minimum_version: String,
	},
	FomodEmptySourceIgnored {
		descriptor_order: u64,
		group_id: Option<String>,
		option_id: Option<String>,
	},
	FomodEmptyOptionAccepted {
		group_id: String,
		option_id: String,
	},
	FomodModuleConfigPreferred {
		selected_config_member: String,
		ignored_config_member: String,
	},
	FomodNonPluginFileDependencyPolicyUsed {
		data_relative_path: String,
		state: FileDependencyState,
	},
	FomodEqualPriorityTieResolved {
		destination: DataRelativePath,
		phase: InstallationPhase,
		declared_priority: i32,
		winner_candidate_id: u64,
		loser_candidate_ids: Vec<u64>,
	},
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleOption {
	pub id: String,
	pub label: String,
	pub description: String,
	pub resolved_type: ResolvedOptionType,
	pub selectable: bool,
	pub synthetic: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedGroup {
	pub id: String,
	pub label: String,
	pub description: String,
	pub cardinality: FomodCardinality,
	pub options: Vec<VisibleOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinnerReason {
	OnlyCandidate,
	HigherPhase,
	HigherDeclaredPriority,
	LaterDescriptorOrder,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoserReason {
	LowerPhase,
	LowerDeclaredPriority,
	EarlierDescriptorOrder,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateDecision {
	Winner {
		reason: WinnerReason,
	},
	Loser {
		reason: LoserReason,
		winner_candidate_id: u64,
	},
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanCandidateReference {
	pub candidate_id: u64,
	pub source_member: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCandidate {
	pub candidate: InstallCandidate,
	pub current_winner: EffectiveResult,
	pub proposed_winner: PlanCandidateReference,
	pub decision: CandidateDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
	NewInstall,
	Replacement,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOverlap {
	pub path: DataRelativePath,
	pub proposed_provider: ProviderReference,
	pub overlapping_physical_files: Vec<ProviderReference>,
	pub before: EffectiveResult,
	pub after_operation: EffectiveResult,
	pub hypothetical_enabled: EffectiveResult,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedModState {
	pub mode: InstallMode,
	pub mod_name: ModName,
	pub priority: ModPriority,
	pub list_position: u64,
	pub enabled: bool,
	pub overlaps: Vec<InstallOverlap>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationAssessment {
	pub overlaps: Vec<InstallOverlap>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
	pub archive_identity: ArchiveIdentity,
	pub mod_name: ModName,
	pub replacement: bool,
	pub accepted_choices: Vec<AcceptedChoice>,
	pub warnings: Vec<InstallWarning>,
	pub candidates: Vec<PlannedCandidate>,
	pub projected_state: ProjectedModState,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedInstallation {
	pub source_basename: String,
	pub fomod_schema_version: Option<String>,
	pub plan: InstallPlan,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdditionalSelectionsRequired {
	pub accepted_choices: Vec<AcceptedChoice>,
	pub unresolved_groups: Vec<UnresolvedGroup>,
	pub warnings: Vec<InstallWarning>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPreview {
	pub plan: InstallPlan,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledArchive {
	pub warnings: Vec<InstallWarning>,
}
