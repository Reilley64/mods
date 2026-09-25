use crate::inputs::tools;
use jsonschema::Validator;
use jsonschema::draft202012;
use jsonschema::error::ValidationErrorKind;
use rmcp::model::Tool;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use serde_json::Value;
use serde_json::from_str;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::Error;
use std::sync::Arc;

pub(crate) struct Contracts {
	inputs: BTreeMap<String, Validator>,
	outputs: BTreeMap<String, Validator>,
	tools: Vec<Tool>,
}

impl Contracts {
	pub(crate) fn new() -> Result<Self> {
		let document: Value = from_str(include_str!("tool-schemas.json")).into_report()?;
		let mut tools = tools()?;

		let mut inputs = BTreeMap::new();
		let mut outputs = BTreeMap::new();
		for tool in &mut tools {
			let name = tool.name.as_ref();
			let schema = Value::Object((*tool.input_schema).clone());
			inputs.insert(name.to_owned(), draft202012::new(&schema).into_report()?);

			let reference = document["tools"][name]["output_schema"]["$ref"]
				.as_str()
				.ok_or_else(|| Error::other("invalid bundled output schema"))?;
			let output_schema =
				json!({"$schema": document["$schema"], "$defs": document["$defs"], "$ref": reference});
			outputs.insert(name.to_owned(), draft202012::new(&output_schema).into_report()?);

			tool.output_schema = Some(Arc::new(
				output_schema
					.as_object()
					.cloned()
					.ok_or_else(|| Error::other("invalid output schema"))?,
			));
		}

		Ok(Self { inputs, outputs, tools })
	}

	pub(crate) fn output_matches(&self, name: &str, result: &Value) -> bool {
		self.outputs
			.get(name)
			.is_some_and(|validator| validator.is_valid(result))
	}

	pub(crate) fn tools(&self) -> Vec<Tool> {
		self.tools.clone()
	}

	pub(crate) fn validate(&self, name: &str, arguments: &Value) -> Result<Value> {
		let validator = self.inputs.get(name).ok_or_else(|| Error::other("unknown tool"))?;

		// Native diagnostics can contain supplied values; expose only sorted field/kind records.
		let mut problems = BTreeSet::new();
		for error in validator.iter_errors(arguments) {
			let pointer = error.instance_path().to_string();
			let mut field = String::new();
			for component in pointer.split('/').skip(1) {
				if component.parse::<usize>().is_ok() {
					field.push_str(&format!("[{component}]"));
					continue;
				}
				if !field.is_empty() {
					field.push('.');
				}
				field.push_str(component);
			}

			match error.kind() {
				ValidationErrorKind::Required { property } => {
					if !field.is_empty() {
						field.push('.');
					}
					field.push_str(property.as_str().unwrap_or("arguments"));
					problems.insert((field, "missing"));
				}
				ValidationErrorKind::AdditionalProperties { unexpected } => {
					for property in unexpected {
						let property = if property.chars().all(|character| {
							character.is_ascii_alphanumeric() || character == '_'
						}) {
							property.as_str()
						} else {
							"<unknown>"
						};
						let path = if field.is_empty() {
							property.to_owned()
						} else {
							format!("{field}.{property}")
						};
						problems.insert((path, "unknown"));
					}
				}
				kind => {
					let instance = arguments.pointer(&pointer);
					let kind = if instance.is_some_and(Value::is_null) {
						"null_not_allowed"
					} else if matches!(
						kind,
						ValidationErrorKind::Enum { .. } | ValidationErrorKind::Constant { .. }
					) && instance.is_some_and(Value::is_string)
					{
						"invalid_enum"
					} else {
						"wrong_type"
					};
					if field.is_empty() {
						field.push_str("arguments");
					}
					problems.insert((field, kind));
				}
			}
		}

		Ok(Value::Array(
			problems.into_iter()
				.map(|(field, kind)| json!({"field": field, "kind": kind}))
				.collect(),
		))
	}
}

#[cfg(test)]
mod tests {
	use super::Contracts;
	use rootcause::Result;
	use serde_json::json;

	#[test]
	fn advertises_exactly_the_eight_fixed_tools() -> Result<()> {
		let contracts = Contracts::new()?;
		let tools = contracts.tools();
		let names: Vec<_> = tools.iter().map(|tool| tool.name.as_ref()).collect();
		assert_eq!(
			names,
			[
				"mods_config_get",
				"mods_config_list",
				"mods_config_set_game_dir",
				"mods_conflicts_explain",
				"mods_conflicts_inspect",
				"mods_conflicts_list",
				"mods_exec",
				"mods_install"
			]
		);
		Ok(())
	}

	#[test]
	fn every_advertised_tool_has_its_output_schema() -> Result<()> {
		for tool in Contracts::new()?.tools() {
			assert!(tool.output_schema.is_some(), "{} has no output schema", tool.name);
		}
		Ok(())
	}

	#[test]
	fn tool_arguments_are_strict_and_errors_do_not_echo_values() -> Result<()> {
		let contracts = Contracts::new()?;
		let errors = contracts.validate(
			"mods_install",
			&json!({"dry_run": null, "choices": [{"group_id": "g", "option_id": 1}], "secret": "never echo"}),
		)?;
		assert_eq!(
			errors,
			json!([
				{"field": "archive", "kind": "missing"},
				{"field": "choices[0].option_id", "kind": "wrong_type"},
				{"field": "dry_run", "kind": "null_not_allowed"},
				{"field": "secret", "kind": "unknown"}
			])
		);
		Ok(())
	}

	#[test]
	fn output_contract_rejects_missing_data_fields() -> Result<()> {
		let contracts = Contracts::new()?;

		assert!(contracts.output_matches("mods_config_list", &json!({"outcome": "complete", "settings": []})));
		assert!(!contracts.output_matches("mods_config_list", &json!({"outcome": "complete"})));
		assert!(!contracts.output_matches(
			"mods_config_list",
			&json!({"outcome": "complete", "settings": [], "extra": true})
		));
		Ok(())
	}
}
