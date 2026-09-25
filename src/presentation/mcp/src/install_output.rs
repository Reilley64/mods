use crate::conflict_output::effective_result;
use crate::conflict_output::list as conflict_list;
use crate::conflict_output::provider;
use application::installation::AcceptedChoice;
use application::installation::AdditionalSelectionsRequired;
use application::installation::AutomaticAction;
use application::installation::AutomaticCause;
use application::installation::AutomaticChoiceEvent;
use application::installation::CandidateDecision;
use application::installation::ChoiceSource;
use application::installation::ConditionEvaluation;
use application::installation::ConditionOperator;
use application::installation::ConditionScope;
use application::installation::FlagWriter;
use application::installation::InstallMode;
use application::installation::InstallPlan;
use application::installation::InstallPreview;
use application::installation::InstallWarning;
use application::installation::InstalledArchive;
use application::installation::LoserReason;
use application::installation::OptionSelectionState;
use application::installation::PlannedCandidate;
use application::installation::ProjectedModState;
use application::installation::ResolvedFlag;
use application::installation::UnresolvedGroup;
use application::installation::WinnerReason;
use domain::ArchiveIdentity;
use domain::FileDependencyState;
use domain::FomodCardinality;
use domain::FomodCondition;
use domain::InstallCandidateOrigin;
use domain::InstallationPhase;
use domain::OptionFileTrigger;
use domain::ResolvedOptionType;
use rmcp::ErrorData;
use serde_json::Value;
use serde_json::json;

pub(crate) fn additional_selections(output: &AdditionalSelectionsRequired) -> Result<Value, ErrorData> {
	let groups = output
		.unresolved_groups
		.iter()
		.map(group)
		.collect::<Result<Vec<_>, _>>()?;
	let requirements = output
		.unresolved_groups
		.iter()
		.map(|group| {
			json!({
			 "group_id": group.id, "cardinality": cardinality(group.cardinality),
			 "accepts_synthetic_none": group.options.iter().any(|option| option.synthetic)
			})
		})
		.collect::<Vec<_>>();

	Ok(
		json!({"outcome": "additional_selections_required", "archive_identity": identity(&output.archive_identity),
  "mod_name": output.mod_name.as_str(), "accepted_choices": output.accepted_choices.iter().map(choice).collect::<Vec<_>>(),
  "automatic_events": output.automatic_events.iter().map(automatic).collect::<Vec<_>>(),
  "resolved_flags": output.resolved_flags.iter().map(flag).collect::<Vec<_>>(),
  "groups": groups, "warnings": output.warnings.iter().map(warning).collect::<Vec<_>>(), "rerun_requirements": requirements}),
	)
}

pub(crate) fn preview(output: &InstallPreview) -> Result<Value, ErrorData> {
	let mut conflicts = conflict_list(&output.hypothetical_enabled_conflicts);
	if let Some(fields) = conflicts.as_object_mut() {
		fields.remove("outcome");
	}

	Ok(json!({"outcome": "preview", "plan": plan(&output.plan)?,
		"projected_state": state(&output.plan.projected_state, "projected"),
		"hypothetical_enabled_conflicts": conflicts}))
}

pub(crate) fn installed(output: &InstalledArchive) -> Result<Value, ErrorData> {
	let mut conflicts = conflict_list(&output.conflicts);
	if let Some(fields) = conflicts.as_object_mut() {
		fields.remove("outcome");
	}

	Ok(json!({"outcome": "installed", "plan": plan(&output.plan)?,
		"committed_state": state(&output.plan.projected_state, "committed"),
		"conflicts": conflicts}))
}

fn plan(plan: &InstallPlan) -> Result<Value, ErrorData> {
	let candidates = plan.candidates.iter().map(candidate).collect::<Result<Vec<_>, _>>()?;

	Ok(json!({
		"archive_identity": identity(&plan.archive_identity), "mod_name": plan.mod_name.as_str(), "replace": plan.replacement,
		"accepted_choices": plan.accepted_choices.iter().map(choice).collect::<Vec<_>>(),
		"automatic_events": plan.automatic_events.iter().map(automatic).collect::<Vec<_>>(),
		"resolved_flags": plan.resolved_flags.iter().map(flag).collect::<Vec<_>>(),
		"warnings": plan.warnings.iter().map(warning).collect::<Vec<_>>(), "candidates": candidates
	}))
}

fn state(state: &ProjectedModState, kind: &str) -> Value {
	let overlaps = state
		.overlaps
		.iter()
		.map(|overlap| {
			json!({
				"normalized_key": overlap.path.comparison_key(), "display_path": overlap.path.as_str(),
				"proposed_provider": provider(&overlap.proposed_provider),
				"overlapping_physical_files": overlap.overlapping_physical_files.iter().map(provider).collect::<Vec<_>>(),
				"before": effective_result(&overlap.before),
				"after_operation": effective_result(&overlap.after_operation),
				"hypothetical_enabled": effective_result(&overlap.hypothetical_enabled)
			})
		})
		.collect::<Vec<_>>();

	json!({"kind": kind, "mode": match state.mode { InstallMode::NewInstall => "new_install", InstallMode::Replacement => "replacement" },
		"mod_name": state.mod_name.as_str(), "priority": state.priority.get(), "list_position": state.list_position,
		"enabled": state.enabled, "overlaps": overlaps})
}

fn identity(value: &ArchiveIdentity) -> Value {
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
		} => {
			json!({"kind": "fomod", "archive_sha256": archive_sha256.as_str(), "package_root": package_root, "config_member": config_member, "config_sha256": config_sha256.as_str()})
		}
	}
}
fn choice(value: &AcceptedChoice) -> Value {
	json!({"kind": "supplied", "sequence": value.sequence, "group_id": value.group_id, "option_id": value.option_id})
}
fn automatic(value: &AutomaticChoiceEvent) -> Value {
	json!({"kind": "automatic", "sequence": value.sequence, "action": match value.action { AutomaticAction::Selected => "selected", AutomaticAction::Withdrawn => "withdrawn" },
 "group_id": value.group_id, "option_id": value.option_id,
 "active_causes": value.active_causes.iter().map(|cause| match cause { AutomaticCause::Required => "required", AutomaticCause::SelectAll => "select_all" }).collect::<Vec<_>>()})
}
fn writer(value: &FlagWriter) -> Value {
	json!({"sequence": value.sequence, "flag_effect_order": value.flag_effect_order,
 "source": match value.source { ChoiceSource::Supplied => "supplied", ChoiceSource::Automatic => "automatic" },
 "group_id": value.group_id, "option_id": value.option_id, "value": value.value})
}
fn flag(value: &ResolvedFlag) -> Value {
	json!({"name": value.name, "value": value.value, "winning_event": writer(&value.winning_event)})
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
fn phase(value: InstallationPhase) -> &'static str {
	match value {
		InstallationPhase::Required => "required",
		InstallationPhase::SelectedOrForced => "selected_or_forced",
		InstallationPhase::Conditional => "conditional",
	}
}
fn file_state(value: FileDependencyState) -> &'static str {
	match value {
		FileDependencyState::Missing => "missing",
		FileDependencyState::Inactive => "inactive",
		FileDependencyState::Active => "active",
	}
}
fn condition(value: &FomodCondition, evaluation: &ConditionEvaluation) -> Result<Value, ErrorData> {
	let mut output = match value {
		FomodCondition::Constant(value) => json!({"kind": "constant", "value": value}),
		FomodCondition::All(children) | FomodCondition::Any(children) => {
			if children.len() != evaluation.children.len() {
				return Err(ErrorData::internal_error("condition facts are incomplete", None));
			}

			let children = children
				.iter()
				.zip(&evaluation.children)
				.map(|(child, facts)| condition(child, facts))
				.collect::<Result<Vec<_>, _>>()?;

			json!({"kind": if matches!(value, FomodCondition::All(_)) { "and" } else { "or" }, "children": children})
		}
		FomodCondition::FileDependency { path, state } => {
			json!({"kind": "file_dependency", "path": path, "state": file_state(*state)})
		}
		FomodCondition::FlagDependency { name, value } => {
			json!({"kind": "flag_dependency", "name": name, "value": value})
		}
		FomodCondition::GameDependency { minimum_version } => {
			json!({"kind": "game_dependency", "minimum_version": minimum_version})
		}
		FomodCondition::NvseDependency { minimum_version } => {
			json!({"kind": "nvse_dependency", "minimum_version": minimum_version})
		}
		FomodCondition::FommDependency { minimum_version } => {
			json!({"kind": "fomm_dependency", "minimum_version": minimum_version})
		}
	};
	output["result"] = json!(evaluation.result);

	Ok(output)
}
fn group(value: &UnresolvedGroup) -> Result<Value, ErrorData> {
	let mut options = Vec::new();
	for option in &value.options {
		let mut effects = option
			.flag_effects
			.iter()
			.map(|flag| json!({"kind": "flag", "name": flag.name, "value": flag.value}))
			.collect::<Vec<_>>();
		effects.extend(option.file_effects.iter().map(|file| json!({"kind": if file.folder { "folder" } else { "file" },
   "descriptor_order": file.descriptor_order, "source": file.source, "destination": file.destination,
   "declared_priority": file.declared_priority, "always_install": file.always_install, "install_if_usable": file.install_if_usable})));

		let mut mapped = json!({"kind": if option.synthetic { "synthetic_none" } else { "option" }, "option_id": option.id,
   "label": option.label, "description": option.description, "selectable": option.selectable,
   "selection_state": match option.selection_state { OptionSelectionState::Unselected => "unselected", OptionSelectionState::Supplied => "supplied", OptionSelectionState::AutomaticRequired => "automatic_required", OptionSelectionState::AutomaticSelectAll => "automatic_select_all" },
   "conditions": condition(&option.condition, &option.condition_evaluation)?, "effects": effects});
		if !option.synthetic {
			mapped["resolved_type"] = json!(match option.resolved_type {
				ResolvedOptionType::Required => "required",
				ResolvedOptionType::NotUsable => "not_usable",
				ResolvedOptionType::Recommended => "recommended",
				ResolvedOptionType::Optional => "optional",
				ResolvedOptionType::CouldBeUsable => "could_be_usable",
			});
		}
		options.push(mapped);
	}

	Ok(
		json!({"group_id": value.id, "label": value.label, "description": value.description,
 "cardinality": cardinality(value.cardinality), "visibility_condition": condition(&value.condition, &value.condition_evaluation)?, "options": options}),
	)
}
fn candidate(value: &PlannedCandidate) -> Result<Value, ErrorData> {
	let item = &value.candidate;
	let origin = match &item.origin {
		InstallCandidateOrigin::Required => json!({"kind": "required"}),
		InstallCandidateOrigin::Option {
			group_id,
			option_id,
			trigger,
		} => json!({"kind": "option", "group_id": group_id, "option_id": option_id,
   "trigger": match trigger { OptionFileTrigger::Selected => "selected", OptionFileTrigger::AlwaysInstall => "always_install", OptionFileTrigger::InstallIfUsable => "install_if_usable" }}),
		InstallCandidateOrigin::Conditional {
			pattern_order,
			condition: predicate,
		} => {
			let facts = value
				.origin_condition_evaluation
				.as_ref()
				.ok_or_else(|| ErrorData::internal_error("condition facts are incomplete", None))?;
			json!({"kind": "conditional", "pattern_order": pattern_order, "condition": condition(predicate, facts)?})
		}
	};
	let decision = match value.decision {
		CandidateDecision::Winner { reason } => {
			json!({"kind": "winner", "reason": match reason { WinnerReason::OnlyCandidate => "only_candidate", WinnerReason::HigherPhase => "higher_phase", WinnerReason::HigherDeclaredPriority => "higher_declared_priority", WinnerReason::LaterDescriptorOrder => "later_descriptor_order" }})
		}
		CandidateDecision::Loser {
			reason,
			winner_candidate_id,
		} => {
			json!({"kind": "loser", "winner_candidate_id": winner_candidate_id, "reason": match reason { LoserReason::LowerPhase => "lower_phase", LoserReason::LowerDeclaredPriority => "lower_declared_priority", LoserReason::EarlierDescriptorOrder => "earlier_descriptor_order" }})
		}
	};

	Ok(
		json!({"candidate_id": item.candidate_id, "origin": origin, "phase": phase(item.phase), "declared_priority": item.declared_priority,
 "descriptor_order": item.descriptor_order, "source_member": item.source_member, "destination": item.destination.as_str(),
 "current_winner": effective_result(&value.current_winner),
 "proposed_winner": {"kind": "archive_candidate", "candidate_id": value.proposed_winner.candidate_id, "source_member": value.proposed_winner.source_member}, "decision": decision}),
	)
}

pub(crate) fn warning(value: &InstallWarning) -> Value {
	let (code, fields) = match value {
		InstallWarning::FomodCouldBeUsableSelected { group_id, option_id } => (
			"fomod_could_be_usable_selected",
			json!({"group_id": group_id, "option_id": option_id}),
		),
		InstallWarning::FomodConflictingFlagValues {
			flag_name,
			writers,
			winning_event,
			..
		} => (
			"fomod_conflicting_flag_values",
			json!({"flag_name": flag_name, "writers": writers.iter().map(writer).collect::<Vec<_>>(), "winning_event": writer(winning_event)}),
		),
		InstallWarning::FomodEmptyConditionList {
			operator,
			scope,
			group_id,
			option_id,
			pattern_order,
		} => {
			let mut fields = json!({"operator": match operator { ConditionOperator::And => "and", ConditionOperator::Or => "or" },
    "scope": match scope { ConditionScope::Module => "module", ConditionScope::StepVisibility => "step_visibility", ConditionScope::OptionTypePattern => "option_type_pattern", ConditionScope::ConditionalFilePattern => "conditional_file_pattern" }});
			if let Some(value) = group_id {
				fields["group_id"] = json!(value);
			}
			if let Some(value) = option_id {
				fields["option_id"] = json!(value);
			}
			if let Some(value) = pattern_order {
				fields["pattern_order"] = json!(value);
			}

			("fomod_empty_condition_list", fields)
		}
		InstallWarning::FomodMalformedGroupRepaired { group_id, .. } => (
			"fomod_malformed_group_repaired",
			json!({"group_id": group_id, "repair": "single_option_exactly_one_to_select_all"}),
		),
		InstallWarning::FomodFommDependencyAssumedCompatible { minimum_version } => (
			"fomod_fomm_dependency_assumed_compatible",
			json!({"minimum_version": minimum_version}),
		),
		InstallWarning::FomodEmptySourceIgnored {
			descriptor_order,
			group_id,
			option_id,
		} => {
			let mut fields = json!({"descriptor_order": descriptor_order});
			if let Some(value) = group_id {
				fields["group_id"] = json!(value);
			}
			if let Some(value) = option_id {
				fields["option_id"] = json!(value);
			}

			("fomod_empty_source_ignored", fields)
		}
		InstallWarning::FomodEmptyOptionAccepted { group_id, option_id } => (
			"fomod_empty_option_accepted",
			json!({"group_id": group_id, "option_id": option_id}),
		),
		InstallWarning::FomodModuleConfigPreferred {
			selected_config_member,
			ignored_config_member,
		} => (
			"fomod_module_config_preferred",
			json!({"selected_config_member": selected_config_member, "ignored_config_member": ignored_config_member}),
		),
		InstallWarning::FomodNonPluginFileDependencyPolicyUsed {
			data_relative_path,
			state,
		} => (
			"fomod_non_plugin_file_dependency_policy_used",
			json!({"data_relative_path": data_relative_path, "state": file_state(*state)}),
		),
		InstallWarning::FomodEqualPriorityTieResolved {
			destination,
			phase: item_phase,
			declared_priority,
			winner_candidate_id,
			loser_candidate_ids,
		} => (
			"fomod_equal_priority_tie_resolved",
			json!({"destination": destination.as_str(), "phase": phase(*item_phase), "declared_priority": declared_priority, "winner_candidate_id": winner_candidate_id, "loser_candidate_ids": loser_candidate_ids}),
		),
	};

	json!({"code": code, "fields": fields})
}

#[cfg(test)]
mod tests {
	use super::additional_selections;
	use super::installed;
	use super::preview;
	use crate::contract::Contracts;
	use application::conflicts::ListEffectiveConflictsOutput;
	use application::installation::AdditionalSelectionsRequired;
	use application::installation::CandidateDecision;
	use application::installation::InstallMode;
	use application::installation::InstallPlan;
	use application::installation::InstallPreview;
	use application::installation::InstalledArchive;
	use application::installation::PlanCandidateReference;
	use application::installation::PlannedCandidate;
	use application::installation::ProjectedModState;
	use application::installation::WinnerReason;
	use domain::ArchiveIdentity;
	use domain::DataRelativePath;
	use domain::EffectiveResult;
	use domain::InstallCandidate;
	use domain::InstallCandidateOrigin;
	use domain::InstallationPhase;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ResolutionStatus;
	use domain::Sha256Digest;
	use rootcause::Result;

	#[test]
	fn nonempty_install_plans_match_preview_and_installed_contracts() -> Result<()> {
		let name = ModName::new("Example".into())?;
		let plan = InstallPlan {
			archive_identity: ArchiveIdentity::DataArchive {
				archive_sha256: Sha256Digest::new("c".repeat(64))?,
				package_root: "Data".into(),
			},
			mod_name: name.clone(),
			replacement: false,
			accepted_choices: vec![],
			automatic_events: vec![],
			resolved_flags: vec![],
			warnings: vec![],
			candidates: vec![PlannedCandidate {
				origin_condition_evaluation: None,
				candidate: InstallCandidate {
					candidate_id: 1,
					origin: InstallCandidateOrigin::Required,
					phase: InstallationPhase::Required,
					declared_priority: 0,
					descriptor_order: 0,
					source_member: "Data/example.esp".into(),
					destination: DataRelativePath::new("example.esp".into())?,
				},
				current_winner: EffectiveResult::Absent {
					controlling_tombstone: None,
				},
				proposed_winner: PlanCandidateReference {
					candidate_id: 1,
					source_member: "Data/example.esp".into(),
				},
				decision: CandidateDecision::Winner {
					reason: WinnerReason::OnlyCandidate,
				},
			}],
			projected_state: ProjectedModState {
				mode: InstallMode::NewInstall,
				mod_name: name,
				priority: ModPriority::new(0),
				list_position: 0,
				enabled: false,
				overlaps: vec![],
			},
		};
		let conflicts = ListEffectiveConflictsOutput {
			resolution_status: ResolutionStatus::Exact,
			rows: vec![],
			problems: vec![],
		};

		let preview_value = preview(&InstallPreview {
			plan: plan.clone(),
			hypothetical_enabled_conflicts: conflicts.clone(),
		})?;
		let installed_value = installed(&InstalledArchive {
			plan,
			conflicts,
			warnings: vec![],
		})?;

		let contracts = Contracts::new()?;
		assert_eq!(
			[
				contracts.output_matches("mods_install", &preview_value),
				contracts.output_matches("mods_install", &installed_value),
			],
			[true, true],
			"nonempty preview and installed plans must satisfy the frozen contract"
		);
		Ok(())
	}

	#[test]
	fn installed_success_preserves_committed_new_and_replacement_facts() -> Result<()> {
		for (mode, enabled) in [
			(InstallMode::NewInstall, false),
			(InstallMode::Replacement, true),
			(InstallMode::Replacement, false),
		] {
			let name = ModName::new("Installed Mod".into())?;
			let output = InstalledArchive {
				plan: InstallPlan {
					archive_identity: ArchiveIdentity::DataArchive {
						archive_sha256: Sha256Digest::new("c".repeat(64))?,
						package_root: "Data".into(),
					},
					mod_name: name.clone(),
					replacement: mode == InstallMode::Replacement,
					accepted_choices: vec![],
					automatic_events: vec![],
					resolved_flags: vec![],
					warnings: vec![],
					candidates: vec![],
					projected_state: ProjectedModState {
						mode,
						mod_name: name,
						priority: ModPriority::new(7),
						list_position: 3,
						enabled,
						overlaps: vec![],
					},
				},
				conflicts: ListEffectiveConflictsOutput {
					resolution_status: ResolutionStatus::Exact,
					rows: vec![],
					problems: vec![],
				},
				warnings: vec![],
			};

			let value = installed(&output)?;

			assert!(Contracts::new()?.output_matches("mods_install", &value));
			assert_eq!(value["committed_state"]["kind"], "committed");
			assert_eq!(value["committed_state"]["enabled"], enabled);
			assert_eq!(value["committed_state"]["priority"], 7);
			assert_eq!(value["committed_state"]["list_position"], 3);
			assert!(value.get("hypothetical_enabled_conflicts").is_none());
		}
		Ok(())
	}

	#[test]
	fn incomplete_install_retains_identity_without_fabricated_recovery() -> Result<()> {
		let output = AdditionalSelectionsRequired {
			archive_identity: ArchiveIdentity::Fomod {
				archive_sha256: Sha256Digest::new("a".repeat(64))?,
				package_root: "package".into(),
				config_member: "fomod/ModuleConfig.xml".into(),
				config_sha256: Sha256Digest::new("b".repeat(64))?,
			},
			mod_name: ModName::new("Example".into())?,
			accepted_choices: vec![],
			automatic_events: vec![],
			resolved_flags: vec![],
			unresolved_groups: vec![],
			warnings: vec![],
		};

		let value = additional_selections(&output)?;
		assert!(Contracts::new()?.output_matches("mods_install", &value));
		assert_eq!(value["archive_identity"]["package_root"], "package");
		assert!(value.get("recovery").is_none());
		Ok(())
	}
}
