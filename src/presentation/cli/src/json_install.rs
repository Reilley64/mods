use crate::json_conflicts;
use crate::json_output;
use application::installation::AcceptedChoice;
use application::installation::AdditionalSelectionsRequired;
use application::installation::CandidateDecision;
use application::installation::ConditionOperator;
use application::installation::ConditionScope;
use application::installation::InstallMode;
use application::installation::InstallPlan;
use application::installation::InstallPreview;
use application::installation::InstallWarning;
use application::installation::LoserReason;
use application::installation::MalformedGroupRepair;
use application::installation::ProjectedModState;
use application::installation::WinnerReason;
use domain::ArchiveIdentity;
use domain::EffectiveResult;
use domain::FileDependencyState;
use domain::FomodCardinality;
use domain::InstallCandidateOrigin;
use domain::InstallationPhase;
use domain::OptionFileTrigger;
use domain::ParticipationReason;
use domain::ProviderReference;
use domain::ResolvedOptionType;
use domain::TombstoneScope;
use serde_json::Value;
use serde_json::json;

pub(crate) fn additional(output: &AdditionalSelectionsRequired) -> Value {
	json!({
	    "outcome": "additional_selections_required", "archive_identity": archive_identity(&output.archive_identity),
	    "mod_name": output.mod_name.as_str(), "accepted_choices": output.accepted_choices.iter().map(choice).collect::<Vec<_>>(),
	    "groups": output.unresolved_groups.iter().map(|group| json!({
		"id": group.id, "label": group.label, "description": group.description,
		"cardinality": cardinality(group.cardinality),
		"options": group.options.iter().map(|option| json!({
		    "id": option.id, "label": option.label, "description": option.description,
		    "resolved_type": option_type(option.resolved_type), "selectable": option.selectable,
		    "synthetic": option.synthetic,
		})).collect::<Vec<_>>(),
	    })).collect::<Vec<_>>(),
	})
}

pub(crate) fn preview(output: &InstallPreview) -> Value {
	json!({"outcome": "preview", "plan": plan(&output.plan), "hypothetical_enabled_conflicts": json_conflicts::list(&output.hypothetical_enabled_conflicts)})
}

pub(crate) fn installed() -> Value {
	json!({"outcome": "installed"})
}

pub(crate) fn warnings(values: &[InstallWarning]) -> Vec<Value> {
	values.iter().map(warning).collect()
}

fn warning(value: &InstallWarning) -> Value {
	let (code, message, details) = match value {
		InstallWarning::FomodCouldBeUsableSelected { group_id, option_id } => (
			"fomod_could_be_usable_selected",
			"a CouldBeUsable option was selected",
			json!({"group_id": group_id, "option_id": option_id}),
		),
		InstallWarning::FomodConflictingFlagValues {
			flag_name,
			values,
			resolved_value,
			..
		} => (
			"fomod_conflicting_flag_values",
			"multiple selected options wrote different flag values",
			json!({"flag_name": flag_name, "values": values, "resolved_value": resolved_value}),
		),
		InstallWarning::FomodEmptyConditionList {
			operator,
			scope,
			group_id,
			option_id,
			pattern_order,
		} => (
			"fomod_empty_condition_list",
			"an empty FOMOD condition list used compatibility behavior",
			json!({
			    "operator": match operator { ConditionOperator::And => "and", ConditionOperator::Or => "or" },
			    "scope": match scope { ConditionScope::Module => "module", ConditionScope::StepVisibility => "step_visibility", ConditionScope::OptionTypePattern => "option_type_pattern", ConditionScope::ConditionalFilePattern => "conditional_file_pattern" },
			    "group_id": group_id, "option_id": option_id, "pattern_order": pattern_order,
			}),
		),
		InstallWarning::FomodMalformedGroupRepaired { group_id, repair } => (
			"fomod_malformed_group_repaired",
			"a malformed FOMOD group was repaired",
			json!({"group_id": group_id, "repair": match repair { MalformedGroupRepair::SingleOptionExactlyOneToSelectAll => "single_option_exactly_one_to_select_all" }}),
		),
		InstallWarning::FomodFommDependencyAssumedCompatible { minimum_version } => (
			"fomod_fomm_dependency_assumed_compatible",
			"a FOMM dependency was assumed compatible",
			json!({"minimum_version": minimum_version}),
		),
		InstallWarning::FomodEmptySourceIgnored {
			descriptor_order,
			group_id,
			option_id,
		} => (
			"fomod_empty_source_ignored",
			"an empty FOMOD source was ignored",
			json!({"descriptor_order": descriptor_order, "group_id": group_id, "option_id": option_id}),
		),
		InstallWarning::FomodEmptyOptionAccepted { group_id, option_id } => (
			"fomod_empty_option_accepted",
			"an option with no effects was accepted",
			json!({"group_id": group_id, "option_id": option_id}),
		),
		InstallWarning::FomodModuleConfigPreferred {
			selected_config_member,
			ignored_config_member,
		} => (
			"fomod_module_config_preferred",
			"ModuleConfig.xml was preferred over script.xml",
			json!({"selected_config_member": selected_config_member, "ignored_config_member": ignored_config_member}),
		),
		InstallWarning::FomodNonPluginFileDependencyPolicyUsed {
			data_relative_path,
			state,
		} => (
			"fomod_non_plugin_file_dependency_policy_used",
			"non-plugin file dependency policy was used",
			json!({"data_relative_path": data_relative_path, "state": match state { FileDependencyState::Missing => "missing", FileDependencyState::Inactive => "inactive", FileDependencyState::Active => "active" }}),
		),
		InstallWarning::FomodEqualPriorityTieResolved {
			destination,
			phase,
			declared_priority,
			winner_candidate_id,
			loser_candidate_ids,
		} => (
			"fomod_equal_priority_tie_resolved",
			"an equal-priority file tie used stable descriptor order",
			json!({
			    "destination": path(destination.as_str()), "phase": phase_name(*phase), "declared_priority": declared_priority,
			    "winner_candidate_id": winner_candidate_id, "loser_candidate_ids": loser_candidate_ids,
			}),
		),
	};
	json_output::warning(code, message, details)
}

fn choice(value: &AcceptedChoice) -> Value {
	json!({"group_id": value.group_id, "option_id": value.option_id})
}

fn cardinality(value: FomodCardinality) -> &'static str {
	match value {
		FomodCardinality::SelectExactlyOne => "select_exactly_one",
		FomodCardinality::SelectAtMostOne => "select_at_most_one",
		FomodCardinality::SelectAtLeastOne => "select_at_least_one",
		FomodCardinality::SelectAny => "select_any",
		FomodCardinality::SelectAll => "select_all",
	}
}

fn option_type(value: ResolvedOptionType) -> &'static str {
	match value {
		ResolvedOptionType::Required => "required",
		ResolvedOptionType::NotUsable => "not_usable",
		ResolvedOptionType::Recommended => "recommended",
		ResolvedOptionType::Optional => "optional",
		ResolvedOptionType::CouldBeUsable => "could_be_usable",
	}
}

fn archive_identity(value: &ArchiveIdentity) -> Value {
	match value {
		ArchiveIdentity::DataArchive {
			archive_sha256,
			package_root,
		} => {
			json!({"kind": "data_archive", "archive_sha256": archive_sha256.as_str(), "package_root": package_root})
		}
		ArchiveIdentity::Fomod {
			archive_sha256,
			package_root,
			config_member,
			config_sha256,
		} => json!({
		    "kind": "fomod", "archive_sha256": archive_sha256.as_str(), "package_root": package_root,
		    "config_member": config_member, "config_sha256": config_sha256.as_str(),
		}),
	}
}

fn plan(value: &InstallPlan) -> Value {
	json!({
	    "archive_identity": archive_identity(&value.archive_identity), "mod_name": value.mod_name.as_str(),
	    "replacement": value.replacement, "accepted_choices": value.accepted_choices.iter().map(choice).collect::<Vec<_>>(),
	    "warnings": warnings(&value.warnings),
	    "candidates": value.candidates.iter().map(|item| {
		let candidate = &item.candidate;
		json!({
		    "candidate_id": candidate.candidate_id, "origin": origin(&candidate.origin), "phase": phase_name(candidate.phase),
		    "declared_priority": candidate.declared_priority, "descriptor_order": candidate.descriptor_order,
		    "source_member": candidate.source_member, "destination": path(candidate.destination.as_str()),
		    "current_winner": effective(&item.current_winner),
		    "proposed_winner": {"candidate_id": item.proposed_winner.candidate_id, "source_member": item.proposed_winner.source_member},
		    "decision": match item.decision {
			CandidateDecision::Winner { reason } => json!({"kind": "winner", "reason": match reason {
			    WinnerReason::OnlyCandidate => "only_candidate", WinnerReason::HigherPhase => "higher_phase",
			    WinnerReason::HigherDeclaredPriority => "higher_declared_priority", WinnerReason::LaterDescriptorOrder => "later_descriptor_order",
			}}),
			CandidateDecision::Loser { reason, winner_candidate_id } => json!({"kind": "loser", "reason": match reason {
			    LoserReason::LowerPhase => "lower_phase", LoserReason::LowerDeclaredPriority => "lower_declared_priority",
			    LoserReason::EarlierDescriptorOrder => "earlier_descriptor_order",
			}, "winner_candidate_id": winner_candidate_id}),
		    },
		})
	    }).collect::<Vec<_>>(),
	    "projected_state": projected(&value.projected_state),
	})
}

fn origin(value: &InstallCandidateOrigin) -> Value {
	match value {
		InstallCandidateOrigin::Required => json!({"kind": "required"}),
		InstallCandidateOrigin::Option {
			group_id,
			option_id,
			trigger,
		} => json!({"kind": "option", "group_id": group_id, "option_id": option_id,
            "trigger": match trigger { OptionFileTrigger::Selected => "selected", OptionFileTrigger::AlwaysInstall => "always_install", OptionFileTrigger::InstallIfUsable => "install_if_usable" }}),
		InstallCandidateOrigin::Conditional { pattern_order, .. } => {
			json!({"kind": "conditional", "pattern_order": pattern_order})
		}
	}
}

fn projected(value: &ProjectedModState) -> Value {
	json!({
	    "mode": match value.mode { InstallMode::NewInstall => "new_install", InstallMode::Replacement => "replacement" },
	    "mod_name": value.mod_name.as_str(), "priority": value.priority.get(), "list_position": value.list_position,
	    "enabled": value.enabled,
	    "overlaps": value.overlaps.iter().map(|overlap| json!({
		"path": path(overlap.path.as_str()), "proposed_provider": provider(&overlap.proposed_provider),
		"overlapping_physical_files": overlap.overlapping_physical_files.iter().map(provider).collect::<Vec<_>>(),
		"before": effective(&overlap.before), "after_operation": effective(&overlap.after_operation),
		"hypothetical_enabled": effective(&overlap.hypothetical_enabled),
	    })).collect::<Vec<_>>(),
	})
}

fn effective(value: &EffectiveResult) -> Value {
	match value {
		EffectiveResult::File(provider_value) => json!({"kind": "file", "provider": provider(provider_value)}),
		EffectiveResult::Absent { controlling_tombstone } => {
			json!({"kind": "absent", "controlling_tombstone": controlling_tombstone.as_ref().map(|tombstone| json!({
			    "scope": match tombstone.scope { TombstoneScope::ExactFile => "exact_file", TombstoneScope::DirectorySubtree => "directory_subtree" },
			    "owner": provider(&tombstone.owner),
			}))})
		}
	}
}

fn provider(value: &ProviderReference) -> Value {
	match value {
		ProviderReference::SteamData { original_path } => {
			json!({"kind": "steam_data", "original_path": path(original_path.as_str())})
		}
		ProviderReference::DataMod {
			mod_name,
			priority,
			original_path,
			participation_reason,
		} => json!({
		    "kind": "data_mod", "mod_name": mod_name.as_str(), "priority": priority.get(), "original_path": path(original_path.as_str()),
		    "participation_reason": match participation_reason {
			ParticipationReason::SteamBase => "steam_base", ParticipationReason::EnabledMod => "enabled_mod",
			ParticipationReason::DisabledMod => "disabled_mod", ParticipationReason::HypotheticalEnabledMod => "hypothetical_enabled_mod",
			ParticipationReason::ProjectedDisabledMod => "projected_disabled_mod", ParticipationReason::Overwrite => "overwrite",
		    },
		}),
		ProviderReference::Overwrite { original_path } => {
			json!({"kind": "overwrite", "original_path": path(original_path.as_str())})
		}
	}
}

fn phase_name(value: InstallationPhase) -> &'static str {
	match value {
		InstallationPhase::Required => "required",
		InstallationPhase::SelectedOrForced => "selected_or_forced",
		InstallationPhase::Conditional => "conditional",
	}
}

fn path(value: &str) -> String {
	value.replace('/', "\\")
}

#[cfg(test)]
mod tests {
	use super::additional;
	use super::preview;
	use super::warnings;
	use application::conflicts::ListEffectiveConflictsOutput;
	use application::installation::AcceptedChoice;
	use application::installation::AdditionalSelectionsRequired;
	use application::installation::ConditionEvaluation;
	use application::installation::InstallMode;
	use application::installation::InstallPlan;
	use application::installation::InstallPreview;
	use application::installation::InstallWarning;
	use application::installation::OptionSelectionState;
	use application::installation::ProjectedModState;
	use application::installation::UnresolvedGroup;
	use application::installation::VisibleOption;
	use domain::ArchiveIdentity;
	use domain::FomodCardinality;
	use domain::FomodCondition;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ResolutionStatus;
	use domain::ResolvedOptionType;
	use domain::Sha256Digest;
	use serde_json::json;

	#[test]
	fn required_choices_preserve_order_and_distinguish_absent_data() -> rootcause::Result<()> {
		let identity = ArchiveIdentity::DataArchive {
			archive_sha256: Sha256Digest::new("a".repeat(64))?,
			package_root: String::new(),
		};
		let option = VisibleOption {
			condition_evaluation: ConditionEvaluation {
				result: true,
				children: vec![],
			},
			condition: FomodCondition::Constant(true),
			flag_effects: vec![],
			file_effects: vec![],
			selection_state: OptionSelectionState::Unselected,
			id: "A".into(),
			label: "Alpha".into(),
			description: "First".into(),
			resolved_type: ResolvedOptionType::Optional,
			selectable: true,
			synthetic: false,
		};
		let output = AdditionalSelectionsRequired {
			archive_identity: identity,
			mod_name: ModName::new("Visuals".to_owned())?,
			automatic_events: vec![],
			resolved_flags: vec![],
			accepted_choices: vec![AcceptedChoice {
				sequence: 0,
				group_id: "previous".into(),
				option_id: "yes".into(),
			}],
			unresolved_groups: vec![UnresolvedGroup {
				condition_evaluation: ConditionEvaluation {
					result: true,
					children: vec![],
				},
				condition: FomodCondition::Constant(true),
				id: "look".into(),
				label: "Look".into(),
				description: "Style".into(),
				cardinality: FomodCardinality::SelectExactlyOne,
				options: vec![option],
			}],
			warnings: vec![],
		};
		let value = additional(&output);
		assert_eq!(value["outcome"], "additional_selections_required");
		assert_eq!(
			value["accepted_choices"],
			json!([{"group_id": "previous", "option_id": "yes"}])
		);
		assert_eq!(value["groups"][0]["options"][0]["selectable"], true);
		assert_eq!(value["groups"][0]["options"][0]["resolved_type"], "optional");
		Ok(())
	}

	#[test]
	fn preview_and_warnings_remain_structured() -> rootcause::Result<()> {
		let warning = InstallWarning::FomodEmptyOptionAccepted {
			group_id: "look".into(),
			option_id: "empty".into(),
		};
		let plan = InstallPlan {
			archive_identity: ArchiveIdentity::DataArchive {
				archive_sha256: Sha256Digest::new("b".repeat(64))?,
				package_root: String::new(),
			},
			mod_name: ModName::new("Visuals".to_owned())?,
			replacement: false,
			accepted_choices: vec![],
			automatic_events: vec![],
			resolved_flags: vec![],
			warnings: vec![warning.clone()],
			candidates: vec![],
			projected_state: ProjectedModState {
				mode: InstallMode::NewInstall,
				mod_name: ModName::new("Visuals".to_owned())?,
				priority: ModPriority::new(0),
				list_position: 0,
				enabled: false,
				overlaps: vec![],
			},
		};
		let output = InstallPreview {
			plan,
			hypothetical_enabled_conflicts: ListEffectiveConflictsOutput {
				resolution_status: ResolutionStatus::Invalid,
				rows: vec![],
				problems: vec![],
			},
		};
		let value = preview(&output);
		assert_eq!(value["outcome"], "preview");
		assert_eq!(
			value["plan"]["warnings"][0]["details"],
			json!({"group_id": "look", "option_id": "empty"})
		);
		assert_eq!(value["hypothetical_enabled_conflicts"]["resolution_status"], "invalid");
		assert_eq!(warnings(&[warning])[0]["code"], "fomod_empty_option_accepted");
		Ok(())
	}
}
