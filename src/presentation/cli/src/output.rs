use crate::install_warning;
use application::environment::InitializeEnvironmentOutput;
use application::environment::InitializeEnvironmentWarning;
use application::installation::AcceptedChoice;
use application::installation::AdditionalSelectionsRequired;
use application::installation::CandidateDecision;
use application::installation::EffectiveResult;
use application::installation::InstallMode;
use application::installation::InstallPlan;
use application::installation::InstallPreview;
use application::installation::InstallWarning;
use application::installation::LoserReason;
use application::installation::ProjectedModState;
use application::installation::TombstoneScope;
use application::installation::WinnerReason;
use application::settings::SetGameDirectoryOutput;
use application::settings::SetGameDirectoryWarning;
use application::settings::SettingRecord;
use application::settings::SettingSource;
use application::settings::SettingValue;
use domain::ArchiveIdentity;
use domain::FomodCardinality;
use domain::InstallCandidate;
use domain::InstallCandidateOrigin;
use domain::InstallationPhase;
use domain::OptionFileTrigger;
use domain::ParticipationReason;
use domain::ProviderReference;
use domain::ResolvedOptionType;
use serde_json::to_string;
use std::fmt::Write;

pub(crate) fn initialization(output: &InitializeEnvironmentOutput) -> (String, String) {
	let stderr = output
		.warnings
		.iter()
		.map(|warning| match warning {
			InitializeEnvironmentWarning::BethesdaRegistryFallbackUsed => {
				"warning: Bethesda registry fallback was used\n"
			}
		})
		.collect();
	(String::new(), stderr)
}

pub(crate) fn settings(records: &[SettingRecord]) -> String {
	records.iter().map(setting).collect::<Vec<_>>().join("\n")
}

pub(crate) fn setting(record: &SettingRecord) -> String {
	format!(
		"{} = {}\nsource = {}\nmanifest_value = {}\nmanifest_path = {}\nshadowed = {}\nwritable = {}\n",
		record.key.cli_name(),
		value(&record.value),
		source(&record.source),
		value(&record.manifest_value),
		quote(record.manifest_path),
		record.shadowed,
		record.writable,
	)
}

pub(crate) fn set_game_directory(output: &SetGameDirectoryOutput) -> (String, String) {
	let stderr = output
		.warnings
		.iter()
		.map(|warning| match warning {
			SetGameDirectoryWarning::EffectiveGameBindingInvalid { .. } => {
				"warning: MODS_GAME_DIR build does not match observed-build-id\n"
			}
		})
		.collect();
	(String::new(), stderr)
}

pub(crate) fn additional_selections(output: &AdditionalSelectionsRequired) -> (String, String) {
	let mut text = String::from("outcome = \"additional_selections_required\"\n");
	accepted_choices(&mut text, "accepted_choices", &output.accepted_choices);
	line_count(&mut text, "groups", output.unresolved_groups.len());
	for (group_index, group) in output.unresolved_groups.iter().enumerate() {
		let prefix = format!("groups[{group_index}]");
		line_string(&mut text, &format!("{prefix}.id"), &group.id);
		line_string(&mut text, &format!("{prefix}.label"), &group.label);
		line_string(&mut text, &format!("{prefix}.description"), &group.description);
		line_string(
			&mut text,
			&format!("{prefix}.cardinality"),
			match group.cardinality {
				FomodCardinality::SelectExactlyOne => "select_exactly_one",
				FomodCardinality::SelectAtMostOne => "select_at_most_one",
				FomodCardinality::SelectAtLeastOne => "select_at_least_one",
				FomodCardinality::SelectAny => "select_any",
				FomodCardinality::SelectAll => "select_all",
			},
		);
		line_count(&mut text, &format!("{prefix}.options"), group.options.len());
		for (option_index, option) in group.options.iter().enumerate() {
			let option_prefix = format!("{prefix}.options[{option_index}]");
			line_string(&mut text, &format!("{option_prefix}.id"), &option.id);
			line_string(&mut text, &format!("{option_prefix}.label"), &option.label);
			line_string(&mut text, &format!("{option_prefix}.description"), &option.description);
			line_string(
				&mut text,
				&format!("{option_prefix}.resolved_type"),
				match option.resolved_type {
					ResolvedOptionType::Required => "required",
					ResolvedOptionType::NotUsable => "not_usable",
					ResolvedOptionType::Recommended => "recommended",
					ResolvedOptionType::Optional => "optional",
					ResolvedOptionType::CouldBeUsable => "could_be_usable",
				},
			);
			line_bool(&mut text, &format!("{option_prefix}.selectable"), option.selectable);
			line_bool(&mut text, &format!("{option_prefix}.synthetic"), option.synthetic);
		}
	}
	warnings(&mut text, "warnings", &output.warnings);
	(text, install_warnings(&output.warnings))
}

pub(crate) fn install_preview(output: &InstallPreview) -> (String, String) {
	let mut text = String::from("outcome = \"preview\"\n");
	install_plan(&mut text, "plan", &output.plan);
	(text, install_warnings(&output.plan.warnings))
}

pub(crate) fn install_warnings(values: &[InstallWarning]) -> String {
	install_warning::messages(values)
}

fn install_plan(text: &mut String, prefix: &str, plan: &InstallPlan) {
	archive_identity(text, &format!("{prefix}.archive_identity"), &plan.archive_identity);
	line_string(text, &format!("{prefix}.mod_name"), plan.mod_name.as_str());
	line_bool(text, &format!("{prefix}.replacement"), plan.replacement);
	accepted_choices(text, &format!("{prefix}.accepted_choices"), &plan.accepted_choices);
	warnings(text, &format!("{prefix}.warnings"), &plan.warnings);
	line_count(text, &format!("{prefix}.candidates"), plan.candidates.len());
	for (index, candidate) in plan.candidates.iter().enumerate() {
		let candidate_prefix = format!("{prefix}.candidates[{index}]");
		install_candidate(text, &candidate_prefix, &candidate.candidate);
		effective_result(
			text,
			&format!("{candidate_prefix}.current_winner"),
			&candidate.current_winner,
		);
		line_u64(
			text,
			&format!("{candidate_prefix}.proposed_winner.candidate_id"),
			candidate.proposed_winner.candidate_id,
		);
		line_string(
			text,
			&format!("{candidate_prefix}.proposed_winner.source_member"),
			&candidate.proposed_winner.source_member,
		);
		match candidate.decision {
			CandidateDecision::Winner { reason } => {
				line_string(text, &format!("{candidate_prefix}.decision.kind"), "winner");
				line_string(
					text,
					&format!("{candidate_prefix}.decision.reason"),
					match reason {
						WinnerReason::OnlyCandidate => "only_candidate",
						WinnerReason::HigherPhase => "higher_phase",
						WinnerReason::HigherDeclaredPriority => "higher_declared_priority",
						WinnerReason::LaterDescriptorOrder => "later_descriptor_order",
					},
				);
			}
			CandidateDecision::Loser {
				reason,
				winner_candidate_id,
			} => {
				line_string(text, &format!("{candidate_prefix}.decision.kind"), "loser");
				line_string(
					text,
					&format!("{candidate_prefix}.decision.reason"),
					match reason {
						LoserReason::LowerPhase => "lower_phase",
						LoserReason::LowerDeclaredPriority => "lower_declared_priority",
						LoserReason::EarlierDescriptorOrder => "earlier_descriptor_order",
					},
				);
				line_u64(
					text,
					&format!("{candidate_prefix}.decision.winner_candidate_id"),
					winner_candidate_id,
				);
			}
		}
	}
	projected_state(text, &format!("{prefix}.projected_state"), &plan.projected_state);
}

fn archive_identity(text: &mut String, prefix: &str, identity: &ArchiveIdentity) {
	match identity {
		ArchiveIdentity::DataArchive {
			archive_sha256,
			package_root,
		} => {
			line_string(text, &format!("{prefix}.kind"), "data_archive");
			line_string(text, &format!("{prefix}.archive_sha256"), archive_sha256.as_str());
			line_string(text, &format!("{prefix}.package_root"), package_root);
		}
		ArchiveIdentity::Fomod {
			archive_sha256,
			package_root,
			config_member,
			config_sha256,
		} => {
			line_string(text, &format!("{prefix}.kind"), "fomod");
			line_string(text, &format!("{prefix}.archive_sha256"), archive_sha256.as_str());
			line_string(text, &format!("{prefix}.package_root"), package_root);
			line_string(text, &format!("{prefix}.config_member"), config_member);
			line_string(text, &format!("{prefix}.config_sha256"), config_sha256.as_str());
		}
	}
}

fn accepted_choices(text: &mut String, prefix: &str, choices: &[AcceptedChoice]) {
	line_count(text, prefix, choices.len());
	for (index, choice) in choices.iter().enumerate() {
		let choice_prefix = format!("{prefix}[{index}]");
		line_string(text, &format!("{choice_prefix}.group_id"), &choice.group_id);
		line_string(text, &format!("{choice_prefix}.option_id"), &choice.option_id);
	}
}

fn install_candidate(text: &mut String, prefix: &str, candidate: &InstallCandidate) {
	line_u64(text, &format!("{prefix}.candidate_id"), candidate.candidate_id);
	candidate_origin(text, &format!("{prefix}.origin"), &candidate.origin);
	line_string(
		text,
		&format!("{prefix}.phase"),
		match candidate.phase {
			InstallationPhase::Required => "required",
			InstallationPhase::SelectedOrForced => "selected_or_forced",
			InstallationPhase::Conditional => "conditional",
		},
	);
	let _ = writeln!(text, "{prefix}.declared_priority = {}", candidate.declared_priority);
	line_u64(text, &format!("{prefix}.descriptor_order"), candidate.descriptor_order);
	line_string(text, &format!("{prefix}.source_member"), &candidate.source_member);
	line_string(
		text,
		&format!("{prefix}.destination"),
		&candidate.destination.as_str().replace('/', "\\"),
	);
}

fn candidate_origin(text: &mut String, prefix: &str, origin: &InstallCandidateOrigin) {
	match origin {
		InstallCandidateOrigin::Required => line_string(text, &format!("{prefix}.kind"), "required"),
		InstallCandidateOrigin::Option {
			group_id,
			option_id,
			trigger,
		} => {
			line_string(text, &format!("{prefix}.kind"), "option");
			line_string(text, &format!("{prefix}.group_id"), group_id);
			line_string(text, &format!("{prefix}.option_id"), option_id);
			line_string(
				text,
				&format!("{prefix}.trigger"),
				match trigger {
					OptionFileTrigger::Selected => "selected",
					OptionFileTrigger::AlwaysInstall => "always_install",
					OptionFileTrigger::InstallIfUsable => "install_if_usable",
				},
			);
		}
		InstallCandidateOrigin::Conditional { pattern_order, .. } => {
			line_string(text, &format!("{prefix}.kind"), "conditional");
			line_u64(text, &format!("{prefix}.pattern_order"), *pattern_order);
		}
	}
}

fn projected_state(text: &mut String, prefix: &str, state: &ProjectedModState) {
	line_string(
		text,
		&format!("{prefix}.mode"),
		match state.mode {
			InstallMode::NewInstall => "new_install",
			InstallMode::Replacement => "replacement",
		},
	);
	line_string(text, &format!("{prefix}.mod_name"), state.mod_name.as_str());
	line_u32(text, &format!("{prefix}.priority"), state.priority.get());
	line_u64(text, &format!("{prefix}.list_position"), state.list_position);
	line_bool(text, &format!("{prefix}.enabled"), state.enabled);
	line_count(text, &format!("{prefix}.overlaps"), state.overlaps.len());
	for (index, overlap) in state.overlaps.iter().enumerate() {
		let overlap_prefix = format!("{prefix}.overlaps[{index}]");
		line_string(
			text,
			&format!("{overlap_prefix}.path"),
			&overlap.path.as_str().replace('/', "\\"),
		);
		provider(
			text,
			&format!("{overlap_prefix}.proposed_provider"),
			&overlap.proposed_provider,
		);
		line_count(
			text,
			&format!("{overlap_prefix}.overlapping_physical_files"),
			overlap.overlapping_physical_files.len(),
		);
		for (provider_index, value) in overlap.overlapping_physical_files.iter().enumerate() {
			provider(
				text,
				&format!("{overlap_prefix}.overlapping_physical_files[{provider_index}]"),
				value,
			);
		}
		effective_result(text, &format!("{overlap_prefix}.before"), &overlap.before);
		effective_result(
			text,
			&format!("{overlap_prefix}.after_operation"),
			&overlap.after_operation,
		);
		effective_result(
			text,
			&format!("{overlap_prefix}.hypothetical_enabled"),
			&overlap.hypothetical_enabled,
		);
	}
}

fn effective_result(text: &mut String, prefix: &str, value: &EffectiveResult) {
	match value {
		EffectiveResult::File(value) => {
			line_string(text, &format!("{prefix}.kind"), "file");
			provider(text, &format!("{prefix}.provider"), value);
		}
		EffectiveResult::Absent { controlling_tombstone } => {
			line_string(text, &format!("{prefix}.kind"), "absent");
			if let Some(tombstone) = controlling_tombstone {
				line_string(
					text,
					&format!("{prefix}.controlling_tombstone.scope"),
					match tombstone.scope {
						TombstoneScope::ExactFile => "exact_file",
						TombstoneScope::DirectorySubtree => "directory_subtree",
					},
				);
				provider(text, &format!("{prefix}.controlling_tombstone.owner"), &tombstone.owner);
			}
		}
	}
}

fn provider(text: &mut String, prefix: &str, value: &ProviderReference) {
	match value {
		ProviderReference::SteamData { original_path } => {
			line_string(text, &format!("{prefix}.kind"), "steam_data");
			line_string(
				text,
				&format!("{prefix}.original_path"),
				&original_path.as_str().replace('/', "\\"),
			);
		}
		ProviderReference::DataMod {
			mod_name,
			priority,
			original_path,
			participation_reason,
		} => {
			line_string(text, &format!("{prefix}.kind"), "data_mod");
			line_string(text, &format!("{prefix}.mod_name"), mod_name.as_str());
			line_u32(text, &format!("{prefix}.priority"), priority.get());
			line_string(
				text,
				&format!("{prefix}.original_path"),
				&original_path.as_str().replace('/', "\\"),
			);
			line_string(
				text,
				&format!("{prefix}.participation_reason"),
				match participation_reason {
					ParticipationReason::SteamBase => "steam_base",
					ParticipationReason::EnabledMod => "enabled_mod",
					ParticipationReason::DisabledMod => "disabled_mod",
					ParticipationReason::HypotheticalEnabledMod => "hypothetical_enabled_mod",
					ParticipationReason::ProjectedDisabledMod => "projected_disabled_mod",
					ParticipationReason::Overwrite => "overwrite",
				},
			);
		}
		ProviderReference::Overwrite { original_path } => {
			line_string(text, &format!("{prefix}.kind"), "overwrite");
			line_string(
				text,
				&format!("{prefix}.original_path"),
				&original_path.as_str().replace('/', "\\"),
			);
		}
	}
}

fn warnings(text: &mut String, prefix: &str, values: &[InstallWarning]) {
	install_warning::write_details(text, prefix, values);
}

fn line_string(text: &mut String, name: &str, value: &str) {
	let _ = writeln!(text, "{name} = {}", quote(value));
}
fn line_bool(text: &mut String, name: &str, value: bool) {
	let _ = writeln!(text, "{name} = {value}");
}
fn line_u32(text: &mut String, name: &str, value: u32) {
	let _ = writeln!(text, "{name} = {value}");
}
fn line_u64(text: &mut String, name: &str, value: u64) {
	let _ = writeln!(text, "{name} = {value}");
}
fn line_count(text: &mut String, name: &str, value: usize) {
	let _ = writeln!(text, "{name}.count = {value}");
}
fn value(value: &SettingValue) -> String {
	match value {
		SettingValue::Unset => "unset".to_owned(),
		SettingValue::String(value) => quote(value),
		SettingValue::Path(value) => quote(&value.display().to_string()),
		SettingValue::UnsignedInteger(value) => value.to_string(),
	}
}

fn source(source: &SettingSource) -> String {
	match source {
		SettingSource::Manifest => "manifest".to_owned(),
		SettingSource::Environment { variable } => {
			format!("environment:{variable}")
		}
		SettingSource::Invocation { argument } => {
			format!("invocation:{argument}")
		}
	}
}

pub(crate) fn quote(value: &str) -> String {
	to_string(value).unwrap_or_else(|_| "\"<unrepresentable>\"".to_owned())
}

#[cfg(test)]
mod tests {
	use super::additional_selections;
	use super::candidate_origin;
	use super::initialization;
	use super::quote;
	use super::set_game_directory;
	use super::setting;
	use application::environment::InitializeEnvironmentOutput;
	use application::environment::InitializeEnvironmentWarning;
	use application::installation::AcceptedChoice;
	use application::installation::AdditionalSelectionsRequired;
	use application::installation::ConditionEvaluation;
	use application::installation::OptionSelectionState;
	use application::installation::UnresolvedGroup;
	use application::installation::VisibleOption;
	use application::settings::EffectiveBinding;
	use application::settings::SetGameDirectoryOutput;
	use application::settings::SetGameDirectoryWarning;
	use application::settings::SettingKey;
	use application::settings::SettingRecord;
	use application::settings::SettingSource;
	use application::settings::SettingValue;
	use domain::ArchiveIdentity;
	use domain::FomodCardinality;
	use domain::FomodCondition;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::InstallCandidateOrigin;
	use domain::ModName;
	use domain::ResolvedOptionType;
	use domain::Sha256Digest;
	use domain::SteamBuildId;
	use std::env::current_dir;
	use std::error::Error;

	#[test]
	fn incomplete_choice_output_contains_only_decision_data() -> rootcause::Result<()> {
		let output = AdditionalSelectionsRequired {
			archive_identity: ArchiveIdentity::DataArchive {
				archive_sha256: Sha256Digest::new("a".repeat(64))?,
				package_root: String::new(),
			},
			mod_name: ModName::new("example".to_owned())?,
			automatic_events: Vec::new(),
			resolved_flags: Vec::new(),
			accepted_choices: vec![AcceptedChoice {
				sequence: 0,
				group_id: "previous".to_owned(),
				option_id: "accepted".to_owned(),
			}],
			unresolved_groups: vec![UnresolvedGroup {
				condition_evaluation: ConditionEvaluation {
					result: true,
					children: Vec::new(),
				},
				condition: FomodCondition::Constant(true),
				id: "visuals".to_owned(),
				label: "Visuals".to_owned(),
				description: "Choose a style".to_owned(),
				cardinality: FomodCardinality::SelectAtMostOne,
				options: vec![VisibleOption {
					condition_evaluation: ConditionEvaluation {
						result: true,
						children: Vec::new(),
					},
					condition: FomodCondition::Constant(true),
					selection_state: OptionSelectionState::Unselected,
					flag_effects: Vec::new(),
					file_effects: Vec::new(),
					id: "none".to_owned(),
					label: "None".to_owned(),
					description: "Select no option".to_owned(),
					resolved_type: ResolvedOptionType::Optional,
					selectable: true,
					synthetic: true,
				}],
			}],
			warnings: Vec::new(),
		};

		let (stdout, stderr) = additional_selections(&output);

		assert!(stderr.is_empty());
		for expected in [
			"accepted_choices[0].group_id = \"previous\"",
			"accepted_choices[0].option_id = \"accepted\"",
			"groups[0].id = \"visuals\"",
			"groups[0].label = \"Visuals\"",
			"groups[0].description = \"Choose a style\"",
			"groups[0].cardinality = \"select_at_most_one\"",
			"groups[0].options[0].resolved_type = \"optional\"",
			"groups[0].options[0].selectable = true",
			"groups[0].options[0].synthetic = true",
		] {
			assert!(stdout.contains(expected), "missing {expected}");
		}
		for removed in [
			"accepted_choices[0].sequence",
			"automatic_events",
			"resolved_flags",
			"conditions",
			"flag_effects",
			"file_effects",
			"rerun_requirements",
		] {
			assert!(!stdout.contains(removed), "found removed trace {removed}");
		}
		Ok(())
	}

	#[test]
	fn conditional_candidate_origin_includes_operational_identity() {
		let origin = InstallCandidateOrigin::Conditional {
			pattern_order: 4,
			condition: FomodCondition::Constant(true),
		};
		let mut output = String::new();

		candidate_origin(&mut output, "candidate.origin", &origin);

		assert!(output.contains("candidate.origin.kind = \"conditional\""));
		assert!(output.contains("candidate.origin.pattern_order = 4"));
	}

	#[test]
	fn mutation_output_is_quiet_except_for_actionable_warnings() -> Result<(), Box<dyn Error>> {
		let game_directory = GameInstallationPath::new(current_dir()?.join("game"))
			.map_err(|_| "test game path must be valid")?;
		let build_id = SteamBuildId::new(1).map_err(|_| "test build ID must be valid")?;
		let binding = GameBinding::new(game_directory.clone(), build_id);
		let mut initialize_output = InitializeEnvironmentOutput {
			game_binding: binding,
			profile_files: Vec::new(),
			warnings: Vec::new(),
		};
		let mut set_output = SetGameDirectoryOutput {
			stored_value: game_directory.clone(),
			stored_observed_build_id: build_id,
			effective_value: game_directory,
			source: SettingSource::Manifest,
			shadowed: false,
			effective_binding: EffectiveBinding::Valid,
			warnings: Vec::new(),
		};

		assert_eq!(initialization(&initialize_output), (String::new(), String::new()));
		assert_eq!(set_game_directory(&set_output), (String::new(), String::new()));

		initialize_output
			.warnings
			.push(InitializeEnvironmentWarning::BethesdaRegistryFallbackUsed);
		set_output
			.warnings
			.push(SetGameDirectoryWarning::EffectiveGameBindingInvalid {
				variable: "MODS_GAME_DIR",
				expected_build_id: 1,
				actual_build_id: 2,
			});
		assert_eq!(
			initialization(&initialize_output),
			(
				String::new(),
				"warning: Bethesda registry fallback was used\n".to_owned()
			)
		);
		assert_eq!(
			set_game_directory(&set_output),
			(
				String::new(),
				"warning: MODS_GAME_DIR build does not match observed-build-id\n".to_owned(),
			)
		);

		Ok(())
	}

	#[test]
	fn path_output_uses_toml_compatible_quoting() {
		let quoted = quote("C:\\Program Files (x86)\\Steam\nNext");
		assert_eq!(quoted, "\"C:\\\\Program Files (x86)\\\\Steam\\nNext\"");
	}

	#[test]
	fn setting_output_contains_all_record_fields() {
		let record = SettingRecord {
			key: SettingKey::GameDir,
			value: SettingValue::Path("C:\\Portable\\Fallout New Vegas".into()),
			source: SettingSource::Environment {
				variable: "MODS_GAME_DIR",
			},
			manifest_value: SettingValue::Path("C:\\Steam\\Fallout New Vegas".into()),
			manifest_path: "game_dir",
			shadowed: true,
			writable: true,
		};
		let output = setting(&record);
		assert!(output.contains("game-dir = \"C:\\\\Portable\\\\Fallout New Vegas\""));
		assert!(output.contains("source = environment:MODS_GAME_DIR"));
		assert!(output.contains("manifest_value = \"C:\\\\Steam\\\\Fallout New Vegas\""));
		assert!(output.contains("manifest_path = \"game_dir\""));
		assert!(output.contains("shadowed = true"));
		assert!(output.contains("writable = true"));
	}
}
