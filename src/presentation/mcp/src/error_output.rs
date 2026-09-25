use application::ErrorCode;
use application::ErrorMarker;
use rmcp::ErrorData;
use rmcp::model::CallToolResult;
use rootcause::Report;
use serde_json::json;

pub(crate) fn application_error<C>(tool: &str, report: &Report<C>) -> Result<CallToolResult, ErrorData> {
	let marker = report
		.iter_reports()
		.find_map(|report| report.downcast_current_context::<ErrorMarker>())
		.ok_or_else(|| ErrorData::internal_error("operation error has no public classification", None))?;

	let mut code = marker.code().as_str();
	let details = match marker.code() {
		ErrorCode::EnvironmentNotInitialized
		| ErrorCode::EnvironmentRootUnsafe
		| ErrorCode::EnvironmentSchemaUnsupported => None,
		ErrorCode::EnvironmentInvalid | ErrorCode::ManualCleanupRequired => {
			code = "environment_invalid";
			None
		}

		ErrorCode::InvalidModName => Some(json!({"field": "mod_name"})),
		ErrorCode::InvalidDataPath => Some(json!({"field": "path"})),
		ErrorCode::InvalidOutputTarget => Some(json!({"field": "output_target"})),
		ErrorCode::InvalidWorkingDirectory => Some(json!({"field": "cwd"})),
		ErrorCode::GameInstallInvalid | ErrorCode::GameInstallNotFound => Some(json!({"field": "game_dir"})),
		ErrorCode::SettingValueInvalid => Some(json!({"field": "game_dir", "setting_key": "game_dir"})),
		ErrorCode::SettingUnknown | ErrorCode::SettingReadOnly => Some(
			json!({"setting_key": marker.setting_key().ok_or_else(|| ErrorData::internal_error("setting error facts are incomplete", None))?}),
		),
		ErrorCode::GameBuildMismatch => {
			let (expected, actual) = marker
				.build_ids()
				.ok_or_else(|| ErrorData::internal_error("build error facts are incomplete", None))?;
			Some(json!({"expected_build_id": expected, "actual_build_id": actual}))
		}
		ErrorCode::InvalidSelection => {
			let mut details = json!({"field": "choices"});
			if let Some(group_id) = marker.group_id() {
				details["group_id"] = json!(group_id);
				if let Some(option_id) = marker.option_id() {
					details["option_id"] = json!(option_id);
				}
			}
			Some(details)
		}
		ErrorCode::ModAlreadyExists
		| ErrorCode::ModNotFound
		| ErrorCode::OutputTargetNotFound
		| ErrorCode::OutputTargetDisabled => {
			let name = marker
				.mod_name()
				.ok_or_else(|| ErrorData::internal_error("mod error facts are incomplete", None))?;
			Some(json!({"mod_name": name.as_str()}))
		}
		ErrorCode::ProgramNotFound | ErrorCode::ProgramUnsupported | ErrorCode::ProgramLaunchFailed => Some(
			json!({"field": "program", "status": {"origin": "launcher", "value": if marker.code() == ErrorCode::ProgramNotFound { 127 } else { 126 }}}),
		),
		ErrorCode::VfsFailed | ErrorCode::ExecutionSupervisionFailed => {
			let phase = marker.phase().ok_or_else(|| {
				ErrorData::internal_error("execution error facts are incomplete", None)
			})?;
			Some(json!({"phase": phase, "status": {"origin": "launcher", "value": 125}}))
		}
		ErrorCode::OperationCancelled if tool == "mods_exec" => {
			if marker.phase() != Some("cleanup") {
				return Err(ErrorData::internal_error("request cancelled before launch", None));
			}

			Some(json!({"phase": "cleanup", "status": {"origin": "launcher", "value": 3221225786u32}}))
		}
		ErrorCode::UnsupportedInstaller
		| ErrorCode::UnsafeArchive
		| ErrorCode::DependencyUnsatisfied
		| ErrorCode::AmbiguousInstallPlan
		| ErrorCode::IoFailure
		| ErrorCode::TransactionFailure
		| ErrorCode::OperationCancelled => {
			let phase = marker.phase().ok_or_else(|| {
				ErrorData::internal_error("operation phase facts are incomplete", None)
			})?;
			Some(json!({"phase": phase}))
		}
		ErrorCode::EnvironmentPublicationFailed if tool == "mods_install" => {
			code = "transaction_failure";
			let phase = marker.phase().ok_or_else(|| {
				ErrorData::internal_error("publication error facts are incomplete", None)
			})?;
			Some(json!({"phase": phase}))
		}
		ErrorCode::EnvironmentAlreadyInitialized
		| ErrorCode::EnvironmentRootNotEmpty
		| ErrorCode::EnvironmentPublicationFailed => {
			return Err(ErrorData::internal_error("error is not supported by this tool", None));
		}
	};

	let mut output =
		json!({"outcome": "error", "error": {"code": code, "message": marker.message(), "retryable": false}});
	if let Some(details) = details {
		output["error"]["details"] = details;
	}

	Ok(CallToolResult::structured_error(output))
}

#[cfg(test)]
mod tests {
	use super::application_error;
	use crate::contract::Contracts;
	use application::ErrorMarker;
	use domain::ModName;
	use rootcause::Result;
	use rootcause::report;
	use serde_json::json;

	#[test]
	fn safe_errors_preserve_only_schema_allowlisted_provenance() -> Result<()> {
		let fixtures = [
			("mods_exec", ErrorMarker::program_not_found()),
			(
				"mods_exec",
				ErrorMarker::execution_supervision_failed().with_phase("running"),
			),
			(
				"mods_install",
				ErrorMarker::mod_already_exists().with_mod_name(ModName::new("Canonical".into())?),
			),
			(
				"mods_install",
				ErrorMarker::invalid_selection(
					"choices",
					Some("group".into()),
					Some("option".into()),
					Some(99),
				),
			),
			("mods_config_set_game_dir", ErrorMarker::game_build_mismatch(123, 456)),
		];
		let contracts = Contracts::new()?;
		for (tool, marker) in fixtures {
			let report = report!("SECRET CAUSE").context(marker);
			let result = application_error(tool, &report)?;
			let value = result.structured_content.ok_or_else(|| report!("structured output"))?;
			assert!(contracts.output_matches(tool, &value));
			assert!(!value.to_string().contains("SECRET"));
			assert!(!value.to_string().contains("supplied_sequence"));
			assert_eq!(value["error"]["retryable"], json!(false));
		}
		Ok(())
	}
	#[test]
	fn invalid_environment_never_fabricates_recovery_details() -> Result<()> {
		let contracts = Contracts::new()?;
		for tool in [
			"mods_config_list",
			"mods_config_get",
			"mods_config_set_game_dir",
			"mods_conflicts_list",
			"mods_conflicts_inspect",
			"mods_conflicts_explain",
			"mods_install",
			"mods_exec",
		] {
			let report = report!(ErrorMarker::environment_invalid(Some("game_binding")));
			let result = application_error(tool, &report)?;
			let value = result.structured_content.ok_or_else(|| report!("missing output"))?;

			assert!(contracts.output_matches(tool, &value), "{tool}");
			assert!(value["error"].get("details").is_none());
		}
		Ok(())
	}
}
