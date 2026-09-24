use rmcp::handler::server::common::schema_for_input;
use rmcp::model::Tool;
use rmcp::model::ToolAnnotations;
use rmcp::schemars;
use rmcp::schemars::JsonSchema;
use rootcause::Result;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigList {}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConfigKey {
	SchemaVersion,
	Name,
	GameDir,
	SteamAppId,
	ObservedBuildId,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigGet {
	pub key: ConfigKey,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigSetGameDir {
	pub game_dir: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DirectChoice {
	pub group_id: String,
	pub option_id: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Install {
	pub archive: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(with = "String")]
	pub mod_name: Option<String>,
	#[serde(default)]
	pub replace: bool,
	#[serde(default)]
	pub choices: Vec<DirectChoice>,
	#[serde(default)]
	pub dry_run: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConflictsList {
	#[serde(default)]
	pub compare_content: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConflictsInspect {
	pub mod_name: String,
	#[serde(default)]
	pub compare_content: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConflictsExplain {
	pub path: String,
	#[serde(default)]
	pub compare_content: bool,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Exec {
	pub program: String,
	#[serde(default)]
	pub args: Vec<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(with = "String")]
	pub output_target: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	#[schemars(with = "String")]
	pub cwd: Option<String>,
}

pub(crate) fn tools() -> Result<Vec<Tool>> {
	let definitions = [
		("mods_config_get", schema_for_input::<ConfigGet>()),
		("mods_config_list", schema_for_input::<ConfigList>()),
		("mods_config_set_game_dir", schema_for_input::<ConfigSetGameDir>()),
		("mods_conflicts_explain", schema_for_input::<ConflictsExplain>()),
		("mods_conflicts_inspect", schema_for_input::<ConflictsInspect>()),
		("mods_conflicts_list", schema_for_input::<ConflictsList>()),
		("mods_exec", schema_for_input::<Exec>()),
		("mods_install", schema_for_input::<Install>()),
	];

	definitions
		.into_iter()
		.map(|(name, schema)| {
			let schema = schema.map_err(|cause| report!(cause))?;
			let read_only = matches!(
				name,
				"mods_config_get"
					| "mods_config_list" | "mods_conflicts_explain"
					| "mods_conflicts_inspect" | "mods_conflicts_list"
			);
			let annotations = ToolAnnotations::new()
				.read_only(read_only)
				.destructive(!read_only)
				.idempotent(read_only)
				.open_world(!read_only);

			Ok(Tool::new(name, name, schema).with_annotations(annotations))
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::tools;
	use jsonschema::draft202012;
	use rootcause::Result;
	use serde_json::Map;
	use serde_json::Value;
	use serde_json::from_str;
	use serde_json::json;

	fn canonical(schema: &Value, document: &Value, depth: usize) -> Value {
		assert!(depth < 32, "recursive input schema reference");
		if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
			assert!(
				schema.as_object().is_some_and(|map| map.keys().all(|key| [
					"$ref",
					"title",
					"description"
				]
				.contains(&key.as_str()))),
				"unexpected ref sibling"
			);
			let target = reference
				.strip_prefix('#')
				.and_then(|pointer| document.pointer(pointer));
			assert!(target.is_some(), "unresolved local input reference");
			return canonical(target.unwrap_or(&Value::Null), document, depth + 1);
		}

		match schema {
			Value::Object(fields) => {
				assert!(!fields.contains_key("$id"), "unexpected schema base");

				let mut output = Map::new();
				for (key, value) in fields {
					if ["$schema", "$defs", "title", "description"].contains(&key.as_str()) {
						continue;
					}

					let mut value = canonical(value, document, depth + 1);
					if ["required", "enum", "type"].contains(&key.as_str())
						&& let Some(items) = value.as_array_mut()
					{
						items.sort_by_key(Value::to_string);
					}
					output.insert(key.clone(), value);
				}
				if output.get("type") == Some(&json!("object")) {
					output.entry("properties").or_insert(json!({}));
					output.entry("required").or_insert(json!([]));
				}
				Value::Object(output)
			}
			Value::Array(items) => {
				Value::Array(items.iter().map(|item| canonical(item, document, depth + 1)).collect())
			}
			value => value.clone(),
		}
	}

	#[test]
	fn generated_inputs_preserve_strict_rejection_and_optional_defaults() -> Result<()> {
		let reference: Value = from_str(include_str!("tool-schemas.json"))?;
		let fixtures = [
			("mods_config_get", json!({"key": "schema_version"})),
			("mods_config_list", json!({})),
			("mods_config_set_game_dir", json!({"game_dir": "game"})),
			("mods_conflicts_explain", json!({"path": "a", "compare_content": false})),
			(
				"mods_conflicts_inspect",
				json!({"mod_name": "a", "compare_content": false}),
			),
			("mods_conflicts_list", json!({"compare_content": false})),
			(
				"mods_exec",
				json!({"program": "game", "args": [], "cwd": "game", "output_target": "native"}),
			),
			(
				"mods_install",
				json!({"archive": "a.zip", "mod_name": "a", "replace": false, "dry_run": false, "choices": []}),
			),
		];

		for (tool, (name, valid)) in tools()?.into_iter().zip(fixtures) {
			assert_eq!(tool.name.as_ref(), name);
			let generated = Value::Object((*tool.input_schema).clone());
			let published = json!({"$defs": reference["$defs"], "$ref": reference["tools"][name]["input_schema"]["$ref"]});
			let generated_validator = draft202012::new(&generated)?;
			let published_validator = draft202012::new(&published)?;
			assert!(generated_validator.is_valid(&valid));
			assert!(published_validator.is_valid(&valid));

			let mut cases = vec![json!(null), json!([]), json!(false)];
			let mut unknown = valid.clone();
			unknown["unknown"] = json!("not echoed");
			cases.push(unknown);
			for key in valid.as_object().into_iter().flat_map(|fields| fields.keys()) {
				let mut null = valid.clone();
				null[key] = Value::Null;
				cases.push(null);
				let mut wrong = valid.clone();
				wrong[key] = json!({});
				cases.push(wrong);
				let mut absent = valid.clone();
				if let Some(object) = absent.as_object_mut() {
					object.remove(key);
				}
				assert_eq!(
					generated_validator.is_valid(&absent),
					published_validator.is_valid(&absent)
				);
			}
			if name == "mods_config_get" {
				cases.push(json!({"key": "unknown"}));
			}
			if name == "mods_install" {
				for choice in [
					json!({"group_id": "g"}),
					json!({"group_id": "g", "option_id": 1}),
					json!({"group_id": "g", "option_id": "o", "extra": true}),
				] {
					let mut invalid = valid.clone();
					invalid["choices"] = json!([choice]);
					cases.push(invalid);
				}
			}
			for invalid in cases {
				assert!(
					!generated_validator.is_valid(&invalid),
					"generated accepted {name}: {invalid}"
				);
				assert!(
					!published_validator.is_valid(&invalid),
					"published accepted {name}: {invalid}"
				);
			}
		}
		Ok(())
	}

	#[test]
	fn generated_inputs_match_published_contracts() -> Result<()> {
		let reference: Value = from_str(include_str!("tool-schemas.json"))?;
		assert_eq!(reference["$schema"], "https://json-schema.org/draft/2020-12/schema");

		for tool in tools()? {
			let generated = Value::Object((*tool.input_schema).clone());
			assert_eq!(generated["$schema"], reference["$schema"]);
			let expected = &reference["tools"][tool.name.as_ref()]["input_schema"];
			assert_eq!(
				canonical(&generated, &generated, 0),
				canonical(expected, &reference, 0),
				"{}",
				tool.name
			);
		}
		Ok(())
	}
	#[test]
	fn advertised_annotations_distinguish_read_only_and_mutating_tools() -> Result<()> {
		for tool in tools()? {
			let read_only = matches!(
				tool.name.as_ref(),
				"mods_config_get"
					| "mods_config_list" | "mods_conflicts_explain"
					| "mods_conflicts_inspect" | "mods_conflicts_list"
			);
			let annotation = tool
				.annotations
				.ok_or_else(|| rootcause::report!("tool annotations missing"))?;

			assert_eq!(annotation.read_only_hint, Some(read_only));
			assert_eq!(annotation.destructive_hint, Some(!read_only));
			assert_eq!(annotation.idempotent_hint, Some(read_only));
			assert_eq!(annotation.open_world_hint, Some(!read_only));
		}

		Ok(())
	}
}
