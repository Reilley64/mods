use crate::output::quote;
use application::installation::ConditionOperator;
use application::installation::ConditionScope;
use application::installation::InstallWarning;
use application::installation::MalformedGroupRepair;
use domain::FileDependencyState;
use domain::InstallationPhase;
use std::borrow::Cow;
use std::fmt::Write;

pub(crate) fn write_details(text: &mut String, prefix: &str, warnings: &[InstallWarning]) {
	let _ = writeln!(text, "{prefix}.count = {}", warnings.len());
	for (index, warning) in warnings.iter().enumerate() {
		let prefix = format!("{prefix}[{index}]");
		let output = WarningOutput::from(warning);
		write_string(text, &format!("{prefix}.kind"), output.name);
		for field in output.fields {
			field.write(text, &prefix);
		}
	}
}

pub(crate) fn messages(warnings: &[InstallWarning]) -> String {
	let mut text = String::new();
	for warning in warnings {
		let output = WarningOutput::from(warning);
		let _ = writeln!(text, "warning [{}]: {}", output.name, output.message);
	}
	text
}

struct WarningOutput<'a> {
	name: &'static str,
	message: &'static str,
	fields: Vec<SafeField<'a>>,
}

impl<'a> From<&'a InstallWarning> for WarningOutput<'a> {
	fn from(warning: &'a InstallWarning) -> Self {
		let mut fields = Vec::new();
		let (name, message) = match warning {
			InstallWarning::FomodCouldBeUsableSelected { group_id, option_id } => {
				fields.push(SafeField::string("group_id", group_id));
				fields.push(SafeField::string("option_id", option_id));
				("fomod_could_be_usable_selected", "a CouldBeUsable option was selected")
			}
			InstallWarning::FomodConflictingFlagValues {
				flag_name,
				values,
				resolved_value,
				..
			} => {
				fields.push(SafeField::string("flag_name", flag_name));
				fields.push(SafeField::count("values", values.len()));
				for (index, value) in values.iter().enumerate() {
					fields.push(SafeField::string(format!("values[{index}]"), value));
				}
				fields.push(SafeField::string("resolved_value", resolved_value));
				(
					"fomod_conflicting_flag_values",
					"multiple selected options wrote different flag values",
				)
			}
			InstallWarning::FomodEmptyConditionList {
				operator,
				scope,
				group_id,
				option_id,
				pattern_order,
			} => {
				fields.push(SafeField::string(
					"operator",
					match operator {
						ConditionOperator::And => "and",
						ConditionOperator::Or => "or",
					},
				));
				fields.push(SafeField::string(
					"scope",
					match scope {
						ConditionScope::Module => "module",
						ConditionScope::StepVisibility => "step_visibility",
						ConditionScope::OptionTypePattern => "option_type_pattern",
						ConditionScope::ConditionalFilePattern => "conditional_file_pattern",
					},
				));
				if let Some(group_id) = group_id {
					fields.push(SafeField::string("group_id", group_id));
				}
				if let Some(option_id) = option_id {
					fields.push(SafeField::string("option_id", option_id));
				}
				if let Some(pattern_order) = pattern_order {
					fields.push(SafeField::unsigned("pattern_order", *pattern_order));
				}
				(
					"fomod_empty_condition_list",
					"an empty FOMOD condition list used compatibility behavior",
				)
			}
			InstallWarning::FomodMalformedGroupRepaired { group_id, repair } => {
				fields.push(SafeField::string("group_id", group_id));
				fields.push(SafeField::string(
					"repair",
					match repair {
						MalformedGroupRepair::SingleOptionExactlyOneToSelectAll => {
							"single_option_exactly_one_to_select_all"
						}
					},
				));
				("fomod_malformed_group_repaired", "a malformed FOMOD group was repaired")
			}
			InstallWarning::FomodFommDependencyAssumedCompatible { minimum_version } => {
				fields.push(SafeField::string("minimum_version", minimum_version));
				(
					"fomod_fomm_dependency_assumed_compatible",
					"a FOMM dependency was assumed compatible",
				)
			}
			InstallWarning::FomodEmptySourceIgnored {
				descriptor_order,
				group_id,
				option_id,
			} => {
				fields.push(SafeField::unsigned("descriptor_order", *descriptor_order));
				if let Some(group_id) = group_id {
					fields.push(SafeField::string("group_id", group_id));
				}
				if let Some(option_id) = option_id {
					fields.push(SafeField::string("option_id", option_id));
				}
				("fomod_empty_source_ignored", "an empty FOMOD source was ignored")
			}
			InstallWarning::FomodEmptyOptionAccepted { group_id, option_id } => {
				fields.push(SafeField::string("group_id", group_id));
				fields.push(SafeField::string("option_id", option_id));
				("fomod_empty_option_accepted", "an option with no effects was accepted")
			}
			InstallWarning::FomodModuleConfigPreferred {
				selected_config_member,
				ignored_config_member,
			} => {
				fields.push(SafeField::string("selected_config_member", selected_config_member));
				fields.push(SafeField::string("ignored_config_member", ignored_config_member));
				(
					"fomod_module_config_preferred",
					"ModuleConfig.xml was preferred over script.xml",
				)
			}
			InstallWarning::FomodNonPluginFileDependencyPolicyUsed {
				data_relative_path,
				state,
			} => {
				fields.push(SafeField::string("data_relative_path", data_relative_path));
				fields.push(SafeField::string(
					"state",
					match state {
						FileDependencyState::Missing => "missing",
						FileDependencyState::Inactive => "inactive",
						FileDependencyState::Active => "active",
					},
				));
				(
					"fomod_non_plugin_file_dependency_policy_used",
					"non-plugin file dependency policy was used",
				)
			}
			InstallWarning::FomodEqualPriorityTieResolved {
				destination,
				phase,
				declared_priority,
				winner_candidate_id,
				loser_candidate_ids,
			} => {
				fields.push(SafeField::owned_string(
					"destination",
					destination.as_str().replace('/', "\\"),
				));
				fields.push(SafeField::string(
					"phase",
					match phase {
						InstallationPhase::Required => "required",
						InstallationPhase::SelectedOrForced => "selected_or_forced",
						InstallationPhase::Conditional => "conditional",
					},
				));
				fields.push(SafeField::signed("declared_priority", *declared_priority));
				fields.push(SafeField::unsigned("winner_candidate_id", *winner_candidate_id));
				fields.push(SafeField::unsigned_list("loser_candidate_ids", loser_candidate_ids));
				(
					"fomod_equal_priority_tie_resolved",
					"an equal-priority file tie used stable descriptor order",
				)
			}
		};
		Self { name, message, fields }
	}
}

struct SafeField<'a> {
	name: String,
	value: SafeValue<'a>,
}

impl<'a> SafeField<'a> {
	fn string(name: impl Into<String>, value: &'a str) -> Self {
		Self {
			name: name.into(),
			value: SafeValue::String(Cow::Borrowed(value)),
		}
	}

	fn owned_string(name: impl Into<String>, value: String) -> Self {
		Self {
			name: name.into(),
			value: SafeValue::String(Cow::Owned(value)),
		}
	}

	fn count(name: impl Into<String>, value: usize) -> Self {
		Self {
			name: format!("{}.count", name.into()),
			value: SafeValue::Count(value),
		}
	}

	fn signed(name: impl Into<String>, value: i32) -> Self {
		Self {
			name: name.into(),
			value: SafeValue::Signed(value),
		}
	}

	fn unsigned(name: impl Into<String>, value: u64) -> Self {
		Self {
			name: name.into(),
			value: SafeValue::Unsigned(value),
		}
	}

	fn unsigned_list(name: impl Into<String>, value: &'a [u64]) -> Self {
		Self {
			name: name.into(),
			value: SafeValue::UnsignedList(value),
		}
	}

	fn write(&self, text: &mut String, prefix: &str) {
		let name = format!("{prefix}.{}", self.name);
		match &self.value {
			SafeValue::String(value) => write_string(text, &name, value),
			SafeValue::Count(value) => {
				let _ = writeln!(text, "{name} = {value}");
			}
			SafeValue::Signed(value) => {
				let _ = writeln!(text, "{name} = {value}");
			}
			SafeValue::Unsigned(value) => {
				let _ = writeln!(text, "{name} = {value}");
			}
			SafeValue::UnsignedList(values) => {
				let values = values.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
				let _ = writeln!(text, "{name} = [{values}]");
			}
		}
	}
}

enum SafeValue<'a> {
	String(Cow<'a, str>),
	Count(usize),
	Signed(i32),
	Unsigned(u64),
	UnsignedList(&'a [u64]),
}

fn write_string(text: &mut String, name: &str, value: &str) {
	let _ = writeln!(text, "{name} = {}", quote(value));
}

#[cfg(test)]
mod tests {
	use super::messages;
	use super::write_details;
	use application::installation::ChoiceSource;
	use application::installation::FlagWriter;
	use application::installation::InstallWarning;

	#[test]
	fn conflicting_flag_warning_omits_writer_provenance() {
		let warnings = [InstallWarning::FomodConflictingFlagValues {
			flag_name: "mode".to_owned(),
			values: vec!["legacy".to_owned(), "modern".to_owned()],
			resolved_value: "modern".to_owned(),
			writers: Vec::new(),
			winning_event: FlagWriter {
				sequence: 1,
				flag_effect_order: 0,
				source: ChoiceSource::Supplied,
				group_id: "second".to_owned(),
				option_id: "modern".to_owned(),
				value: "modern".to_owned(),
			},
		}];
		let mut details = String::new();

		write_details(&mut details, "warnings", &warnings);

		assert!(details.contains("warnings[0].flag_name = \"mode\""));
		assert!(details.contains("warnings[0].values.count = 2"));
		assert!(details.contains("warnings[0].values[0] = \"legacy\""));
		assert!(details.contains("warnings[0].resolved_value = \"modern\""));
		for removed in ["writers", "winning_event", "sequence", "flag_effect_order", "source"] {
			assert!(!details.contains(removed));
		}
		assert_eq!(
			messages(&warnings),
			concat!(
				"warning [fomod_conflicting_flag_values]: ",
				"multiple selected options wrote different flag values\n",
			)
		);
	}
}
