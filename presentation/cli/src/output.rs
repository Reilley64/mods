use application::environment::InitializeEnvironmentOutput;
use application::environment::InitializeEnvironmentWarning;
use application::settings::SetGameDirectoryOutput;
use application::settings::SetGameDirectoryWarning;
use application::settings::SettingRecord;
use application::settings::SettingSource;
use application::settings::SettingValue;
use serde_json::to_string;

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
	use super::*;
	use application::settings::EffectiveBinding;
	use application::settings::SettingKey;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use std::env::current_dir;
	use std::error::Error;

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
