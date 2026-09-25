use application::settings::EffectiveBinding;
use application::settings::SetGameDirectoryOutput;
use application::settings::SetGameDirectoryWarning;
use application::settings::SettingRecord;
use application::settings::SettingSource;
use application::settings::SettingValue;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use serde_json::Value;
use serde_json::json;

pub(crate) fn setting(record: &SettingRecord) -> Value {
	let source = match record.source {
		SettingSource::Manifest => json!({"kind": "manifest"}),
		SettingSource::Environment { .. } => json!({"kind": "environment", "variable": "mods_game_dir"}),
		SettingSource::Invocation { .. } => json!({"kind": "invocation", "argument": "game_dir"}),
	};

	json!({
		"key": record.key.manifest_path(),
		"value": value(&record.value),
		"source": source,
		"manifest_value": value(&record.manifest_value),
		"manifest_path": record.manifest_path,
		"shadowed": record.shadowed,
		"writable": record.writable,
	})
}

pub(crate) fn mutation(output: &SetGameDirectoryOutput) -> CallToolResult {
	let source = match output.source {
		SettingSource::Manifest => json!({"kind": "manifest"}),
		SettingSource::Environment { .. } => json!({"kind": "environment", "variable": "mods_game_dir"}),
		SettingSource::Invocation { .. } => json!({"kind": "invocation", "argument": "game_dir"}),
	};
	let binding = match output.effective_binding {
		EffectiveBinding::Valid => "valid",
		EffectiveBinding::Invalid => "invalid",
	};
	let warnings: Vec<_> = output
		.warnings
		.iter()
		.map(|warning| match warning {
			SetGameDirectoryWarning::EffectiveGameBindingInvalid {
				expected_build_id,
				actual_build_id,
				..
			} => json!({
				"code": "effective_game_binding_invalid",
				"fields": {"variable": "mods_game_dir", "expected_build_id": expected_build_id, "actual_build_id": actual_build_id}
			}),
		})
		.collect();

	let mut result = CallToolResult::structured(json!({
		"outcome": "complete",
		"stored_value": {"kind": "path", "value": output.stored_value.as_path().to_string_lossy()},
		"stored_observed_build_id": output.stored_observed_build_id.get(),
		"effective_value": {"kind": "path", "value": output.effective_value.as_path().to_string_lossy()},
		"source": source,
		"shadowed": output.shadowed,
		"effective_binding": binding,
		"warnings": warnings,
	}));
	result.content.extend(output.warnings.iter().map(|SetGameDirectoryWarning::EffectiveGameBindingInvalid { expected_build_id, actual_build_id, .. }| {
		ContentBlock::text(format!("warning: MODS_GAME_DIR overrides the stored game directory and the effective game binding is invalid (expected build {expected_build_id}, actual build {actual_build_id}); correct or unset MODS_GAME_DIR"))
	}));

	result
}

fn value(value: &SettingValue) -> Value {
	match value {
		SettingValue::Unset => json!({"kind": "unset"}),
		SettingValue::String(value) => json!({"kind": "string", "value": value}),
		SettingValue::Path(value) => json!({"kind": "path", "value": value.to_string_lossy()}),
		SettingValue::UnsignedInteger(value) => json!({"kind": "unsigned_integer", "value": value}),
	}
}

#[cfg(test)]
mod tests {
	use super::mutation;
	use super::setting;
	use crate::contract::Contracts;
	use application::settings::EffectiveBinding;
	use application::settings::SetGameDirectoryOutput;
	use application::settings::SetGameDirectoryWarning;
	use application::settings::SettingKey;
	use application::settings::SettingRecord;
	use application::settings::SettingSource;
	use application::settings::SettingValue;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use rootcause::Result;
	use rootcause::report;
	use serde_json::json;
	use std::env::temp_dir;

	#[test]
	fn setting_output_preserves_typed_values_and_normalized_source() {
		let record = SettingRecord {
			key: SettingKey::GameDir,
			value: SettingValue::Path("game".into()),
			manifest_value: SettingValue::Path("stored".into()),
			source: SettingSource::Environment {
				variable: "MODS_GAME_DIR",
			},
			manifest_path: "game_dir",
			shadowed: true,
			writable: true,
		};

		assert_eq!(
			setting(&record),
			json!({"key": "game_dir", "value": {"kind": "path", "value": "game"}, "manifest_value": {"kind": "path", "value": "stored"}, "source": {"kind": "environment", "variable": "mods_game_dir"}, "manifest_path": "game_dir", "shadowed": true, "writable": true})
		);
	}
	#[test]
	fn mutation_success_has_structured_facts() -> Result<()> {
		let output = SetGameDirectoryOutput {
			stored_value: GameInstallationPath::new(temp_dir().join("stored"))?,
			stored_observed_build_id: SteamBuildId::new(12)?,
			effective_value: GameInstallationPath::new(temp_dir().join("effective"))?,
			source: SettingSource::Environment {
				variable: "MODS_GAME_DIR",
			},
			shadowed: true,
			effective_binding: EffectiveBinding::Invalid,
			warnings: vec![SetGameDirectoryWarning::EffectiveGameBindingInvalid {
				variable: "MODS_GAME_DIR",
				expected_build_id: 12,
				actual_build_id: 13,
			}],
		};

		let result = mutation(&output);
		let body = result
			.structured_content
			.as_ref()
			.ok_or_else(|| report!("missing structured mutation success"))?;

		assert!(Contracts::new()?.output_matches("mods_config_set_game_dir", body));
		assert_eq!(body["stored_observed_build_id"], 12);
		assert_eq!(body["warnings"][0]["fields"]["actual_build_id"], 13);
		assert_eq!(body["effective_binding"], "invalid");
		Ok(())
	}
}
