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
				"warning: derived plugin load order is not enforced by virtual timestamps".to_owned()
			}
			ExecutionWarning::StalePluginEntry { name } => {
				format!("warning: plugin entry is unavailable: {name}")
			}
			ExecutionWarning::StaleLoadOrderEntry { name } => {
				format!("warning: load order entry is unavailable: {name}")
			}
			ExecutionWarning::DuplicatePluginEntry { file, name } => {
				format!("warning: duplicate plugin entry in {file}: {name}")
			}
			ExecutionWarning::UnlistedPlugin { name } => {
				format!("warning: plugin uses fallback load order: {name}")
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
		assert!(visible.contains("virtual timestamps"));
		assert!(visible.contains("Missing.esp"));
		let value = result.structured_content.ok_or_else(|| report!("missing output"))?;

		assert!(Contracts::new()?.output_matches("mods_exec", &value));
		assert_eq!(value["stdout"]["text"], json!("secret"));
		assert_eq!(value["status"]["value"], json!(259));
		assert_eq!(value["stderr"]["binary_output"], json!(true));
		assert!(value["stderr"].get("text").is_none());
		Ok(())
	}
}
