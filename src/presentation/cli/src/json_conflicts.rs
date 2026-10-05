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
use domain::ProviderRank;
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
	json!({"resolution_status": status(output.resolution_status), "rows": output.rows.iter().map(row).collect::<Vec<_>>(), "problems": output.problems.iter().map(problem).collect::<Vec<_>>()})
}

pub(crate) fn inspection(output: &InspectModConflictsOutput) -> Value {
	let summary = &output.provider_summary;
	json!({
	    "mod_name": output.mod_name.as_str(), "participation": participation(output.participation),
	    "resolution_status": status(output.resolution_status),
	    "provider_summary": {
		"ordinary_file_count": summary.ordinary_file_count,
		"effective_file_count": summary.effective_file_count,
		"file_conflict_win_count": summary.file_conflict_win_count,
		"file_conflict_loss_count": summary.file_conflict_loss_count,
		"owned_file_tombstone_count": summary.owned_file_tombstone_count,
		"owned_directory_tombstone_count": summary.owned_directory_tombstone_count,
		"lower_file_entries_suppressed_count": summary.lower_file_entries_suppressed_count,
		"own_file_entries_suppressed_count": summary.own_file_entries_suppressed_count,
		"state": provider_state(summary.state),
	    },
	    "rows": output.rows.iter().map(row).collect::<Vec<_>>(),
	    "problems": output.problems.iter().map(problem).collect::<Vec<_>>(),
	})
}

pub(crate) fn explanation(output: &ExplainPathOutput) -> Value {
	json!({
	    "normalized_key": output.normalized_key,
	    "display_path": path(output.display_path.as_str()),
	    "resolution_status": status(output.resolution_status),
	    "effective_result": output.effective_result.as_ref().map(effective_result),
	    "provider_stack": output.provider_stack.iter().map(provider).collect::<Vec<_>>(),
	    "tombstone_effects": output.tombstone_effects.iter().map(tombstone_effect).collect::<Vec<_>>(),
	    "content_comparisons": output.content_comparisons.iter().map(comparison).collect::<Vec<_>>(),
	    "reasons": output.reasons.iter().map(reason).collect::<Vec<_>>(),
	    "problems": output.problems.iter().map(problem).collect::<Vec<_>>(),
	})
}

fn status(value: ResolutionStatus) -> &'static str {
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

fn path(value: &str) -> String {
	value.replace('/', "\\")
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
		    "kind": "ordinary_conflict", "normalized_key": normalized_key, "display_path": path(display_path.as_str()),
		    "participation": participation(*state), "effective_file": provider(effective_file),
		    "losing_files": losing_files.iter().map(provider).collect::<Vec<_>>(),
		    "content_comparisons": content_comparisons.iter().map(comparison).collect::<Vec<_>>(),
		}),
		ConflictRow::Tombstone {
			normalized_key,
			display_path,
			participation: state,
			controlling_tombstone,
			suppressed_entries,
		} => json!({
		    "kind": "tombstone", "normalized_key": normalized_key, "display_path": path(display_path.as_str()),
		    "participation": participation(*state), "controlling_tombstone": tombstone(controlling_tombstone),
		    "suppressed_entries": suppressed_entries.iter().map(provider).collect::<Vec<_>>(),
		}),
	}
}

fn provider(value: &ProviderReference) -> Value {
	let rank = rank(value.rank());
	match value {
		ProviderReference::SteamData { original_path } => json!({
		    "kind": "steam_data", "priority": rank, "original_path": path(original_path.as_str()), "participation_reason": "steam_base",
		}),
		ProviderReference::DataMod {
			mod_name,
			original_path,
			participation_reason,
			..
		} => json!({
		    "kind": "data_mod", "mod_name": mod_name.as_str(), "priority": rank,
		    "original_path": path(original_path.as_str()), "participation_reason": match participation_reason {
			ParticipationReason::SteamBase => "steam_base",
			ParticipationReason::EnabledMod => "enabled_mod",
			ParticipationReason::DisabledMod => "disabled_mod",
			ParticipationReason::HypotheticalEnabledMod => "hypothetical_enabled_mod",
			ParticipationReason::ProjectedDisabledMod => "projected_disabled_mod",
			ParticipationReason::Overwrite => "overwrite",
		    },
		}),
		ProviderReference::Overwrite { original_path } => json!({
		    "kind": "overwrite", "priority": rank, "original_path": path(original_path.as_str()), "participation_reason": "overwrite",
		}),
	}
}

fn identity(value: &ProviderIdentity) -> Value {
	let rank = rank(value.rank());
	match value {
		ProviderIdentity::SteamData => json!({"kind": "steam_data", "priority": rank}),
		ProviderIdentity::DataMod { mod_name, .. } => {
			json!({"kind": "data_mod", "mod_name": mod_name.as_str(), "priority": rank})
		}
		ProviderIdentity::Overwrite => json!({"kind": "overwrite", "priority": rank}),
	}
}

fn rank(value: ProviderRank) -> Value {
	match value {
		ProviderRank::Base => json!({"kind": "base"}),
		ProviderRank::Regular(value) => json!({"kind": "regular", "priority": value.get()}),
		ProviderRank::Overwrite => json!({"kind": "overwrite"}),
	}
}

fn tombstone(value: &Tombstone) -> Value {
	json!({"scope": match value.scope { TombstoneScope::ExactFile => "exact_file", TombstoneScope::DirectorySubtree => "directory_subtree" }, "owner": provider(&value.owner)})
}

fn effective_result(value: &EffectiveResult) -> Value {
	match value {
		EffectiveResult::File(file) => json!({"kind": "file", "provider": provider(file)}),
		EffectiveResult::Absent { controlling_tombstone } => {
			json!({"kind": "absent", "controlling_tombstone": controlling_tombstone.as_ref().map(tombstone)})
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
	let mut result = json!({"state": state, "winner": provider(value.winner()), "loser": provider(value.loser())});
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

fn tombstone_effect(value: &TombstoneEffect) -> Value {
	match value {
		TombstoneEffect::Controlling {
			tombstone: controlling,
			suppressed_entries,
		} => json!({
		    "kind": "controlling", "tombstone": tombstone(controlling),
		    "suppressed_entries": suppressed_entries.iter().map(provider).collect::<Vec<_>>(),
		}),
		TombstoneEffect::ShadowedByTombstone {
			tombstone: shadowed,
			controlling_tombstone,
		} => json!({
		    "kind": "shadowed_by_tombstone", "tombstone": tombstone(shadowed), "controlling_tombstone": tombstone(controlling_tombstone),
		}),
		TombstoneEffect::OverriddenByFile {
			tombstone: overridden,
			overriding_file,
		} => json!({
		    "kind": "overridden_by_file", "tombstone": tombstone(overridden), "overriding_file": provider(overriding_file),
		}),
		TombstoneEffect::Orphan { tombstone: orphan } => {
			json!({"kind": "orphan", "tombstone": tombstone(orphan)})
		}
	}
}

fn reason(value: &ResolutionReason) -> Value {
	match value {
		ResolutionReason::EffectiveFile { provider: file } => {
			json!({"kind": "effective_file", "provider": provider(file)})
		}
		ResolutionReason::LowerPriorityFile { provider: file, winner } => {
			json!({"kind": "lower_priority_file", "provider": provider(file), "winner": provider(winner)})
		}
		ResolutionReason::SuppressedByTombstone {
			provider: file,
			controlling_tombstone,
		} => {
			json!({"kind": "suppressed_by_tombstone", "provider": provider(file), "controlling_tombstone": tombstone(controlling_tombstone)})
		}
		ResolutionReason::AbsentNoEntry => json!({"kind": "absent_no_entry"}),
		ResolutionReason::AbsentByTombstone { controlling_tombstone } => {
			json!({"kind": "absent_by_tombstone", "controlling_tombstone": tombstone(controlling_tombstone)})
		}
		ResolutionReason::ShadowedTombstone {
			tombstone: shadowed,
			controlling_tombstone,
		} => {
			json!({"kind": "shadowed_tombstone", "tombstone": tombstone(shadowed), "controlling_tombstone": tombstone(controlling_tombstone)})
		}
		ResolutionReason::TombstoneOverriddenByFile {
			tombstone: overridden,
			overriding_file,
		} => {
			json!({"kind": "tombstone_overridden_by_file", "tombstone": tombstone(overridden), "overriding_file": provider(overriding_file)})
		}
		ResolutionReason::NamespaceInvalid => json!({"kind": "namespace_invalid"}),
	}
}

fn problem(value: &ConflictProblem) -> Value {
	let kind = match value.kind {
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
	};
	let scope = match &value.scope {
		ProblemScope::Global => json!({"kind": "global"}),
		ProblemScope::Modlist => json!({"kind": "modlist"}),
		ProblemScope::Provider(value) => json!({"kind": "provider", "provider": identity(value)}),
		ProblemScope::Path {
			normalized_key,
			display_path,
		} => {
			json!({"kind": "path", "normalized_key": normalized_key, "display_path": path(display_path.as_str())})
		}
		ProblemScope::Subtree {
			normalized_key,
			display_path,
		} => {
			json!({"kind": "subtree", "normalized_key": normalized_key, "display_path": path(display_path.as_str())})
		}
	};
	json!({"kind": kind, "scope": scope})
}

fn provider_state(value: ProviderState) -> &'static str {
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

#[cfg(test)]
mod tests {
	use super::explanation;
	use super::list;
	use application::conflicts::ExplainPathOutput;
	use application::conflicts::ListEffectiveConflictsOutput;
	use domain::ConflictProblem;
	use domain::ConflictProblemKind;
	use domain::ConflictRow;
	use domain::ContentComparison;
	use domain::DataRelativePath;
	use domain::EffectiveResult;
	use domain::Participation;
	use domain::ProblemScope;
	use domain::ProviderReference;
	use domain::ResolutionReason;
	use domain::ResolutionStatus;
	use domain::Sha256Digest;
	use serde_json::json;

	#[test]
	fn conflicts_keep_problems_and_content_state_even_at_exit_zero() -> rootcause::Result<()> {
		let display_path = DataRelativePath::new("Textures/A.dds".to_owned())?;
		let winner = ProviderReference::Overwrite {
			original_path: display_path.clone(),
		};
		let loser = ProviderReference::SteamData {
			original_path: display_path.clone(),
		};
		let comparison = ContentComparison::DifferentSha256 {
			winner: winner.clone(),
			loser: loser.clone(),
			winner_sha256: Sha256Digest::new("a".repeat(64))?,
			loser_sha256: Sha256Digest::new("b".repeat(64))?,
		};
		let problem = ConflictProblem {
			kind: ConflictProblemKind::ProviderMissing,
			scope: ProblemScope::Path {
				normalized_key: "textures/a.dds".into(),
				display_path: display_path.clone(),
			},
		};
		let value = list(&ListEffectiveConflictsOutput {
			resolution_status: ResolutionStatus::Invalid,
			rows: vec![ConflictRow::OrdinaryConflict {
				normalized_key: "textures/a.dds".into(),
				display_path: display_path.clone(),
				participation: Participation::Active,
				effective_file: winner.clone(),
				losing_files: vec![loser.clone()],
				content_comparisons: vec![comparison.clone()],
			}],
			problems: vec![problem.clone()],
		});
		assert_eq!(value["resolution_status"], "invalid");
		assert_eq!(value["rows"][0]["content_comparisons"][0]["state"], "different_sha256");
		assert_eq!(
			value["rows"][0]["content_comparisons"][0]["winner_sha256"],
			"a".repeat(64)
		);
		assert_eq!(value["rows"][0]["losing_files"][0]["priority"], json!({"kind": "base"}));
		assert_eq!(value["problems"][0]["scope"]["display_path"], "Textures\\A.dds");

		let explained = explanation(&ExplainPathOutput {
			normalized_key: "textures/a.dds".into(),
			display_path,
			resolution_status: ResolutionStatus::Invalid,
			effective_result: Some(EffectiveResult::File(winner.clone())),
			provider_stack: vec![winner, loser],
			tombstone_effects: vec![],
			content_comparisons: vec![comparison],
			reasons: vec![ResolutionReason::NamespaceInvalid],
			problems: vec![problem],
		});
		assert_eq!(explained["effective_result"]["kind"], "file");
		assert_eq!(explained["reasons"][0]["kind"], "namespace_invalid");
		Ok(())
	}
}
