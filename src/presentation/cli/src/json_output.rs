use application::ErrorMarker;
use application::settings::SetGameDirectoryWarning;
use application::settings::SettingRecord;
use application::settings::SettingSource;
use application::settings::SettingValue;
use clap::Error as ClapError;
use clap::error::ErrorKind;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

const PROBLEM_BASE: &str = "https://github.com/Reilley64/mods/blob/main/docs/cli/problems.md#";

pub(crate) fn problem(code: &str, title: &str, detail: &str, status: u32, details: Value) -> Value {
	let mut result = json!({
	    "type": format!("{PROBLEM_BASE}{code}"),
	    "title": title,
	    "detail": detail,
	    "exit_code": status,
	    "code": code,
	});
	if details != json!({}) {
		result["details"] = details;
	}
	result
}

pub(crate) fn marker_problem(marker: &ErrorMarker, status: u32) -> Value {
	let mut details = Map::new();
	if let Some(value) = marker.phase() {
		details.insert("phase".into(), json!(value));
	}
	if let Some(value) = marker.field() {
		details.insert("field".into(), json!(value));
	}
	if let Some(value) = marker.group_id() {
		details.insert("group_id".into(), json!(value));
	}
	if let Some(value) = marker.option_id() {
		details.insert("option_id".into(), json!(value));
	}
	if let Some(value) = marker.supplied_sequence() {
		details.insert("sequence".into(), json!(value));
	}
	if let Some((expected, actual)) = marker.build_ids() {
		details.insert("expected_build_id".into(), json!(expected));
		details.insert("actual_build_id".into(), json!(actual));
	}

	let code = marker.code().as_str();
	let mut title = code.replace('_', " ");
	if let Some(first) = title.get_mut(..1) {
		first.make_ascii_uppercase();
	}

	problem(code, &title, marker.message(), status, Value::Object(details))
}

pub(crate) fn clap_document(error: &ClapError) -> Value {
	match error.kind() {
		ErrorKind::DisplayHelp => json!({"help": error.to_string(), "warnings": []}),
		ErrorKind::DisplayVersion => json!({"version": env!("CARGO_PKG_VERSION"), "warnings": []}),
		ErrorKind::Io => problem(
			"startup_failed",
			"Startup failed",
			"unable to determine startup directory",
			error.exit_code() as u32,
			json!({}),
		),
		_ => problem(
			"invalid_arguments",
			"Invalid arguments",
			&error.to_string(),
			error.exit_code() as u32,
			json!({}),
		),
	}
}

pub(crate) fn document(value: &Value) -> String {
	format!("{}\n", value)
}

pub(crate) fn setting(record: &SettingRecord) -> Value {
	let setting_value = |value: &SettingValue| match value {
		SettingValue::Unset => Value::Null,
		SettingValue::String(value) => json!(value),
		SettingValue::Path(value) => json!(value.display().to_string()),
		SettingValue::UnsignedInteger(value) => json!(value),
	};
	let source = match &record.source {
		SettingSource::Manifest => json!({"kind": "manifest"}),
		SettingSource::Environment { variable } => json!({"kind": "environment", "variable": variable}),
		SettingSource::Invocation { argument } => json!({"kind": "invocation", "argument": argument}),
	};
	json!({
	    "key": record.key.cli_name(), "value": setting_value(&record.value),
	    "source": source, "manifest_value": setting_value(&record.manifest_value),
	    "manifest_path": record.manifest_path, "shadowed": record.shadowed,
	    "writable": record.writable,
	})
}

pub(crate) fn game_directory_warnings(values: &[SetGameDirectoryWarning]) -> Vec<Value> {
	values.iter()
		.map(|warning_value| match warning_value {
			SetGameDirectoryWarning::EffectiveGameBindingInvalid {
				variable,
				expected_build_id,
				actual_build_id,
			} => warning(
				"effective_game_binding_invalid",
				"MODS_GAME_DIR build does not match observed-build-id",
				json!({"variable": variable, "expected_build_id": expected_build_id, "actual_build_id": actual_build_id}),
			),
		})
		.collect()
}

pub(crate) fn warning(code: &str, message: &str, details: Value) -> Value {
	json!({ "code": code, "message": message, "details": details })
}

pub(crate) fn diagnostic_warning() -> Value {
	warning(
		"diagnostic_logging_unavailable",
		"diagnostic session logging is unavailable",
		json!({}),
	)
}

#[cfg(test)]
mod tests {
	use super::clap_document;
	use super::game_directory_warnings;
	use super::marker_problem;
	use super::setting;
	use crate::commands::parse_from;
	use application::ErrorCode;
	use application::ErrorMarker;
	use application::settings::SetGameDirectoryWarning;
	use application::settings::SettingKey;
	use application::settings::SettingRecord;
	use application::settings::SettingSource;
	use application::settings::SettingValue;
	use serde_json::json;
	use std::error::Error;

	#[test]
	fn setting_uses_null_and_structured_override_without_human_sentinels() {
		let record = SettingRecord {
			key: SettingKey::Name,
			value: SettingValue::Unset,
			source: SettingSource::Environment {
				variable: "MODS_GAME_DIR",
			},
			manifest_value: SettingValue::String("unset".to_owned()),
			manifest_path: "name",
			shadowed: true,
			writable: true,
		};
		let value = setting(&record);
		assert_eq!(
			value,
			json!({
			    "key": "name", "value": null, "source": {"kind": "environment", "variable": "MODS_GAME_DIR"},
			    "manifest_value": "unset", "manifest_path": "name", "shadowed": true, "writable": true,
			})
		);
	}

	#[test]
	fn problem_type_and_allowlisted_details_do_not_include_internal_cause() {
		let marker = ErrorMarker::invalid_selection(
			"choices",
			Some("group".to_owned()),
			Some("option".to_owned()),
			Some(2),
		);
		let value = marker_problem(&marker, 2);
		assert_eq!(
			value["type"],
			"https://github.com/Reilley64/mods/blob/main/docs/cli/problems.md#invalid_selection"
		);
		assert_eq!(value["title"], "Invalid selection");
		assert_eq!(
			value["details"],
			json!({"field": "choices", "group_id": "group", "option_id": "option", "sequence": 2})
		);
		assert!(value.get("status").is_none());
	}
	#[test]
	fn game_directory_warning_keeps_actionable_build_ids() {
		let value = game_directory_warnings(&[SetGameDirectoryWarning::EffectiveGameBindingInvalid {
			variable: "MODS_GAME_DIR",
			expected_build_id: 10,
			actual_build_id: 11,
		}]);
		assert_eq!(
			value[0]["details"],
			json!({"variable": "MODS_GAME_DIR", "expected_build_id": 10, "actual_build_id": 11})
		);
	}
	#[test]
	fn help_version_and_argument_failure_have_one_structured_document() -> Result<(), Box<dyn Error>> {
		for (arguments, key) in [
			(vec!["mods", "--json", "--help"], "help"),
			(vec!["mods", "--json", "--version"], "version"),
			(vec!["mods", "--json", "missing"], "type"),
		] {
			let error = parse_from(arguments).err().ok_or("expected clap document")?;
			let document = clap_document(&error);
			assert!(document.get(key).is_some());
			assert!(document.get("status").is_none());
		}
		Ok(())
	}

	#[test]
	fn hidden_exec_rejects_json_before_any_launch() -> Result<(), Box<dyn Error>> {
		for arguments in [
			vec!["mods", "--json", "exec", "--hidden", "--", "tool.exe"],
			vec!["mods", "exec", "--hidden", "--json", "--", "tool.exe"],
		] {
			let error = parse_from(arguments).err().ok_or("expected argument conflict")?;
			let document = clap_document(&error);
			assert_eq!(document["code"], "invalid_arguments");
			assert_eq!(document["exit_code"], 2);
		}
		assert!(parse_from(["mods", "exec", "--hidden", "--", "tool.exe", "--json"]).is_ok());
		Ok(())
	}

	#[test]
	fn every_problem_code_has_a_problem_anchor() {
		let problems = include_str!("../../../../docs/cli/problems.md");
		let codes = [
			ErrorCode::NexusSourceInvalid,
			ErrorCode::NexusPremiumRequired,
			ErrorCode::NexusCredentialsInvalid,
			ErrorCode::NexusAccessDenied,
			ErrorCode::NexusRateLimited,
			ErrorCode::NexusUnavailable,
			ErrorCode::NexusNetworkFailure,
			ErrorCode::NexusResponseInvalid,
			ErrorCode::EnvironmentNotInitialized,
			ErrorCode::EnvironmentAlreadyInitialized,
			ErrorCode::EnvironmentRootNotEmpty,
			ErrorCode::EnvironmentRootUnsafe,
			ErrorCode::EnvironmentSchemaUnsupported,
			ErrorCode::EnvironmentInvalid,
			ErrorCode::EnvironmentPublicationFailed,
			ErrorCode::ManualCleanupRequired,
			ErrorCode::GameInstallNotFound,
			ErrorCode::GameInstallInvalid,
			ErrorCode::GameBuildMismatch,
			ErrorCode::SettingUnknown,
			ErrorCode::SettingReadOnly,
			ErrorCode::SettingValueInvalid,
			ErrorCode::InvalidSelection,
			ErrorCode::UnsupportedInstaller,
			ErrorCode::DependencyUnsatisfied,
			ErrorCode::UnsafeArchive,
			ErrorCode::AmbiguousInstallPlan,
			ErrorCode::InvalidModName,
			ErrorCode::InvalidDataPath,
			ErrorCode::ModAlreadyExists,
			ErrorCode::ModNotFound,
			ErrorCode::IoFailure,
			ErrorCode::TransactionFailure,
			ErrorCode::InvalidOutputTarget,
			ErrorCode::OutputTargetNotFound,
			ErrorCode::OutputTargetDisabled,
			ErrorCode::InvalidWorkingDirectory,
			ErrorCode::ProgramNotFound,
			ErrorCode::ProgramUnsupported,
			ErrorCode::ProgramLaunchFailed,
			ErrorCode::VfsFailed,
			ErrorCode::ExecutionSupervisionFailed,
			ErrorCode::OperationCancelled,
			ErrorCode::ShortcutUnsupported,
			ErrorCode::ShortcutNameInvalid,
			ErrorCode::ShortcutDestinationInvalid,
			ErrorCode::ShortcutLaunchInvalid,
			ErrorCode::ShortcutArgumentsTooLong,
			ErrorCode::ShortcutFailed,
		];
		for code in codes {
			match code {
				ErrorCode::NexusSourceInvalid
				| ErrorCode::NexusPremiumRequired
				| ErrorCode::NexusCredentialsInvalid
				| ErrorCode::NexusAccessDenied
				| ErrorCode::NexusRateLimited
				| ErrorCode::NexusUnavailable
				| ErrorCode::NexusNetworkFailure
				| ErrorCode::NexusResponseInvalid
				| ErrorCode::EnvironmentNotInitialized
				| ErrorCode::EnvironmentAlreadyInitialized
				| ErrorCode::EnvironmentRootNotEmpty
				| ErrorCode::EnvironmentRootUnsafe
				| ErrorCode::EnvironmentSchemaUnsupported
				| ErrorCode::EnvironmentInvalid
				| ErrorCode::EnvironmentPublicationFailed
				| ErrorCode::ManualCleanupRequired
				| ErrorCode::GameInstallNotFound
				| ErrorCode::GameInstallInvalid
				| ErrorCode::GameBuildMismatch
				| ErrorCode::SettingUnknown
				| ErrorCode::SettingReadOnly
				| ErrorCode::SettingValueInvalid
				| ErrorCode::InvalidSelection
				| ErrorCode::UnsupportedInstaller
				| ErrorCode::DependencyUnsatisfied
				| ErrorCode::UnsafeArchive
				| ErrorCode::AmbiguousInstallPlan
				| ErrorCode::InvalidModName
				| ErrorCode::InvalidDataPath
				| ErrorCode::ModAlreadyExists
				| ErrorCode::ModNotFound
				| ErrorCode::IoFailure
				| ErrorCode::TransactionFailure
				| ErrorCode::InvalidOutputTarget
				| ErrorCode::OutputTargetNotFound
				| ErrorCode::OutputTargetDisabled
				| ErrorCode::InvalidWorkingDirectory
				| ErrorCode::ProgramNotFound
				| ErrorCode::ProgramUnsupported
				| ErrorCode::ProgramLaunchFailed
				| ErrorCode::VfsFailed
				| ErrorCode::ExecutionSupervisionFailed
				| ErrorCode::OperationCancelled
				| ErrorCode::ShortcutUnsupported
				| ErrorCode::ShortcutNameInvalid
				| ErrorCode::ShortcutDestinationInvalid
				| ErrorCode::ShortcutLaunchInvalid
				| ErrorCode::ShortcutArgumentsTooLong
				| ErrorCode::ShortcutFailed => {}
			}
		}
		let synthetic = [
			"invalid_arguments",
			"environment_root_selection_failed",
			"startup_failed",
			"operation_failed",
			"nexus_file_selection_required",
		];
		for code in codes.map(ErrorCode::as_str).into_iter().chain(synthetic) {
			assert!(
				problems.contains(&format!("<a id=\"{code}\"></a>\n## {code}\n")),
				"{code}"
			);
		}
	}
}
