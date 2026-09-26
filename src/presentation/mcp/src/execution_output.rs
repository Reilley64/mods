use application::execution::ExecuteProgramOutput;
use application::execution::ExecutionWarning;
use infrastructure_dependencies::CapturedOutput;
use infrastructure_dependencies::CapturedStream;
use rmcp::model::CallToolResult;
use rmcp::model::ContentBlock;
use serde_json::Value;
use serde_json::json;

pub(crate) fn executed(output: ExecuteProgramOutput, captured: CapturedOutput) -> CallToolResult {
	let mut result = CallToolResult::structured(json!({
	    "outcome": "executed", "status": {"origin": "child", "value": output.status.value()},
	    "stdout": stream(captured.stdout), "stderr": stream(captured.stderr), "warnings": []
	}));
	for warning in output.warnings {
		let text = match warning {
			ExecutionWarning::LoadOrderNotEnforced => {
				"warning: Plugin diagnostics use the analytical Data projection, not an observed runtime view. Mappings use canonical Profile State; this computed list does not change those files. Projected plugin order is advisory and is not enforced through virtual timestamps.".to_owned()
			}
			ExecutionWarning::StalePluginEntry { name } => {
				format!("warning: analysis projection: plugins.txt entry {name} is absent from the analytical Data view; runtime availability is not established.")
			}
			ExecutionWarning::StaleLoadOrderEntry { name } => {
				format!("warning: analysis projection: loadorder.txt entry {name} is absent from the analytical Data view; runtime availability is not established.")
			}
			ExecutionWarning::DuplicatePluginEntry { file, name } => {
				format!("warning: duplicate entry {name} in {file}; analysis projection uses the first occurrence; canonical file is unchanged.")
			}
			ExecutionWarning::UnlistedPlugin { name } => {
				format!("warning: analysis projection: {name} is absent from loadorder.txt; projected order uses backing-file modification time.")
			}
			ExecutionWarning::ProfileStateInvalid => {
				"warning: retained profile state is invalid after execution".to_owned()
			}
		};
		result.content.push(ContentBlock::text(text));
	}

	result
}

fn stream(stream: CapturedStream) -> Value {
	let mut output = json!({"byte_count": stream.byte_count, "sha256": stream.sha256, "binary_output": stream.text.is_none()});
	if let Some(text) = stream.text {
		output["text"] = json!(text);
	}

	output
}

#[cfg(test)]
mod tests {
	use super::executed;
	use crate::contract::Contracts;
	use application::execution::ExecuteProgramOutput;
	use application::execution::ExecutionWarning;
	use domain::ProcessStatus;
	use infrastructure_dependencies::CapturedOutput;
	use infrastructure_dependencies::CapturedStream;
	use rootcause::Result;
	use rootcause::report;
	use serde_json::json;
	use serde_json::to_string;

	#[test]
	fn preserves_complete_text_binary_metadata_and_nonzero_child_status() -> Result<()> {
		let result = executed(
			ExecuteProgramOutput {
				status: ProcessStatus::new(259),
				warnings: vec![
					ExecutionWarning::LoadOrderNotEnforced,
					ExecutionWarning::StalePluginEntry {
						name: "Missing.esp".into(),
					},
					ExecutionWarning::StaleLoadOrderEntry {
						name: "Ordered.esp".into(),
					},
					ExecutionWarning::DuplicatePluginEntry {
						file: "plugins.txt".into(),
						name: "Duplicate.esp".into(),
					},
					ExecutionWarning::UnlistedPlugin {
						name: "Unlisted.esp".into(),
					},
					ExecutionWarning::ProfileStateInvalid,
				],
			},
			CapturedOutput {
				stdout: CapturedStream {
					byte_count: 6,
					sha256: "a".repeat(64),
					text: Some("secret".into()),
				},
				stderr: CapturedStream {
					byte_count: 1,
					sha256: "b".repeat(64),
					text: None,
				},
			},
		);
		let visible = to_string(&result.content)?;
		assert!(visible.contains("warning: Plugin diagnostics use the analytical Data projection, not an observed runtime view. Mappings use canonical Profile State; this computed list does not change those files. Projected plugin order is advisory and is not enforced through virtual timestamps."));
		assert!(visible.contains("warning: analysis projection: plugins.txt entry Missing.esp is absent from the analytical Data view; runtime availability is not established."));
		assert!(visible.contains("warning: analysis projection: loadorder.txt entry Ordered.esp is absent from the analytical Data view; runtime availability is not established."));
		assert!(visible.contains("warning: duplicate entry Duplicate.esp in plugins.txt; analysis projection uses the first occurrence; canonical file is unchanged."));
		assert!(visible.contains("warning: analysis projection: Unlisted.esp is absent from loadorder.txt; projected order uses backing-file modification time."));
		assert!(visible.contains("warning: retained profile state is invalid after execution"));
		let value = result.structured_content.ok_or_else(|| report!("missing output"))?;

		assert!(Contracts::new()?.output_matches("mods_exec", &value));
		assert_eq!(
			value,
			json!({
			"outcome": "executed",
			"status": {"origin": "child", "value": 259},
			"stdout": {"byte_count": 6, "sha256": "a".repeat(64), "binary_output": false, "text": "secret"},
			"stderr": {"byte_count": 1, "sha256": "b".repeat(64), "binary_output": true},
			"warnings": []
			})
		);
		assert_eq!(value["status"]["value"], json!(259));
		assert_eq!(value["stderr"]["binary_output"], json!(true));
		assert!(value["stderr"].get("text").is_none());
		Ok(())
	}
}
