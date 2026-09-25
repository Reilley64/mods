use application::conflicts::ExplainPathOutput;
use application::conflicts::InspectModConflictsOutput;
use application::conflicts::ListEffectiveConflictsOutput;
use domain::ConflictProblem;
use domain::ConflictProblemKind;
use domain::ConflictRow;
use domain::ContentComparison;
use domain::ContentState;
use domain::EffectiveResult;
use domain::Participation;
use domain::ParticipationReason;
use domain::ProblemScope;
use domain::ProviderIdentity;
use domain::ProviderReference;
use domain::ProviderState;
use domain::ResolutionReason;
use domain::ResolutionStatus;
use domain::Tombstone;
use domain::TombstoneEffect;
use domain::TombstoneScope;
use serde_json::Value;
use serde_json::json;

pub(crate) fn list(output: &ListEffectiveConflictsOutput) -> Value {
	json!({"outcome": "complete", "resolution_status": resolution_status(output.resolution_status),
		"rows": output.rows.iter().map(row).collect::<Vec<_>>(),
		"problems": output.problems.iter().map(problem).collect::<Vec<_>>()})
}

pub(crate) fn explanation(output: &ExplainPathOutput) -> Value {
	let mut result = json!({"outcome": "complete", "resolution_status": resolution_status(output.resolution_status),
		"normalized_key": output.normalized_key, "display_path": output.display_path.as_str(),
		"provider_stack": output.provider_stack.iter().map(provider).collect::<Vec<_>>(),
		"tombstone_effects": output.tombstone_effects.iter().map(effect).collect::<Vec<_>>(),
		"content_comparisons": output.content_comparisons.iter().map(comparison).collect::<Vec<_>>(),
		"reasons": output.reasons.iter().map(reason).collect::<Vec<_>>(),
		"problems": output.problems.iter().map(problem).collect::<Vec<_>>()});

	if let Some(effective) = &output.effective_result {
		result["effective_result"] = effective_result(effective);
	}

	result
}

fn effect(value: &TombstoneEffect) -> Value {
	match value {
		TombstoneEffect::Controlling {
			tombstone: item,
			suppressed_entries,
		} => {
			json!({"kind": "controlling", "tombstone": tombstone(item), "suppressed_entries": suppressed_entries.iter().map(provider).collect::<Vec<_>>()})
		}
		TombstoneEffect::ShadowedByTombstone {
			tombstone: item,
			controlling_tombstone,
		} => {
			json!({"kind": "shadowed_by_tombstone", "tombstone": tombstone(item), "controlling_tombstone": tombstone(controlling_tombstone)})
		}
		TombstoneEffect::OverriddenByFile {
			tombstone: item,
			overriding_file,
		} => {
			json!({"kind": "overridden_by_file", "tombstone": tombstone(item), "overriding_file": provider(overriding_file)})
		}
		TombstoneEffect::Orphan { tombstone: item } => json!({"kind": "orphan", "tombstone": tombstone(item)}),
	}
}

fn reason(value: &ResolutionReason) -> Value {
	match value {
		ResolutionReason::EffectiveFile { provider: item } => {
			json!({"kind": "effective_file", "provider": provider(item)})
		}
		ResolutionReason::LowerPriorityFile { provider: item, winner } => {
			json!({"kind": "lower_priority_file", "provider": provider(item), "winner": provider(winner)})
		}
		ResolutionReason::SuppressedByTombstone {
			provider: item,
			controlling_tombstone,
		} => {
			json!({"kind": "suppressed_by_tombstone", "provider": provider(item), "controlling_tombstone": tombstone(controlling_tombstone)})
		}
		ResolutionReason::AbsentNoEntry => json!({"kind": "absent_no_entry"}),
		ResolutionReason::AbsentByTombstone { controlling_tombstone } => {
			json!({"kind": "absent_by_tombstone", "controlling_tombstone": tombstone(controlling_tombstone)})
		}
		ResolutionReason::ShadowedTombstone {
			tombstone: item,
			controlling_tombstone,
		} => {
			json!({"kind": "shadowed_tombstone", "tombstone": tombstone(item), "controlling_tombstone": tombstone(controlling_tombstone)})
		}
		ResolutionReason::TombstoneOverriddenByFile {
			tombstone: item,
			overriding_file,
		} => {
			json!({"kind": "tombstone_overridden_by_file", "tombstone": tombstone(item), "overriding_file": provider(overriding_file)})
		}
		ResolutionReason::NamespaceInvalid => json!({"kind": "namespace_invalid"}),
	}
}

pub(crate) fn inspection(output: &InspectModConflictsOutput) -> Value {
	let summary = &output.provider_summary;
	json!({"outcome": "complete", "mod_name": output.mod_name.as_str(), "participation": participation(output.participation),
	"resolution_status": resolution_status(output.resolution_status),
	"rows": output.rows.iter().map(row).collect::<Vec<_>>(), "problems": output.problems.iter().map(problem).collect::<Vec<_>>(),
	"provider_summary": {
		"ordinary_file_count": summary.ordinary_file_count,
		"effective_file_count": summary.effective_file_count,
		"file_conflict_win_count": summary.file_conflict_win_count,
		"file_conflict_loss_count": summary.file_conflict_loss_count,
		"owned_file_tombstone_count": summary.owned_file_tombstone_count,
		"owned_directory_tombstone_count": summary.owned_directory_tombstone_count,
		"lower_file_entries_suppressed_count": summary.lower_file_entries_suppressed_count,
		"own_file_entries_suppressed_count": summary.own_file_entries_suppressed_count,
		"state": provider_state(summary.state)
	}})
}

const fn provider_state(value: ProviderState) -> &'static str {
	match value {
		ProviderState::Invalid => "invalid",
		ProviderState::Inactive => "inactive",
		ProviderState::SuppressionOnly => "suppression_only",
		ProviderState::Empty => "empty",
		ProviderState::FullyOverridden => "fully_overridden",
		ProviderState::Mixed => "mixed",
		ProviderState::WinningConflicts => "winning_conflicts",
		ProviderState::LosingConflicts => "losing_conflicts",
		ProviderState::Uncontested => "uncontested",
	}
}

fn row(value: &ConflictRow) -> Value {
	match value {
		ConflictRow::OrdinaryConflict {
			normalized_key,
			display_path,
			participation: state,
			effective_file,
			losing_files,
			content_comparisons,
		} => json!({
			"kind": "ordinary_conflict", "normalized_key": normalized_key, "display_path": display_path.as_str(),
			"participation": participation(*state), "effective_file": provider(effective_file),
			"losing_files": losing_files.iter().map(provider).collect::<Vec<_>>(),
			"content_comparisons": content_comparisons.iter().map(comparison).collect::<Vec<_>>()
		}),
		ConflictRow::Tombstone {
			normalized_key,
			display_path,
			participation: state,
			controlling_tombstone,
			suppressed_entries,
		} => json!({
			"kind": "tombstone", "normalized_key": normalized_key, "display_path": display_path.as_str(),
			"participation": participation(*state), "controlling_tombstone": tombstone(controlling_tombstone),
			"suppressed_entries": suppressed_entries.iter().map(provider).collect::<Vec<_>>()
		}),
	}
}

pub(crate) fn provider(value: &ProviderReference) -> Value {
	match value {
		ProviderReference::SteamData { original_path } => {
			json!({"kind": "steam_data", "priority": {"kind": "base"}, "original_path": original_path.as_str(), "participation_reason": "steam_base"})
		}
		ProviderReference::Overwrite { original_path } => {
			json!({"kind": "overwrite", "priority": {"kind": "overwrite"}, "original_path": original_path.as_str(), "participation_reason": "overwrite"})
		}
		ProviderReference::DataMod {
			mod_name,
			priority,
			original_path,
			participation_reason: reason,
		} => {
			json!({"kind": "data_mod", "mod_name": mod_name.as_str(), "priority": {"kind": "regular", "priority": priority.get()}, "original_path": original_path.as_str(), "participation_reason": participation_reason(*reason)})
		}
	}
}

fn identity(value: &ProviderIdentity) -> Value {
	match value {
		ProviderIdentity::SteamData => json!({"kind": "steam_data", "priority": {"kind": "base"}}),
		ProviderIdentity::Overwrite => json!({"kind": "overwrite", "priority": {"kind": "overwrite"}}),
		ProviderIdentity::DataMod { mod_name, priority } => {
			json!({"kind": "data_mod", "mod_name": mod_name.as_str(), "priority": {"kind": "regular", "priority": priority.get()}})
		}
	}
}

fn tombstone(value: &Tombstone) -> Value {
	let kind = match value.scope {
		TombstoneScope::ExactFile => "exact_file",
		TombstoneScope::DirectorySubtree => "directory_subtree",
	};
	json!({"kind": kind, "owner": provider(&value.owner)})
}

pub(crate) fn effective_result(value: &EffectiveResult) -> Value {
	match value {
		EffectiveResult::File(file) => json!({"kind": "file", "provider": provider(file)}),
		EffectiveResult::Absent { controlling_tombstone } => {
			let mut result = json!({"kind": "absent"});
			if let Some(controlling) = controlling_tombstone {
				result["controlling_tombstone"] = tombstone(controlling);
			}
			result
		}
	}
}

fn comparison(value: &ContentComparison) -> Value {
	let state = match value.state() {
		ContentState::NotCompared => "not_compared",
		ContentState::SameSha256 => "same_sha256",
		ContentState::DifferentSha256 => "different_sha256",
		ContentState::Unavailable => "unavailable",
		ContentState::Unstable => "unstable",
	};
	let mut result =
		json!({"content_state": state, "winner": provider(value.winner()), "loser": provider(value.loser())});
	if let ContentComparison::SameSha256 {
		winner_sha256,
		loser_sha256,
		..
	}
	| ContentComparison::DifferentSha256 {
		winner_sha256,
		loser_sha256,
		..
	} = value
	{
		result["winner_sha256"] = json!(winner_sha256.as_str());
		result["loser_sha256"] = json!(loser_sha256.as_str());
	}

	result
}

fn problem(value: &ConflictProblem) -> Value {
	let scope = match &value.scope {
		ProblemScope::Global => json!({"kind": "global"}),
		ProblemScope::Modlist => json!({"kind": "modlist"}),
		ProblemScope::Provider(provider) => json!({"kind": "provider", "provider": identity(provider)}),
		ProblemScope::Path {
			normalized_key,
			display_path,
		} => json!({"kind": "path", "normalized_key": normalized_key, "display_path": display_path.as_str()}),
		ProblemScope::Subtree {
			normalized_key,
			display_path,
		} => json!({"kind": "subtree", "normalized_key": normalized_key, "display_path": display_path.as_str()}),
	};
	json!({"kind": problem_kind(value.kind), "scope": scope})
}

fn resolution_status(value: ResolutionStatus) -> &'static str {
	match value {
		ResolutionStatus::Exact => "exact",
		ResolutionStatus::Invalid => "invalid",
	}
}

fn participation(value: Participation) -> &'static str {
	match value {
		Participation::Active => "active",
		Participation::Inactive => "inactive",
		Participation::Hypothetical => "hypothetical",
	}
}

fn participation_reason(value: ParticipationReason) -> &'static str {
	match value {
		ParticipationReason::SteamBase => "steam_base",
		ParticipationReason::EnabledMod => "enabled_mod",
		ParticipationReason::DisabledMod => "disabled_mod",
		ParticipationReason::HypotheticalEnabledMod => "hypothetical_enabled_mod",
		ParticipationReason::ProjectedDisabledMod => "projected_disabled_mod",
		ParticipationReason::Overwrite => "overwrite",
	}
}

fn problem_kind(value: ConflictProblemKind) -> &'static str {
	match value {
		ConflictProblemKind::ModlistInvalid => "modlist_invalid",
		ConflictProblemKind::ProviderMissing => "provider_missing",
		ConflictProblemKind::InternalKeyCollision => "internal_key_collision",
		ConflictProblemKind::FileDirectoryCollision => "file_directory_collision",
		ConflictProblemKind::OrdinaryTombstoneCollision => "ordinary_tombstone_collision",
		ConflictProblemKind::DirectoryTombstoneDescendantCollision => {
			"directory_tombstone_descendant_collision"
		}
		ConflictProblemKind::InvalidTombstoneMetadata => "invalid_tombstone_metadata",
		ConflictProblemKind::InvalidTombstonePath => "invalid_tombstone_path",
		ConflictProblemKind::ReparsePoint => "reparse_point",
		ConflictProblemKind::HardLink => "hard_link",
		ConflictProblemKind::UnsupportedEntryType => "unsupported_entry_type",
		ConflictProblemKind::ContainmentEscape => "containment_escape",
		ConflictProblemKind::NonLosslessName => "non_lossless_name",
		ConflictProblemKind::ReservedPath => "reserved_path",
	}
}

#[cfg(test)]
mod tests {
	use super::explanation;
	use super::inspection;
	use super::list;
	use crate::contract::Contracts;
	use application::conflicts::ExplainPathOutput;
	use application::conflicts::InspectModConflictsOutput;
	use application::conflicts::ListEffectiveConflictsOutput;
	use domain::ConflictProblem;
	use domain::ConflictProblemKind;
	use domain::ConflictRow;
	use domain::DataRelativePath;
	use domain::EffectiveResult;
	use domain::ModName;
	use domain::Participation;
	use domain::ProblemScope;
	use domain::ProviderReference;
	use domain::ProviderState;
	use domain::ProviderSummary;
	use domain::ResolutionReason;
	use domain::ResolutionStatus;
	use domain::Tombstone;
	use domain::TombstoneScope;
	use rootcause::Result;
	use serde_json::json;

	#[test]
	fn conflict_list_preserves_tombstone_scope_and_suppressed_provider() -> Result<()> {
		let path = DataRelativePath::new("textures/a.dds".to_owned())?;
		let owner = ProviderReference::Overwrite {
			original_path: DataRelativePath::new("textures".to_owned())?,
		};
		let suppressed = ProviderReference::SteamData {
			original_path: path.clone(),
		};
		let output = ListEffectiveConflictsOutput {
			resolution_status: ResolutionStatus::Exact,
			rows: vec![ConflictRow::Tombstone {
				normalized_key: path.comparison_key().to_owned(),
				display_path: path,
				participation: Participation::Active,
				controlling_tombstone: Tombstone {
					scope: TombstoneScope::DirectorySubtree,
					owner,
				},
				suppressed_entries: vec![suppressed],
			}],
			problems: Vec::new(),
		};

		let wire = list(&output);
		assert!(Contracts::new()?.output_matches("mods_conflicts_list", &wire));
		assert_eq!(
			wire.pointer("/rows/0/controlling_tombstone/kind"),
			Some(&json!("directory_subtree"))
		);
		assert_eq!(
			wire.pointer("/rows/0/controlling_tombstone/owner/original_path"),
			Some(&json!("textures"))
		);
		assert_eq!(
			wire.pointer("/rows/0/suppressed_entries/0/kind"),
			Some(&json!("steam_data"))
		);
		Ok(())
	}
	#[test]
	fn path_explanation_distinguishes_unknown_from_invalid_namespace() -> Result<()> {
		let mut output = ExplainPathOutput {
			normalized_key: "missing.txt".to_owned(),
			display_path: DataRelativePath::new("missing.txt".to_owned())?,
			resolution_status: ResolutionStatus::Exact,
			effective_result: Some(EffectiveResult::Absent {
				controlling_tombstone: None,
			}),
			provider_stack: Vec::new(),
			tombstone_effects: Vec::new(),
			content_comparisons: Vec::new(),
			reasons: vec![ResolutionReason::AbsentNoEntry],
			problems: Vec::new(),
		};
		let contracts = Contracts::new()?;

		let unknown = explanation(&output);
		assert!(contracts.output_matches("mods_conflicts_explain", &unknown));
		assert_eq!(unknown["effective_result"], json!({"kind": "absent"}));

		output.resolution_status = ResolutionStatus::Invalid;
		output.effective_result = None;
		output.reasons = vec![ResolutionReason::NamespaceInvalid];
		output.problems.push(ConflictProblem {
			kind: ConflictProblemKind::ModlistInvalid,
			scope: ProblemScope::Modlist,
		});

		let invalid = explanation(&output);
		assert!(contracts.output_matches("mods_conflicts_explain", &invalid));
		assert!(invalid.get("effective_result").is_none());
		Ok(())
	}

	#[test]
	fn inspection_keeps_hypothetical_participation_and_inactive_summary() -> Result<()> {
		let output = InspectModConflictsOutput {
			mod_name: ModName::new("optional".to_owned())?,
			participation: Participation::Hypothetical,
			resolution_status: ResolutionStatus::Exact,
			provider_summary: ProviderSummary {
				ordinary_file_count: 4,
				effective_file_count: 3,
				file_conflict_win_count: 2,
				file_conflict_loss_count: 1,
				owned_file_tombstone_count: 5,
				owned_directory_tombstone_count: 6,
				lower_file_entries_suppressed_count: 7,
				own_file_entries_suppressed_count: 8,
				state: ProviderState::Inactive,
			},
			rows: Vec::new(),
			problems: Vec::new(),
		};

		let wire = inspection(&output);
		assert!(Contracts::new()?.output_matches("mods_conflicts_inspect", &wire));
		assert_eq!(wire["participation"], "hypothetical");
		assert_eq!(wire["provider_summary"]["state"], "inactive");
		assert_eq!(wire["provider_summary"]["own_file_entries_suppressed_count"], 8);
		Ok(())
	}
}
