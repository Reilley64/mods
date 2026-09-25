use crate::errors::ErrorMarker;
use crate::installation::AcceptedChoice;
use crate::installation::AutomaticAction;
use crate::installation::AutomaticCause;
use crate::installation::AutomaticChoiceEvent;
use crate::installation::ChoiceSource;
use crate::installation::ConditionEvaluation;
use crate::installation::FileDependencyFact;
use crate::installation::FileDependencyKind;
use crate::installation::FlagWriter;
use crate::installation::FomodInstaller;
use crate::installation::FomodOption;
use crate::installation::InstallWarning;
use crate::installation::OptionSelectionState;
use crate::installation::ResolvedFlag;
use crate::installation::UnresolvedGroup;
use crate::installation::VisibleOption;
use domain::FileDependencyState;
use domain::FomodCardinality;
use domain::FomodChoice;
use domain::FomodCondition;
use domain::InstallCandidate;
use domain::InstallCandidateOrigin;
use domain::OptionFileTrigger;
use domain::ResolvedOptionType;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::report;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

// FOMOD conditions can encode state machines whose automatic selections traverse exponentially many states.
// This permits large ordinary installers while bounding every evaluator scan and fixed-point step.
const EVALUATION_WORK_LIMIT: usize = 1_000_000;

struct EvaluationBudget {
	remaining: usize,
}

impl EvaluationBudget {
	fn new() -> Self {
		Self {
			remaining: EVALUATION_WORK_LIMIT,
		}
	}

	fn spend(&mut self) -> Result<(), ErrorMarker> {
		let Some(remaining) = self.remaining.checked_sub(1) else {
			return Err(report!(
				ErrorMarker::unsupported_installer().with_phase("fomod_evaluation")
			));
		};
		self.remaining = remaining;
		Ok(())
	}
}

pub(super) struct DependencyFacts<'a> {
	pub file_dependencies: &'a HashMap<String, FileDependencyFact>,
	pub game_version: Option<&'a [u32]>,
	pub nvse_version: Option<&'a [u32]>,
}

#[derive(Debug)]
pub(super) struct FomodEvaluation {
	pub candidate_conditions: HashMap<u64, ConditionEvaluation>,
	pub choices: Vec<AcceptedChoice>,
	pub automatic_events: Vec<AutomaticChoiceEvent>,
	pub resolved_flags: Vec<ResolvedFlag>,
	pub warnings: Vec<InstallWarning>,
	pub unresolved_groups: Vec<UnresolvedGroup>,
	pub candidates: Vec<InstallCandidate>,
}

#[derive(Clone)]
struct SuppliedSelection {
	sequence: u64,
	group_id: String,
	option_id: String,
}

#[derive(Clone)]
struct SelectedOption {
	group_index: usize,
	option_index: usize,
	sequence: u64,
	automatic: bool,
}

pub(super) fn condition_tree_matches(
	installer: &FomodInstaller,
	predicate: impl Fn(&FomodCondition) -> bool + Copy,
	cancellation: &CancellationToken,
) -> Result<bool, ErrorMarker> {
	let mut budget = EvaluationBudget::new();

	if condition_matches_predicate(&installer.module_condition, predicate, &mut budget, cancellation)? {
		return Ok(true);
	}
	for group in &installer.groups {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		if condition_matches_predicate(&group.condition, predicate, &mut budget, cancellation)? {
			return Ok(true);
		}
		for option in &group.options {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if condition_matches_predicate(&option.condition, predicate, &mut budget, cancellation)? {
				return Ok(true);
			}
			for pattern in &option.type_patterns {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if condition_matches_predicate(
					&pattern.condition,
					predicate,
					&mut budget,
					cancellation,
				)? {
					return Ok(true);
				}
			}
		}
	}
	for pattern in &installer.conditional_candidates {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		if condition_matches_predicate(&pattern.condition, predicate, &mut budget, cancellation)? {
			return Ok(true);
		}
	}
	Ok(false)
}

pub(super) fn evaluate(
	installer: &FomodInstaller,
	choices: &[FomodChoice],
	facts: DependencyFacts<'_>,
	cancellation: &CancellationToken,
) -> Result<FomodEvaluation, ErrorMarker> {
	let mut budget = EvaluationBudget::new();

	if cancellation.is_cancelled() {
		return Err(report!(
			ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
		));
	}

	let mut unsupported_file_dependency = false;
	visit_installer_conditions(
		installer,
		&mut |condition| {
			if let FomodCondition::FileDependency {
				path,
				state: FileDependencyState::Inactive,
			} = condition
			{
				let plugin = facts
					.file_dependencies
					.get(&case_fold_key(path))
					.is_some_and(|fact| fact.kind == FileDependencyKind::Plugin)
					|| is_plugin_path(path);
				unsupported_file_dependency |= !plugin;
			}
			Ok(())
		},
		&mut budget,
		cancellation,
	)?;
	if unsupported_file_dependency {
		return Err(report!(
			ErrorMarker::unsupported_installer().with_phase("fomod_evaluation")
		));
	}

	let mut selected = Vec::new();
	let mut supplied_events = Vec::new();
	let mut automatic_events = Vec::new();
	let mut next_sequence = 0_u64;
	if !condition_matches(
		&installer.module_condition,
		&BTreeMap::new(),
		&facts,
		&mut budget,
		cancellation,
	)? {
		return Err(report!(
			ErrorMarker::dependency_unsatisfied().with_phase("fomod_evaluation")
		));
	}
	automatic_events.extend(stabilize_automatic(
		installer,
		&facts,
		&mut selected,
		&supplied_events,
		&mut next_sequence,
		&mut budget,
		cancellation,
	)?);

	for choice in choices {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let supplied_sequence = next_sequence;
		let flags = resolved_flag_map(installer, &selected, &mut budget, cancellation)?;
		let mut matched_group = None;
		for (index, group) in installer.groups.iter().enumerate() {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if group.id == choice.group_id {
				matched_group = Some((index, group));
				break;
			}
		}
		let Some((group_index, group)) = matched_group else {
			return Err(report!(ErrorMarker::invalid_selection(
				"choices",
				Some(choice.group_id.clone()),
				Some(choice.option_id.clone()),
				Some(supplied_sequence),
			)));
		};
		if !condition_matches(&group.condition, &flags, &facts, &mut budget, cancellation)? {
			return Err(report!(ErrorMarker::invalid_selection(
				"choices",
				Some(group.id.clone()),
				Some(choice.option_id.clone()),
				Some(supplied_sequence),
			)));
		}
		if choice.option_id == "none" {
			let mut group_has_selection = false;
			for item in &selected {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if item.group_index == group_index {
					group_has_selection = true;
					break;
				}
			}
			let mut group_acknowledged = false;
			for event in &supplied_events {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if event.group_id == group.id {
					group_acknowledged = true;
					break;
				}
			}
			if !matches!(
				group.cardinality,
				FomodCardinality::SelectAtMostOne | FomodCardinality::SelectAny
			) || group_has_selection || group_acknowledged
			{
				return Err(report!(ErrorMarker::invalid_selection(
					"choices",
					Some(group.id.clone()),
					Some(choice.option_id.clone()),
					Some(supplied_sequence),
				)));
			}
			supplied_events.push(SuppliedSelection {
				sequence: next_sequence,
				group_id: group.id.clone(),
				option_id: "none".to_owned(),
			});
			next_sequence = next_sequence.checked_add(1).ok_or_else(|| {
				report!(ErrorMarker::unsupported_installer().with_phase("fomod_evaluation"))
			})?;
		} else {
			let mut matched_option = None;
			for (index, option) in group.options.iter().enumerate() {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if option.id == choice.option_id {
					matched_option = Some((index, option));
					break;
				}
			}
			let Some((option_index, option)) = matched_option else {
				return Err(report!(ErrorMarker::invalid_selection(
					"choices",
					Some(group.id.clone()),
					Some(choice.option_id.clone()),
					Some(supplied_sequence),
				)));
			};
			let mut already_selected = false;
			for item in &selected {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if item.group_index == group_index && item.option_index == option_index {
					already_selected = true;
					break;
				}
			}
			let mut none_supplied = false;
			for event in &supplied_events {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if event.group_id == group.id && event.option_id == "none" {
					none_supplied = true;
					break;
				}
			}
			if !option_is_selectable(option, &flags, &facts, &mut budget, cancellation)?
				|| already_selected || none_supplied
			{
				return Err(report!(ErrorMarker::invalid_selection(
					"choices",
					Some(group.id.clone()),
					Some(option.id.clone()),
					Some(supplied_sequence),
				)));
			}
			selected.push(SelectedOption {
				group_index,
				option_index,
				sequence: next_sequence,
				automatic: false,
			});
			supplied_events.push(SuppliedSelection {
				sequence: next_sequence,
				group_id: group.id.clone(),
				option_id: option.id.clone(),
			});
			next_sequence = next_sequence.checked_add(1).ok_or_else(|| {
				report!(ErrorMarker::unsupported_installer().with_phase("fomod_evaluation"))
			})?;
			let flags = resolved_flag_map(installer, &selected, &mut budget, cancellation)?;
			let mut selected_count = 0;
			for item in &selected {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if item.group_index == group_index
					&& option_is_selectable(
						&group.options[item.option_index],
						&flags,
						&facts,
						&mut budget,
						cancellation,
					)? {
					selected_count += 1;
				}
			}
			if exceeds_maximum(group.cardinality, selected_count) {
				return Err(report!(ErrorMarker::invalid_selection(
					"choices",
					Some(group.id.clone()),
					Some(option.id.clone()),
					Some(supplied_sequence),
				)));
			}
		}
		automatic_events.extend(stabilize_automatic(
			installer,
			&facts,
			&mut selected,
			&supplied_events,
			&mut next_sequence,
			&mut budget,
			cancellation,
		)?);
	}

	validate_supplied_selections(
		installer,
		&facts,
		&selected,
		&supplied_events,
		&mut budget,
		cancellation,
	)?;

	let flags = resolved_flag_map(installer, &selected, &mut budget, cancellation)?;
	let mut unresolved_groups = Vec::new();
	for (group_index, group) in installer.groups.iter().enumerate() {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		if !condition_matches(&group.condition, &flags, &facts, &mut budget, cancellation)? {
			continue;
		}
		let mut selected_count = 0;
		for item in &selected {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if item.group_index == group_index
				&& option_is_selectable(
					&group.options[item.option_index],
					&flags,
					&facts,
					&mut budget,
					cancellation,
				)? {
				selected_count += 1;
			}
		}
		let mut acknowledged = false;
		for event in &supplied_events {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if event.group_id == group.id {
				acknowledged = true;
				break;
			}
		}
		let mut selectable_count = 0;
		for option in &group.options {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if option_is_selectable(option, &flags, &facts, &mut budget, cancellation)? {
				selectable_count += 1;
			}
		}
		let complete = match group.cardinality {
			FomodCardinality::SelectExactlyOne => selected_count == 1,
			FomodCardinality::SelectAtMostOne | FomodCardinality::SelectAny => {
				acknowledged || selected_count > 0
			}
			FomodCardinality::SelectAtLeastOne => selected_count >= 1,
			FomodCardinality::SelectAll => selected_count == selectable_count,
		};
		if complete {
			continue;
		}

		if selectable_count == 0
			&& matches!(
				group.cardinality,
				FomodCardinality::SelectExactlyOne | FomodCardinality::SelectAtLeastOne
			) {
			return Err(report!(
				ErrorMarker::unsupported_installer().with_phase("fomod_evaluation")
			));
		}

		let mut options = Vec::new();
		for option in &group.options {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if !condition_matches(&option.condition, &flags, &facts, &mut budget, cancellation)? {
				continue;
			}
			let resolved_type = resolve_option_type(option, &flags, &facts, &mut budget, cancellation)?;
			let selection_state = if let Some(selection) = selected.iter().find(|selection| {
				selection.group_index == group_index
					&& group.options[selection.option_index].id == option.id
			}) {
				if !selection.automatic {
					OptionSelectionState::Supplied
				} else if resolved_type == ResolvedOptionType::Required {
					OptionSelectionState::AutomaticRequired
				} else {
					OptionSelectionState::AutomaticSelectAll
				}
			} else {
				OptionSelectionState::Unselected
			};
			options.push(VisibleOption {
				condition: option.condition.clone(),
				condition_evaluation: condition_evaluation(
					&option.condition,
					&flags,
					&facts,
					&mut budget,
					cancellation,
				)?,
				flag_effects: option.flag_writes.clone(),
				file_effects: option.file_effects.clone(),
				selection_state,
				id: option.id.clone(),
				label: option.label.clone(),
				description: option.description.clone(),
				resolved_type,
				selectable: resolved_type != ResolvedOptionType::NotUsable,
				synthetic: false,
			});
		}
		if matches!(
			group.cardinality,
			FomodCardinality::SelectAtMostOne | FomodCardinality::SelectAny
		) {
			options.push(VisibleOption {
				condition_evaluation: ConditionEvaluation {
					result: true,
					children: Vec::new(),
				},
				condition: FomodCondition::Constant(true),
				flag_effects: Vec::new(),
				file_effects: Vec::new(),
				selection_state: OptionSelectionState::Unselected,
				id: "none".to_owned(),
				label: "None".to_owned(),
				description: "Select no option".to_owned(),
				resolved_type: ResolvedOptionType::Optional,
				selectable: true,
				synthetic: true,
			});
		}
		if options.is_empty() {
			return Err(report!(
				ErrorMarker::unsupported_installer().with_phase("fomod_evaluation")
			));
		}
		unresolved_groups.push(UnresolvedGroup {
			condition: group.condition.clone(),
			condition_evaluation: condition_evaluation(
				&group.condition,
				&flags,
				&facts,
				&mut budget,
				cancellation,
			)?,
			id: group.id.clone(),
			label: group.label.clone(),
			description: group.description.clone(),
			cardinality: group.cardinality,
			options,
		});
	}

	let mut ordered = Vec::with_capacity(selected.len());
	for item in &selected {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		ordered.push(item.clone());
	}
	ordered.sort_by_key(|item| item.sequence);
	let mut flag_writers = BTreeMap::<String, Vec<FlagWriter>>::new();
	for item in ordered {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let group = &installer.groups[item.group_index];
		let option = &group.options[item.option_index];
		for (order, write) in option.flag_writes.iter().enumerate() {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			flag_writers.entry(write.name.clone()).or_default().push(FlagWriter {
				sequence: item.sequence,
				flag_effect_order: order as u64,
				source: if item.automatic {
					ChoiceSource::Automatic
				} else {
					ChoiceSource::Supplied
				},
				group_id: group.id.clone(),
				option_id: option.id.clone(),
				value: write.value.clone(),
			});
		}
	}

	let mut warnings = Vec::new();
	let mut resolved_flags = Vec::new();
	for (name, writers) in flag_writers {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let Some(winning_event) = writers.last().cloned() else {
			continue;
		};

		resolved_flags.push(ResolvedFlag {
			name: name.clone(),
			value: winning_event.value.clone(),
			winning_event: winning_event.clone(),
		});

		let mut values = BTreeSet::new();
		for writer in &writers {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			values.insert(writer.value.clone());
		}

		if values.len() > 1 {
			warnings.push(InstallWarning::FomodConflictingFlagValues {
				flag_name: name,
				values: values.into_iter().collect(),
				resolved_value: winning_event.value.clone(),
				writers,
				winning_event,
			});
		}
	}

	for warning in &installer.warnings {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		warnings.push(warning.clone());
	}

	for item in &selected {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		if item.automatic {
			continue;
		}
		let group = &installer.groups[item.group_index];
		let option = &group.options[item.option_index];
		if resolve_option_type(option, &flags, &facts, &mut budget, cancellation)?
			== ResolvedOptionType::CouldBeUsable
		{
			warnings.push(InstallWarning::FomodCouldBeUsableSelected {
				group_id: group.id.clone(),
				option_id: option.id.clone(),
			});
		}
	}

	let mut ordinary_file_dependencies = BTreeSet::new();
	visit_installer_conditions(
		installer,
		&mut |condition| {
			if let FomodCondition::FileDependency { path, state } = condition {
				let ordinary = facts
					.file_dependencies
					.get(&case_fold_key(path))
					.is_some_and(|fact| fact.kind == FileDependencyKind::OrdinaryDataFile)
					|| !is_plugin_path(path);
				if ordinary
					&& matches!(state, FileDependencyState::Active | FileDependencyState::Missing)
				{
					ordinary_file_dependencies.insert((path.clone(), *state));
				}
			}
			Ok(())
		},
		&mut budget,
		cancellation,
	)?;
	for (path, state) in ordinary_file_dependencies {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		warnings.push(InstallWarning::FomodNonPluginFileDependencyPolicyUsed {
			data_relative_path: path,
			state,
		});
	}

	let mut fomm_versions = BTreeSet::new();
	visit_installer_conditions(
		installer,
		&mut |condition| {
			if let FomodCondition::FommDependency { minimum_version } = condition {
				fomm_versions.insert(minimum_version.clone());
			}
			Ok(())
		},
		&mut budget,
		cancellation,
	)?;
	for version in fomm_versions {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		warnings.push(InstallWarning::FomodFommDependencyAssumedCompatible {
			minimum_version: version,
		});
	}

	let mut candidate_conditions = HashMap::new();
	let mut candidates = Vec::with_capacity(installer.required_candidates.len());
	for candidate in &installer.required_candidates {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		candidates.push(candidate.clone());
	}
	for (group_index, group) in installer.groups.iter().enumerate() {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let group_visible = condition_matches(&group.condition, &flags, &facts, &mut budget, cancellation)?;
		for (option_index, option) in group.options.iter().enumerate() {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			let option_visible =
				condition_matches(&option.condition, &flags, &facts, &mut budget, cancellation)?;
			let mut selected_option = false;
			for item in &selected {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if item.group_index == group_index && item.option_index == option_index {
					selected_option = true;
					break;
				}
			}
			let chosen = group_visible && option_visible && selected_option;
			let usable = resolve_option_type(option, &flags, &facts, &mut budget, cancellation)?
				!= ResolvedOptionType::NotUsable;
			for candidate in &option.file_candidates {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				let include = match &candidate.origin {
					InstallCandidateOrigin::Option {
						trigger: OptionFileTrigger::Selected,
						..
					} => chosen,
					InstallCandidateOrigin::Option {
						trigger: OptionFileTrigger::AlwaysInstall,
						..
					} => true,
					InstallCandidateOrigin::Option {
						trigger: OptionFileTrigger::InstallIfUsable,
						..
					} => usable,
					_ => chosen,
				};
				if include {
					candidates.push(candidate.clone());
				}
			}
		}
	}
	for pattern in &installer.conditional_candidates {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		if !condition_matches(&pattern.condition, &flags, &facts, &mut budget, cancellation)? {
			continue;
		}

		let evaluated = condition_evaluation(&pattern.condition, &flags, &facts, &mut budget, cancellation)?;
		for candidate in &pattern.candidates {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			candidate_conditions.insert(candidate.candidate_id, evaluated.clone());
			candidates.push(candidate.clone());
		}
	}
	if cancellation.is_cancelled() {
		return Err(report!(
			ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
		));
	}
	let mut choices = Vec::with_capacity(supplied_events.len());
	for choice in supplied_events {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		choices.push(AcceptedChoice {
			sequence: choice.sequence,
			group_id: choice.group_id,
			option_id: choice.option_id,
		});
	}
	Ok(FomodEvaluation {
		candidate_conditions,
		choices,
		automatic_events,
		resolved_flags,
		warnings,
		unresolved_groups,
		candidates,
	})
}

fn stabilize_automatic(
	installer: &FomodInstaller,
	dependency_facts: &DependencyFacts<'_>,
	selected_options: &mut Vec<SelectedOption>,
	supplied_events: &[SuppliedSelection],
	next_sequence: &mut u64,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<Vec<AutomaticChoiceEvent>, ErrorMarker> {
	let mut automatic_events = Vec::new();
	let mut seen_automatic_states = BTreeSet::new();
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let flags = resolved_flag_map(installer, selected_options, budget, cancellation)?;
		let mut desired_automatic = BTreeSet::new();
		for (group_index, group) in installer.groups.iter().enumerate() {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if !condition_matches(&group.condition, &flags, dependency_facts, budget, cancellation)? {
				continue;
			}
			for (option_index, option) in group.options.iter().enumerate() {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if !condition_matches(
					&option.condition,
					&flags,
					dependency_facts,
					budget,
					cancellation,
				)? {
					continue;
				}
				let option_type =
					resolve_option_type(option, &flags, dependency_facts, budget, cancellation)?;
				if option_type == ResolvedOptionType::Required
					|| group.cardinality == FomodCardinality::SelectAll
						&& option_type != ResolvedOptionType::NotUsable
				{
					desired_automatic.insert((group_index, option_index));
				}
			}
		}

		let mut previous_automatic = BTreeSet::new();
		for item in selected_options.iter() {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if item.automatic {
				previous_automatic.insert((item.group_index, item.option_index));
			}
		}
		for (group_index, option_index) in previous_automatic.difference(&desired_automatic) {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			automatic_events.push(AutomaticChoiceEvent {
				sequence: *next_sequence,
				action: AutomaticAction::Withdrawn,
				group_id: installer.groups[*group_index].id.clone(),
				option_id: installer.groups[*group_index].options[*option_index].id.clone(),
				active_causes: Vec::new(),
			});
			*next_sequence = next_sequence.checked_add(1).ok_or_else(|| {
				report!(ErrorMarker::unsupported_installer().with_phase("fomod_evaluation"))
			})?;
		}

		let mut retained = Vec::with_capacity(selected_options.len());
		for item in selected_options.drain(..) {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if !item.automatic || desired_automatic.contains(&(item.group_index, item.option_index)) {
				retained.push(item);
			}
		}
		*selected_options = retained;
		for (group_index, option_index) in &desired_automatic {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			let mut automatic_present = false;
			let mut supplied_sequence = None;
			for item in selected_options.iter() {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if item.group_index == *group_index && item.option_index == *option_index {
					if item.automatic {
						automatic_present = true;
					} else {
						supplied_sequence = Some(item.sequence);
					}
					break;
				}
			}
			if automatic_present {
				continue;
			}
			if let Some(sequence) = supplied_sequence {
				return Err(report!(ErrorMarker::invalid_selection(
					"choices",
					Some(installer.groups[*group_index].id.clone()),
					Some(installer.groups[*group_index].options[*option_index].id.clone()),
					Some(sequence),
				)));
			}

			let group = &installer.groups[*group_index];
			let option = &group.options[*option_index];
			let mut active_causes = Vec::new();
			if resolve_option_type(option, &flags, dependency_facts, budget, cancellation)?
				== ResolvedOptionType::Required
			{
				active_causes.push(AutomaticCause::Required);
			}
			if group.cardinality == FomodCardinality::SelectAll {
				active_causes.push(AutomaticCause::SelectAll);
			}

			automatic_events.push(AutomaticChoiceEvent {
				sequence: *next_sequence,
				action: AutomaticAction::Selected,
				group_id: group.id.clone(),
				option_id: option.id.clone(),
				active_causes,
			});

			selected_options.push(SelectedOption {
				group_index: *group_index,
				option_index: *option_index,
				sequence: *next_sequence,
				automatic: true,
			});
			*next_sequence = next_sequence.checked_add(1).ok_or_else(|| {
				report!(ErrorMarker::unsupported_installer().with_phase("fomod_evaluation"))
			})?;
		}

		if previous_automatic == desired_automatic {
			validate_supplied_selections(
				installer,
				dependency_facts,
				selected_options,
				supplied_events,
				budget,
				cancellation,
			)?;

			let flags = resolved_flag_map(installer, selected_options, budget, cancellation)?;
			for (group_index, group) in installer.groups.iter().enumerate() {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				let mut selected_count = 0;
				for item in selected_options.iter() {
					if cancellation.is_cancelled() {
						return Err(report!(ErrorMarker::operation_cancelled()
							.with_phase("fomod_evaluation")));
					}
					budget.spend()?;
					if item.group_index == group_index
						&& option_is_selectable(
							&group.options[item.option_index],
							&flags,
							dependency_facts,
							budget,
							cancellation,
						)? {
						selected_count += 1;
					}
				}
				if exceeds_maximum(group.cardinality, selected_count) {
					return Err(report!(
						ErrorMarker::unsupported_installer().with_phase("fomod_evaluation")
					));
				}
			}

			return Ok(automatic_events);
		}
		if !seen_automatic_states.insert(desired_automatic) {
			return Err(report!(
				ErrorMarker::unsupported_installer().with_phase("fomod_evaluation")
			));
		}
	}
}

fn validate_supplied_selections(
	installer: &FomodInstaller,
	dependency_facts: &DependencyFacts<'_>,
	selected_options: &[SelectedOption],
	supplied_events: &[SuppliedSelection],
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let flags = resolved_flag_map(installer, selected_options, budget, cancellation)?;
	for supplied_event in supplied_events {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let mut matched_group = None;
		for (group_index, group) in installer.groups.iter().enumerate() {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if group.id == supplied_event.group_id {
				matched_group = Some((group_index, group));
				break;
			}
		}
		let Some((group_index, group)) = matched_group else {
			return Err(report!(ErrorMarker::invalid_selection(
				"choices",
				Some(supplied_event.group_id.clone()),
				Some(supplied_event.option_id.clone()),
				Some(supplied_event.sequence),
			)));
		};
		if !condition_matches(&group.condition, &flags, dependency_facts, budget, cancellation)? {
			return Err(report!(ErrorMarker::invalid_selection(
				"choices",
				Some(supplied_event.group_id.clone()),
				Some(supplied_event.option_id.clone()),
				Some(supplied_event.sequence),
			)));
		}

		if supplied_event.option_id == "none" {
			for item in selected_options {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if item.group_index == group_index {
					return Err(report!(ErrorMarker::invalid_selection(
						"choices",
						Some(supplied_event.group_id.clone()),
						Some(supplied_event.option_id.clone()),
						Some(supplied_event.sequence),
					)));
				}
			}
			continue;
		}

		let mut matched_option = None;
		for option in &group.options {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if option.id == supplied_event.option_id {
				matched_option = Some(option);
				break;
			}
		}
		let Some(option) = matched_option else {
			return Err(report!(ErrorMarker::invalid_selection(
				"choices",
				Some(supplied_event.group_id.clone()),
				Some(supplied_event.option_id.clone()),
				Some(supplied_event.sequence),
			)));
		};
		if !option_is_selectable(option, &flags, dependency_facts, budget, cancellation)? {
			return Err(report!(ErrorMarker::invalid_selection(
				"choices",
				Some(supplied_event.group_id.clone()),
				Some(supplied_event.option_id.clone()),
				Some(supplied_event.sequence),
			)));
		}
	}

	Ok(())
}

fn exceeds_maximum(cardinality: FomodCardinality, count: usize) -> bool {
	matches!(
		cardinality,
		FomodCardinality::SelectExactlyOne | FomodCardinality::SelectAtMostOne
	) && count > 1
}

fn option_is_selectable(
	option: &FomodOption,
	flags: &BTreeMap<String, String>,
	facts: &DependencyFacts<'_>,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<bool, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(
			ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
		));
	}
	budget.spend()?;

	Ok(
		condition_matches(&option.condition, flags, facts, budget, cancellation)?
			&& resolve_option_type(option, flags, facts, budget, cancellation)?
				!= ResolvedOptionType::NotUsable,
	)
}

fn resolve_option_type(
	option: &FomodOption,
	flags: &BTreeMap<String, String>,
	facts: &DependencyFacts<'_>,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<ResolvedOptionType, ErrorMarker> {
	for pattern in &option.type_patterns {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		if condition_matches(&pattern.condition, flags, facts, budget, cancellation)? {
			return Ok(pattern.option_type);
		}
	}

	Ok(option.default_type)
}

fn condition_evaluation(
	condition: &FomodCondition,
	flags: &BTreeMap<String, String>,
	facts: &DependencyFacts<'_>,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<ConditionEvaluation, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(
			ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
		));
	}
	budget.spend()?;

	let mut children = Vec::new();
	let result = match condition {
		FomodCondition::All(conditions) | FomodCondition::Any(conditions) => {
			for child in conditions {
				children.push(condition_evaluation(child, flags, facts, budget, cancellation)?);
			}
			if matches!(condition, FomodCondition::All(_)) {
				children.iter().all(|child| child.result)
			} else {
				children.iter().any(|child| child.result)
			}
		}
		_ => condition_matches(condition, flags, facts, budget, cancellation)?,
	};

	Ok(ConditionEvaluation { result, children })
}

fn condition_matches(
	condition: &FomodCondition,
	flags: &BTreeMap<String, String>,
	facts: &DependencyFacts<'_>,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<bool, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(
			ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
		));
	}
	budget.spend()?;

	match condition {
		FomodCondition::Constant(value) => Ok(*value),
		FomodCondition::All(children) => {
			for child in children {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if !condition_matches(child, flags, facts, budget, cancellation)? {
					return Ok(false);
				}
			}
			Ok(true)
		}
		FomodCondition::Any(children) => {
			for child in children {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				if condition_matches(child, flags, facts, budget, cancellation)? {
					return Ok(true);
				}
			}
			Ok(false)
		}
		FomodCondition::FileDependency { path, state } => Ok(facts
			.file_dependencies
			.get(&case_fold_key(path))
			.map_or(FileDependencyState::Missing, |fact| fact.state)
			== *state),
		FomodCondition::FlagDependency { name, value } => {
			Ok(flags.get(name).map_or(value.is_empty(), |actual| actual == value))
		}
		FomodCondition::GameDependency { minimum_version } => {
			let Some(actual) = facts.game_version else {
				return Ok(false);
			};
			let required = parse_version(minimum_version, budget, cancellation)?;
			Ok(!version_less_than(actual, &required, budget, cancellation)?)
		}
		FomodCondition::NvseDependency { minimum_version } => {
			let Some(actual) = facts.nvse_version else {
				return Ok(false);
			};
			let required = parse_version(minimum_version, budget, cancellation)?;
			Ok(!version_less_than(actual, &required, budget, cancellation)?)
		}
		FomodCondition::FommDependency { .. } => Ok(true),
	}
}

fn resolved_flag_map(
	installer: &FomodInstaller,
	selected: &[SelectedOption],
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<BTreeMap<String, String>, ErrorMarker> {
	let mut ordered = Vec::with_capacity(selected.len());
	for item in selected {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		ordered.push(item.clone());
	}
	ordered.sort_by_key(|item| item.sequence);
	let mut flags = BTreeMap::new();
	for item in ordered {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		for write in &installer.groups[item.group_index].options[item.option_index].flag_writes {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			flags.insert(write.name.clone(), write.value.clone());
		}
	}
	Ok(flags)
}

fn visit_installer_conditions(
	installer: &FomodInstaller,
	visitor: &mut impl FnMut(&FomodCondition) -> Result<(), ErrorMarker>,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	visit_condition(&installer.module_condition, visitor, budget, cancellation)?;
	for group in &installer.groups {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		visit_condition(&group.condition, visitor, budget, cancellation)?;
		for option in &group.options {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			visit_condition(&option.condition, visitor, budget, cancellation)?;
			for pattern in &option.type_patterns {
				if cancellation.is_cancelled() {
					return Err(report!(
						ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
					));
				}
				budget.spend()?;
				visit_condition(&pattern.condition, visitor, budget, cancellation)?;
			}
		}
	}
	for pattern in &installer.conditional_candidates {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		visit_condition(&pattern.condition, visitor, budget, cancellation)?;
	}
	Ok(())
}

fn visit_condition(
	condition: &FomodCondition,
	visitor: &mut impl FnMut(&FomodCondition) -> Result<(), ErrorMarker>,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(
			ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
		));
	}
	budget.spend()?;
	visitor(condition)?;
	if let FomodCondition::All(children) | FomodCondition::Any(children) = condition {
		for child in children {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			visit_condition(child, visitor, budget, cancellation)?;
		}
	}
	Ok(())
}

fn condition_matches_predicate(
	condition: &FomodCondition,
	predicate: impl Fn(&FomodCondition) -> bool + Copy,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<bool, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(
			ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
		));
	}
	budget.spend()?;
	if predicate(condition) {
		return Ok(true);
	}
	if let FomodCondition::All(children) | FomodCondition::Any(children) = condition {
		for child in children {
			if cancellation.is_cancelled() {
				return Err(report!(
					ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
				));
			}
			budget.spend()?;
			if condition_matches_predicate(child, predicate, budget, cancellation)? {
				return Ok(true);
			}
		}
	}
	Ok(false)
}

fn is_plugin_path(path: &str) -> bool {
	if path.split(['/', '\\']).count() != 1 {
		return false;
	}

	let folded = case_fold_key(path);
	[".esp", ".esm", ".esl"]
		.iter()
		.any(|extension| folded.ends_with(extension))
}

// FOMOD versions are arbitrary-length numeric dot components, not SemVer. Evaluation also needs
// cooperative cancellation and budget accounting, so a SemVer abstraction is unsuitable here.
fn parse_version(
	value: &str,
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<Vec<u32>, ErrorMarker> {
	let mut parts = Vec::new();
	for part in value.split('.') {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let part = part.parse::<u32>().map_err(|error| {
			report!(error).context(ErrorMarker::unsupported_installer().with_phase("fomod_evaluation"))
		})?;
		parts.push(part);
	}
	Ok(parts)
}

fn version_less_than(
	actual: &[u32],
	required: &[u32],
	budget: &mut EvaluationBudget,
	cancellation: &CancellationToken,
) -> Result<bool, ErrorMarker> {
	let len = actual.len().max(required.len());
	for index in 0..len {
		if cancellation.is_cancelled() {
			return Err(report!(
				ErrorMarker::operation_cancelled().with_phase("fomod_evaluation")
			));
		}
		budget.spend()?;
		let actual_part = actual.get(index).copied().unwrap_or(0);
		let required_part = required.get(index).copied().unwrap_or(0);
		if actual_part != required_part {
			return Ok(actual_part < required_part);
		}
	}
	Ok(false)
}

#[cfg(test)]
mod tests {
	use super::DependencyFacts;
	use super::EvaluationBudget;
	use super::evaluate;
	use super::parse_version;
	use super::version_less_than;
	use crate::ErrorCode;
	use crate::ErrorMarker;
	use crate::installation::FileDependencyFact;
	use crate::installation::FileDependencyKind;
	use crate::installation::FomodFlagWrite;
	use crate::installation::FomodGroup;
	use crate::installation::FomodInstaller;
	use crate::installation::FomodOption;
	use crate::installation::FomodOptionTypePattern;
	use domain::FileDependencyState;
	use domain::FomodCardinality;
	use domain::FomodCondition;
	use domain::ResolvedOptionType;
	use domain::case_fold_key;
	use std::collections::HashMap;
	use std::error::Error;
	use std::num::ParseIntError;
	use std::result::Result as StdResult;
	use tokio_util::sync::CancellationToken;

	fn flag_condition(index: usize, value: &str) -> FomodCondition {
		FomodCondition::FlagDependency {
			name: format!("bit-{index}"),
			value: value.to_owned(),
		}
	}

	fn installer(groups: Vec<FomodGroup>) -> FomodInstaller {
		FomodInstaller {
			schema_version: "test".to_owned(),
			module_condition: FomodCondition::Constant(true),
			groups,
			required_candidates: Vec::new(),
			conditional_candidates: Vec::new(),
			warnings: Vec::new(),
		}
	}

	fn automatic_group(index: usize, required_when: FomodCondition) -> FomodGroup {
		FomodGroup {
			id: format!("group-{index}"),
			label: format!("Group {index}"),
			description: String::new(),
			cardinality: FomodCardinality::SelectAtMostOne,
			condition: FomodCondition::Constant(true),
			options: vec![FomodOption {
				id: format!("option-{index}"),
				label: format!("Option {index}"),
				description: String::new(),
				condition: FomodCondition::Constant(true),
				default_type: ResolvedOptionType::Optional,
				type_patterns: vec![FomodOptionTypePattern {
					condition: required_when,
					option_type: ResolvedOptionType::Required,
				}],
				flag_writes: vec![FomodFlagWrite {
					name: format!("bit-{index}"),
					value: "set".to_owned(),
				}],
				file_candidates: Vec::new(),
				file_effects: Vec::new(),
			}],
		}
	}

	#[test]
	fn malformed_or_overflowing_versions_are_unsupported() -> StdResult<(), Box<dyn Error>> {
		for version in ["1.invalid.3", "4294967296"] {
			let result = parse_version(version, &mut EvaluationBudget::new(), &CancellationToken::new());
			let Err(report) = result else {
				return Err(format!("{version} must be rejected").into());
			};

			assert!(report.iter_reports().any(|report| report
				.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::UnsupportedInstaller)));
			assert!(report
				.iter_reports()
				.any(|report| report.downcast_current_context::<ParseIntError>().is_some()));
		}
		Ok(())
	}

	#[test]
	fn versions_use_numeric_components_and_ignore_trailing_zeroes() -> StdResult<(), Box<dyn Error>> {
		let cancellation = CancellationToken::new();
		let mut budget = EvaluationBudget::new();

		let two_is_less_than_ten = version_less_than(&[1, 2], &[1, 10], &mut budget, &cancellation)
			.map_err(|_| "numeric component comparison must succeed")?;
		let ten_is_less_than_two = version_less_than(&[1, 10], &[1, 2], &mut budget, &cancellation)
			.map_err(|_| "numeric component comparison must succeed")?;
		let shorter_is_less = version_less_than(&[1, 2], &[1, 2, 0], &mut budget, &cancellation)
			.map_err(|_| "trailing-zero comparison must succeed")?;
		let longer_is_less = version_less_than(&[1, 2, 0], &[1, 2], &mut budget, &cancellation)
			.map_err(|_| "trailing-zero comparison must succeed")?;

		assert!(two_is_less_than_ten);
		assert!(!ten_is_less_than_two);
		assert!(!shorter_is_less);
		assert!(!longer_is_less);
		Ok(())
	}

	#[test]
	fn file_dependencies_use_simple_unicode_case_folded_keys() -> StdResult<(), Box<dyn Error>> {
		let mut installer = installer(Vec::new());
		installer.module_condition = FomodCondition::FileDependency {
			path: "textures/éς.bin".to_owned(),
			state: FileDependencyState::Inactive,
		};
		let file_dependencies = HashMap::from([(
			case_fold_key("Textures/ÉΣ.BIN"),
			FileDependencyFact {
				kind: FileDependencyKind::Plugin,
				state: FileDependencyState::Inactive,
			},
		)]);

		let evaluation = evaluate(
			&installer,
			&[],
			DependencyFacts {
				file_dependencies: &file_dependencies,
				game_version: None,
				nvse_version: None,
			},
			&CancellationToken::new(),
		)
		.map_err(|_| "case-folded file dependency must match")?;

		assert!(evaluation.candidates.is_empty());
		Ok(())
	}

	#[test]
	fn nested_plugin_extension_inactive_dependency_is_unsupported() -> StdResult<(), Box<dyn Error>> {
		let mut installer = installer(Vec::new());
		installer.module_condition = FomodCondition::FileDependency {
			path: "subdir\\Nested.esp".to_owned(),
			state: FileDependencyState::Inactive,
		};
		let file_dependencies = HashMap::from([(
			case_fold_key("subdir\\Nested.esp"),
			FileDependencyFact {
				kind: FileDependencyKind::OrdinaryDataFile,
				state: FileDependencyState::Active,
			},
		)]);

		let result = evaluate(
			&installer,
			&[],
			DependencyFacts {
				file_dependencies: &file_dependencies,
				game_version: None,
				nvse_version: None,
			},
			&CancellationToken::new(),
		);
		let Err(report) = result else {
			return Err("a nested Data file cannot request the Inactive plugin state".into());
		};

		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::UnsupportedInstaller)));
		Ok(())
	}

	#[test]
	fn exactly_one_static_not_usable_option_has_no_valid_choice() -> StdResult<(), Box<dyn Error>> {
		let installer = installer(vec![FomodGroup {
			id: "choice".to_owned(),
			label: "Choice".to_owned(),
			description: String::new(),
			cardinality: FomodCardinality::SelectExactlyOne,
			condition: FomodCondition::Constant(true),
			options: vec![FomodOption {
				id: "only".to_owned(),
				label: "Only".to_owned(),
				description: String::new(),
				condition: FomodCondition::Constant(true),
				default_type: ResolvedOptionType::NotUsable,
				type_patterns: Vec::new(),
				flag_writes: Vec::new(),
				file_candidates: Vec::new(),
				file_effects: Vec::new(),
			}],
		}]);
		let file_dependencies = HashMap::new();

		let result = evaluate(
			&installer,
			&[],
			DependencyFacts {
				file_dependencies: &file_dependencies,
				game_version: None,
				nvse_version: None,
			},
			&CancellationToken::new(),
		);
		let Err(report) = result else {
			return Err("exactly one group without a selectable option must be unsupported".into());
		};

		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::UnsupportedInstaller)));
		Ok(())
	}

	#[test]
	fn root_plugin_inactive_dependency_remains_supported_with_simple_case_fold() -> StdResult<(), Box<dyn Error>> {
		let mut installer = installer(Vec::new());
		installer.module_condition = FomodCondition::FileDependency {
			path: "Root.eſp".to_owned(),
			state: FileDependencyState::Inactive,
		};
		let file_dependencies = HashMap::new();

		let result = evaluate(
			&installer,
			&[],
			DependencyFacts {
				file_dependencies: &file_dependencies,
				game_version: None,
				nvse_version: None,
			},
			&CancellationToken::new(),
		);
		let Err(report) = result else {
			return Err("a missing root plugin must leave the Inactive dependency unsatisfied".into());
		};

		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::DependencyUnsatisfied)));
		Ok(())
	}

	#[test]
	fn binary_counter_automatic_graph_exhausts_evaluation_budget() -> StdResult<(), Box<dyn Error>> {
		let mut groups = Vec::new();
		for index in 0..32 {
			let next_bit_is_set = if index == 0 {
				flag_condition(index, "")
			} else {
				let carry_is_set = FomodCondition::All(
					(0..index).map(|lower| flag_condition(lower, "set")).collect(),
				);
				let carry_is_clear = FomodCondition::Any(
					(0..index).map(|lower| flag_condition(lower, "")).collect(),
				);
				FomodCondition::Any(vec![
					FomodCondition::All(vec![flag_condition(index, "set"), carry_is_clear]),
					FomodCondition::All(vec![flag_condition(index, ""), carry_is_set]),
				])
			};
			groups.push(automatic_group(index, next_bit_is_set));
		}
		let installer = installer(groups);
		let file_dependencies = HashMap::new();

		let result = evaluate(
			&installer,
			&[],
			DependencyFacts {
				file_dependencies: &file_dependencies,
				game_version: None,
				nvse_version: None,
			},
			&CancellationToken::new(),
		);
		let Err(report) = result else {
			return Err(
				"the exponential automatic-selection graph must exhaust the evaluator budget".into(),
			);
		};

		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::UnsupportedInstaller)));
		Ok(())
	}

	#[test]
	fn long_automatic_dependency_chain_stays_within_evaluation_budget() -> StdResult<(), Box<dyn Error>> {
		let mut groups = Vec::new();
		groups.push(automatic_group(0, FomodCondition::Constant(true)));
		for index in 1..64 {
			groups.push(automatic_group(index, flag_condition(index - 1, "set")));
		}
		let installer = installer(groups);
		let file_dependencies = HashMap::new();

		let evaluation = evaluate(
			&installer,
			&[],
			DependencyFacts {
				file_dependencies: &file_dependencies,
				game_version: None,
				nvse_version: None,
			},
			&CancellationToken::new(),
		)
		.map_err(|_| "a long finite automatic-selection chain must remain supported")?;

		assert!(evaluation.choices.is_empty());
		assert!(evaluation.unresolved_groups.is_empty());
		assert!(evaluation.candidates.is_empty());
		Ok(())
	}
}
