use crate::diagnostics::SINK_WARNING;
use crate::json_output;
use crate::output::quote;
use application::ErrorCode;
use application::ErrorMarker;
use application::export::CompletedExport;
use application::export::RetainedExport;
use application::installation::DownloadModFile;
use application::ports::LoadOrderFile;
use application::ports::RetainedProfile;
use rootcause::Report;
use rootcause::report;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::fmt::Display;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

pub(crate) const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;
const INVALID_INVOCATION_STATUS: u32 = 2;
const FILE_SELECTION_REQUIRED: &str = "nexus_file_selection_required";
const FILE_SELECTION_MESSAGE: &str = "Select a file with --file <id>.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutcome {
	pub(crate) status: u32,
	pub(crate) stdout: String,
	pub(crate) stderr: String,
	pub(crate) command_failed: bool,
	pub(crate) diagnostic_log: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandFamily {
	Ordinary,
	Execution,
}

pub(crate) enum CommandResult {
	Succeeded {
		stdout: String,
		stderr: String,
		document: Value,
		warnings: Vec<Value>,
	},
	ProgramExited {
		status: u32,
		stderr: String,
	},
	Failed(Report),
	FailedAfterLaunch(Report),
	ExportFailed {
		report: Report,
		output: PathBuf,
	},
	FileSelectionRequired(Vec<DownloadModFile>),
	HiddenExecutionUnsupported,
	RootSelectionFailed(&'static str),
}

pub(crate) struct OutcomeContext {
	pub(crate) family: CommandFamily,
	pub(crate) json: bool,
	pub(crate) quiet_success: bool,
	pub(crate) diagnostic_logging_unavailable: bool,
	pub(crate) diagnostic_session: Option<Uuid>,
	pub(crate) diagnostic_log: Option<PathBuf>,
}

pub(crate) fn outcome(result: CommandResult, context: OutcomeContext) -> CommandOutcome {
	let (problem, json_supported) = match result {
		CommandResult::Succeeded {
			stdout,
			stderr,
			mut document,
			mut warnings,
		} => {
			if !context.json {
				return text_outcome(0, stdout, stderr, false, context);
			}

			if context.diagnostic_logging_unavailable {
				warnings.push(diagnostic_logging_unavailable());
			}
			document["warnings"] = Value::Array(warnings);

			return CommandOutcome {
				status: 0,
				stdout: json_output::document(&document),
				stderr: String::new(),
				command_failed: false,
				diagnostic_log: context.diagnostic_log,
			};
		}
		CommandResult::ProgramExited { status, stderr } => {
			return text_outcome(status, String::new(), stderr, false, context);
		}
		CommandResult::Failed(report) => {
			let allowlist = match context.family {
				CommandFamily::Ordinary => Allowlist::Marker,
				CommandFamily::Execution => Allowlist::Execution,
			};
			(report_problem(&report, context.family, allowlist), true)
		}
		CommandResult::FailedAfterLaunch(report) => {
			(report_problem(&report, context.family, Allowlist::Execution), false)
		}
		CommandResult::ExportFailed { report, output } => (
			report_problem(&report, context.family, Allowlist::Export(&output)),
			true,
		),
		CommandResult::FileSelectionRequired(files) => {
			let mut text = format!("error [{FILE_SELECTION_REQUIRED}]: {FILE_SELECTION_MESSAGE}\n");
			for file in &files {
				text.push_str(&format!(
					"file_id = {}, name = {}, version = {}, category = {}\n",
					file.file_id,
					quote(&file.name),
					quote(&file.version),
					quote(&file.category)
				));
			}

			let files = files
				.iter()
				.map(|file| {
					json!({
						"file_id": file.file_id,
						"name": file.name,
						"version": file.version,
						"category": file.category,
					})
				})
				.collect::<Vec<_>>();
			let document = json_output::problem(
				FILE_SELECTION_REQUIRED,
				"Nexus file selection required",
				FILE_SELECTION_MESSAGE,
				INVALID_INVOCATION_STATUS,
				json!({"files": files}),
			);

			(
				Problem {
					status: INVALID_INVOCATION_STATUS,
					text,
					document,
				},
				true,
			)
		}
		CommandResult::HiddenExecutionUnsupported => {
			let unsupported = report!(ErrorMarker::program_unsupported()).into();
			let problem = Problem {
				text: "error [program_unsupported]: hidden managed execution is unsupported on this platform\n"
					.to_owned(),
				..report_problem(&unsupported, context.family, Allowlist::Marker)
			};
			(problem, true)
		}
		CommandResult::RootSelectionFailed(message) => {
			let problem = Problem {
				status: INVALID_INVOCATION_STATUS,
				text: format!("error: {message}\n"),
				document: json_output::problem(
					"environment_root_selection_failed",
					"Environment Root selection failed",
					message,
					INVALID_INVOCATION_STATUS,
					json!({}),
				),
			};
			(problem, true)
		}
	};

	if !(context.json && json_supported) {
		return text_outcome(problem.status, String::new(), problem.text, true, context);
	}

	let mut document = problem.document;
	if context.diagnostic_logging_unavailable {
		document["warnings"] = json!([diagnostic_logging_unavailable()]);
	}
	if let Some(session) = context.diagnostic_session {
		document["instance"] = json!(session.to_string());
	}

	CommandOutcome {
		status: problem.status,
		stdout: String::new(),
		stderr: json_output::document(&document),
		command_failed: true,
		diagnostic_log: context.diagnostic_log,
	}
}

#[cfg(any(windows, test))]
pub(crate) fn hidden_failure_dialog(outcome: &CommandOutcome) -> Option<String> {
	if !outcome.command_failed {
		return None;
	}

	let mut message = outcome.stderr.trim_end().to_owned();
	if let Some(path) = &outcome.diagnostic_log {
		message.push_str(&format!("\nDiagnostic log: {}", path.display()));
	}
	Some(message)
}

struct Problem {
	status: u32,
	text: String,
	document: Value,
}

enum Allowlist<'a> {
	Marker,
	Execution,
	Export(&'a Path),
}

#[derive(Default)]
struct ProblemDetails {
	text: String,
	values: Map<String, Value>,
}

impl ProblemDetails {
	fn quoted(&mut self, key: &str, value: impl Display) {
		let value = value.to_string();
		self.text.push_str(&format!("{key} = {}\n", quote(&value)));
		self.values.insert(key.to_owned(), json!(value));
	}

	fn unquoted(&mut self, key: &str, value: impl Display + Into<Value>) {
		self.text.push_str(&format!("{key} = {value}\n"));
		self.values.insert(key.to_owned(), value.into());
	}
}

fn report_problem(report: &Report, family: CommandFamily, allowlist: Allowlist<'_>) -> Problem {
	let marker = attachment::<ErrorMarker>(report);
	let status = match (family, marker.map(ErrorMarker::code)) {
		(_, Some(ErrorCode::OperationCancelled)) => STATUS_CONTROL_C_EXIT,
		(CommandFamily::Ordinary, Some(ErrorCode::InvalidSelection)) => INVALID_INVOCATION_STATUS,
		(CommandFamily::Ordinary, _) => 1,
		(CommandFamily::Execution, Some(ErrorCode::ProgramNotFound)) => 127,
		(
			CommandFamily::Execution,
			Some(
				ErrorCode::ProgramUnsupported
				| ErrorCode::ProgramLaunchFailed
				| ErrorCode::InvalidWorkingDirectory,
			),
		) => 126,
		(CommandFamily::Execution, _) => 125,
	};

	let mut details = ProblemDetails::default();
	if let Some(marker) = marker {
		if let Some(phase) = marker.phase() {
			details.unquoted("phase", phase);
		}
		if let Some(field) = marker.field() {
			details.quoted("field", field);
		}
		if let Some(mod_name) = marker.mod_name() {
			details.quoted("mod_name", mod_name.as_str());
		}
		if let Some(group_id) = marker.group_id() {
			details.quoted("group_id", group_id);
		}
		if let Some(option_id) = marker.option_id() {
			details.quoted("option_id", option_id);
		}
		if let Some(sequence) = marker.supplied_sequence() {
			details.unquoted("sequence", sequence);
		}
	}

	let load_order_file = attachment::<LoadOrderFile>(report);
	let retained_profile = attachment::<RetainedProfile>(report);
	let retained_export = attachment::<RetainedExport>(report);
	let completed_export = attachment::<CompletedExport>(report).is_some();

	match allowlist {
		Allowlist::Marker => {}
		Allowlist::Execution => {
			if let Some(file) = load_order_file {
				details.quoted("load_order_file", file.path.display());
			}
			if let Some(retained) = retained_profile {
				details.quoted("retained_execution_inis", retained.path.display());
				details.text.push_str(
					"Inspect retained INI edits after all managed processes have stopped. Do not discard them blindly.\n",
				);
			}
		}
		Allowlist::Export(output) => {
			if let Some(file) = load_order_file {
				details.quoted("load_order_file", file.path.display());
			}
			if let Some(retained) = retained_export {
				details.quoted("retained_partial_output", retained.path.display());
				details.text.push_str(
					"This is the partial output folder; export wrote into it directly and did not finish. Inspect it before manual cleanup or a retry.\n",
				);
				details.values
					.insert("output".to_owned(), json!(output.display().to_string()));
			} else {
				details.quoted("output", output.display());
			}
			if let Some(retained) = retained_profile {
				details.quoted("retained_export_stage", retained.path.display());
				if completed_export {
					details.text.push_str(
						"The export output is complete. Only the temp stage remains.\n",
					);
				}
				details.text.push_str(
					"The stage holds only derived profile copies. Delete it before the next exec or export.\n",
				);
			}
			if completed_export {
				details.values.insert("output_complete".to_owned(), json!(true));
			}

			let advice = match marker.map(ErrorMarker::code) {
				Some(ErrorCode::EnvironmentAlreadyInitialized) => {
					Some("Choose a new output folder; even an empty existing folder is refused.")
				}
				Some(ErrorCode::EnvironmentRootUnsafe) => Some(
					"Check that the destination is outside the Environment Root and that source paths are ordinary files.",
				),
				Some(ErrorCode::InvalidDataPath) => {
					Some("Choose a safe output folder name and inspect the source paths.")
				}
				_ => None,
			};
			if let Some(advice) = advice {
				details.text.push_str(advice);
				details.text.push('\n');
			}
		}
	}

	let (headline, document) = match marker {
		Some(marker) => {
			let code = marker.code().as_str();
			let mut title = code.replace('_', " ");
			if let Some(first) = title.get_mut(..1) {
				first.make_ascii_uppercase();
			}
			(
				format!("error [{code}]: {}\n", marker.message()),
				json_output::problem(
					code,
					&title,
					marker.message(),
					status,
					Value::Object(details.values),
				),
			)
		}
		None => (
			"error: operation failed\n".to_owned(),
			json_output::problem(
				"operation_failed",
				"Operation failed",
				"operation failed",
				status,
				Value::Object(details.values),
			),
		),
	};

	Problem {
		status,
		text: headline + &details.text,
		document,
	}
}

fn attachment<C: 'static>(report: &Report) -> Option<&C> {
	report.iter_reports()
		.find_map(|entry| entry.downcast_current_context::<C>())
}

fn text_outcome(
	status: u32,
	stdout: String,
	mut stderr: String,
	command_failed: bool,
	context: OutcomeContext,
) -> CommandOutcome {
	if context.diagnostic_logging_unavailable && (!context.quiet_success || status != 0) {
		stderr.push_str(SINK_WARNING);
		stderr.push('\n');
	}
	if status != 0
		&& let Some(session) = context.diagnostic_session
	{
		stderr.push_str(&format!("diagnostic session: {session}\n"));
	}

	CommandOutcome {
		status,
		stdout,
		stderr,
		command_failed,
		diagnostic_log: context.diagnostic_log,
	}
}

fn diagnostic_logging_unavailable() -> Value {
	json_output::warning(
		"diagnostic_logging_unavailable",
		"diagnostic session logging is unavailable",
		json!({}),
	)
}

#[cfg(test)]
mod tests {
	use super::CommandFamily;
	use super::CommandOutcome;
	use super::CommandResult;
	use super::OutcomeContext;
	use super::hidden_failure_dialog;
	use super::outcome;
	use crate::diagnostics::SINK_WARNING;
	use application::ErrorMarker;
	use application::export::CompletedExport;
	use application::export::ExportEnvironmentError;
	use application::export::RetainedExport;
	use application::installation::DownloadModFile;
	use application::ports::LoadOrderFile;
	use application::ports::RetainedProfile;
	use application::settings::ListSettingsError;
	use domain::ModName;
	use rootcause::Report;
	use rootcause::report;
	use serde_json::Value;
	use serde_json::from_str;
	use serde_json::json;
	use std::error::Error;
	use std::io::Error as IoError;
	use std::path::PathBuf;
	use uuid::Uuid;

	const PROBLEM_TYPE: &str = "https://github.com/Reilley64/mods/blob/main/docs/cli/problems.md#";

	fn context(family: CommandFamily, json: bool) -> OutcomeContext {
		OutcomeContext {
			family,
			json,
			quiet_success: false,
			diagnostic_logging_unavailable: false,
			diagnostic_session: None,
			diagnostic_log: None,
		}
	}

	fn rendered(
		family: CommandFamily,
		result: impl Fn() -> CommandResult,
	) -> Result<(CommandOutcome, CommandOutcome, Value), Box<dyn Error>> {
		let text = outcome(result(), context(family, false));
		let json = outcome(result(), context(family, true));
		let document = from_str(if json.stdout.is_empty() {
			&json.stderr
		} else {
			&json.stdout
		})?;
		Ok((text, json, document))
	}

	fn failure(marker: ErrorMarker, attachments: Vec<Report>) -> Report {
		let mut failure = report!(IoError::other("private cause")).context(marker);
		for attachment in attachments {
			failure.children_mut().push(attachment.into_cloneable());
		}
		failure.context(ExportEnvironmentError).into()
	}

	fn load_order_file() -> Report {
		report!(LoadOrderFile {
			path: PathBuf::from("Data/FalloutNV.esm")
		})
		.into()
	}

	fn retained_profile(path: &str) -> Report {
		report!(RetainedProfile {
			path: PathBuf::from(path)
		})
		.into()
	}

	fn assert_no_raw_report(outcome: &CommandOutcome) {
		for raw in [
			"private cause",
			"ExportEnvironmentError",
			"RetainedProfile",
			"LoadOrderFile",
		] {
			assert!(!outcome.stdout.contains(raw), "{raw}");
			assert!(!outcome.stderr.contains(raw), "{raw}");
		}
	}

	#[test]
	fn ordinary_failures_render_only_marker_details_in_text_and_json() -> Result<(), Box<dyn Error>> {
		let (text, json, problem) = rendered(CommandFamily::Ordinary, || {
			CommandResult::Failed(failure(
				ErrorMarker::invalid_selection(
					"choices",
					Some("group\nvalue".to_owned()),
					Some("option".to_owned()),
					Some(3),
				),
				vec![load_order_file(), retained_profile("/env/temp/inis")],
			))
		})?;

		assert_eq!(
			text,
			CommandOutcome {
				status: 2,
				stdout: String::new(),
				stderr: concat!(
					"error [invalid_selection]: FOMOD selection is invalid\n",
					"field = \"choices\"\n",
					"group_id = \"group\\nvalue\"\n",
					"option_id = \"option\"\n",
					"sequence = 3\n"
				)
				.to_owned(),
				command_failed: true,
				diagnostic_log: None,
			}
		);
		assert_eq!(json.status, 2);
		assert!(json.stdout.is_empty());
		assert_eq!(
			problem,
			json!({
				"type": format!("{PROBLEM_TYPE}invalid_selection"),
				"title": "Invalid selection",
				"detail": "FOMOD selection is invalid",
				"exit_code": 2,
				"code": "invalid_selection",
				"details": {"field": "choices", "group_id": "group\nvalue", "option_id": "option", "sequence": 3},
			})
		);
		assert_no_raw_report(&text);
		assert_no_raw_report(&json);
		Ok(())
	}

	#[test]
	fn marker_phase_and_mod_name_render_in_text_and_json() -> Result<(), Box<dyn Error>> {
		let (text, _, problem) = rendered(CommandFamily::Ordinary, || {
			let missing = ModName::new("Missing Mod".to_owned()).map_or_else(
				|_| report!(ErrorMarker::invalid_mod_name()).into(),
				|missing| {
					report!(ErrorMarker::environment_invalid(Some("profile"))
						.with_mod_name(missing))
					.context(ListSettingsError)
					.into()
				},
			);
			CommandResult::Failed(missing)
		})?;

		assert_eq!(
			text.stderr,
			"error [environment_invalid]: environment is invalid\nphase = profile\nmod_name = \"Missing Mod\"\n"
		);
		assert_eq!(
			problem["details"],
			json!({"phase": "profile", "mod_name": "Missing Mod"})
		);
		assert_eq!(problem["exit_code"], 1);
		Ok(())
	}

	#[test]
	fn a_report_without_a_marker_is_a_generic_operation_failure() -> Result<(), Box<dyn Error>> {
		for (family, status) in [(CommandFamily::Ordinary, 1), (CommandFamily::Execution, 125)] {
			let (text, json, problem) = rendered(family, || {
				CommandResult::Failed(report!(IoError::other("private cause")).into())
			})?;

			assert_eq!(text.status, status);
			assert_eq!(text.stderr, "error: operation failed\n");
			assert_eq!(json.status, status);
			assert_eq!(
				problem,
				json!({
					"type": format!("{PROBLEM_TYPE}operation_failed"),
					"title": "Operation failed",
					"detail": "operation failed",
					"exit_code": status,
					"code": "operation_failed",
				})
			);
		}
		Ok(())
	}

	#[test]
	fn each_family_maps_markers_to_its_exit_status() -> Result<(), Box<dyn Error>> {
		for (family, marker, status) in [
			(CommandFamily::Ordinary, ErrorMarker::operation_cancelled(), 0xC000_013A),
			(
				CommandFamily::Ordinary,
				ErrorMarker::invalid_selection("choices", None, None, None),
				2,
			),
			(CommandFamily::Ordinary, ErrorMarker::io_failure(), 1),
			(CommandFamily::Ordinary, ErrorMarker::program_not_found(), 1),
			(
				CommandFamily::Execution,
				ErrorMarker::operation_cancelled(),
				0xC000_013A,
			),
			(CommandFamily::Execution, ErrorMarker::program_not_found(), 127),
			(CommandFamily::Execution, ErrorMarker::program_unsupported(), 126),
			(CommandFamily::Execution, ErrorMarker::program_launch_failed(), 126),
			(CommandFamily::Execution, ErrorMarker::invalid_working_directory(), 126),
			(CommandFamily::Execution, ErrorMarker::vfs_failed(), 125),
			(
				CommandFamily::Execution,
				ErrorMarker::execution_supervision_failed(),
				125,
			),
			(CommandFamily::Execution, ErrorMarker::environment_invalid(None), 125),
			(
				CommandFamily::Execution,
				ErrorMarker::invalid_selection("choices", None, None, None),
				125,
			),
		] {
			let (text, json, problem) =
				rendered(family, || CommandResult::Failed(report!(marker.clone()).into()))?;

			assert_eq!(text.status, status, "{family:?} {:?}", marker.code());
			assert_eq!(json.status, status, "{family:?} {:?}", marker.code());
			assert_eq!(problem["exit_code"], status, "{family:?} {:?}", marker.code());
		}
		Ok(())
	}

	#[test]
	fn execution_failures_name_the_load_order_file_and_retained_inis() -> Result<(), Box<dyn Error>> {
		let execution_failure = || {
			failure(
				ErrorMarker::execution_supervision_failed().with_phase("profile_retained"),
				vec![load_order_file(), retained_profile("/env/temp/exec-inis")],
			)
		};
		let expected_text = concat!(
			"error [execution_supervision_failed]: process supervision failed\n",
			"phase = profile_retained\n",
			"load_order_file = \"Data/FalloutNV.esm\"\n",
			"retained_execution_inis = \"/env/temp/exec-inis\"\n",
			"Inspect retained INI edits after all managed processes have stopped. Do not discard them blindly.\n",
		);

		let (text, json, problem) =
			rendered(CommandFamily::Execution, || CommandResult::Failed(execution_failure()))?;

		assert_eq!(text.status, 125);
		assert_eq!(text.stderr, expected_text);
		assert!(text.command_failed);
		assert_eq!(json.status, 125);
		assert_eq!(problem["code"], "execution_supervision_failed");
		assert_eq!(
			problem["details"],
			json!({
				"phase": "profile_retained",
				"load_order_file": "Data/FalloutNV.esm",
				"retained_execution_inis": "/env/temp/exec-inis",
			})
		);
		assert_no_raw_report(&text);
		assert_no_raw_report(&json);

		let after_launch = outcome(
			CommandResult::FailedAfterLaunch(execution_failure()),
			context(CommandFamily::Execution, true),
		);
		assert_eq!(after_launch, text);
		Ok(())
	}

	#[test]
	fn export_failures_name_the_partial_output_instead_of_the_output() -> Result<(), Box<dyn Error>> {
		let (text, json, problem) = rendered(CommandFamily::Ordinary, || CommandResult::ExportFailed {
			report: failure(
				ErrorMarker::io_failure(),
				vec![
					load_order_file(),
					report!(RetainedExport {
						path: PathBuf::from("/output")
					})
					.into(),
				],
			),
			output: PathBuf::from("/output"),
		})?;

		assert_eq!(text.status, 1);
		assert_eq!(
			text.stderr,
			concat!(
				"error [io_failure]: input/output operation failed\n",
				"load_order_file = \"Data/FalloutNV.esm\"\n",
				"retained_partial_output = \"/output\"\n",
				"This is the partial output folder; export wrote into it directly and did not finish. Inspect it before manual cleanup or a retry.\n",
			)
		);
		assert_eq!(json.status, 1);
		assert_eq!(
			problem["details"],
			json!({
				"load_order_file": "Data/FalloutNV.esm",
				"retained_partial_output": "/output",
				"output": "/output",
			})
		);
		assert_no_raw_report(&text);
		assert_no_raw_report(&json);
		Ok(())
	}

	#[test]
	fn export_failures_name_a_retained_stage_and_whether_the_output_is_complete() -> Result<(), Box<dyn Error>> {
		for completed in [false, true] {
			let (text, _, problem) = rendered(CommandFamily::Ordinary, || {
				let mut attachments = vec![retained_profile("/env/temp/export-inis-1")];
				if completed {
					attachments.push(report!(CompletedExport {
						path: PathBuf::from("/output")
					})
					.into());
				}
				CommandResult::ExportFailed {
					report: failure(ErrorMarker::io_failure(), attachments),
					output: PathBuf::from("/output"),
				}
			})?;

			let completion = if completed {
				"The export output is complete. Only the temp stage remains.\n"
			} else {
				""
			};
			assert_eq!(
				text.stderr,
				format!(
					"error [io_failure]: input/output operation failed\noutput = \"/output\"\nretained_export_stage = \"/env/temp/export-inis-1\"\n{completion}The stage holds only derived profile copies. Delete it before the next exec or export.\n"
				)
			);
			let mut details = json!({
				"output": "/output",
				"retained_export_stage": "/env/temp/export-inis-1",
			});
			if completed {
				details["output_complete"] = json!(true);
			}
			assert_eq!(problem["details"], details);
		}
		Ok(())
	}

	#[test]
	fn export_failures_add_a_safe_next_step_for_known_markers() -> Result<(), Box<dyn Error>> {
		for (marker, advice) in [
			(
				ErrorMarker::environment_already_initialized(),
				"Choose a new output folder; even an empty existing folder is refused.\n",
			),
			(
				ErrorMarker::environment_root_unsafe(),
				"Check that the destination is outside the Environment Root and that source paths are ordinary files.\n",
			),
			(
				ErrorMarker::invalid_data_path(),
				"Choose a safe output folder name and inspect the source paths.\n",
			),
			(ErrorMarker::io_failure(), ""),
		] {
			let (text, _, problem) = rendered(CommandFamily::Ordinary, || CommandResult::ExportFailed {
				report: report!(marker.clone()).into(),
				output: PathBuf::from("/existing"),
			})?;

			assert_eq!(
				text.stderr,
				format!(
					"error [{}]: {}\noutput = \"/existing\"\n{advice}",
					marker.code().as_str(),
					marker.message()
				)
			);
			assert_eq!(problem["details"], json!({"output": "/existing"}));
		}
		Ok(())
	}

	#[test]
	fn nexus_file_selection_lists_files_in_published_order() -> Result<(), Box<dyn Error>> {
		let (text, json, problem) = rendered(CommandFamily::Ordinary, || {
			CommandResult::FileSelectionRequired(vec![
				DownloadModFile {
					file_id: 9,
					name: "Optional".into(),
					version: "2.0".into(),
					category: "OPTIONAL".into(),
				},
				DownloadModFile {
					file_id: 7,
					name: "Main \"Files\"".into(),
					version: "1.0".into(),
					category: "MAIN".into(),
				},
			])
		})?;

		assert_eq!(text.status, 2);
		assert_eq!(
			text.stderr,
			concat!(
				"error [nexus_file_selection_required]: Select a file with --file <id>.\n",
				"file_id = 9, name = \"Optional\", version = \"2.0\", category = \"OPTIONAL\"\n",
				"file_id = 7, name = \"Main \\\"Files\\\"\", version = \"1.0\", category = \"MAIN\"\n",
			)
		);
		assert_eq!(json.status, 2);
		assert_eq!(
			problem,
			json!({
				"type": format!("{PROBLEM_TYPE}nexus_file_selection_required"),
				"title": "Nexus file selection required",
				"detail": "Select a file with --file <id>.",
				"exit_code": 2,
				"code": "nexus_file_selection_required",
				"details": {"files": [
					{"file_id": 9, "name": "Optional", "version": "2.0", "category": "OPTIONAL"},
					{"file_id": 7, "name": "Main \"Files\"", "version": "1.0", "category": "MAIN"},
				]},
			})
		);
		Ok(())
	}

	#[test]
	fn hidden_execution_is_an_unsupported_program() -> Result<(), Box<dyn Error>> {
		let (text, _, problem) =
			rendered(CommandFamily::Execution, || CommandResult::HiddenExecutionUnsupported)?;

		assert_eq!(text.status, 126);
		assert_eq!(
			text.stderr,
			"error [program_unsupported]: hidden managed execution is unsupported on this platform\n"
		);
		assert!(text.command_failed);
		assert_eq!(problem["code"], "program_unsupported");
		assert_eq!(problem["detail"], "program is unsupported");
		assert_eq!(problem["exit_code"], 126);
		Ok(())
	}

	#[test]
	fn root_selection_failures_are_usage_errors() -> Result<(), Box<dyn Error>> {
		for family in [CommandFamily::Ordinary, CommandFamily::Execution] {
			let (text, json, problem) = rendered(family, || {
				CommandResult::RootSelectionFailed("environment root must be an absolute path")
			})?;

			assert_eq!(text.status, 2);
			assert_eq!(text.stderr, "error: environment root must be an absolute path\n");
			assert_eq!(json.status, 2);
			assert_eq!(
				problem,
				json!({
					"type": format!("{PROBLEM_TYPE}environment_root_selection_failed"),
					"title": "Environment Root selection failed",
					"detail": "environment root must be an absolute path",
					"exit_code": 2,
					"code": "environment_root_selection_failed",
				})
			);
		}
		Ok(())
	}

	#[test]
	fn successes_publish_text_or_one_json_document() -> Result<(), Box<dyn Error>> {
		let (text, json, document) = rendered(CommandFamily::Ordinary, || CommandResult::Succeeded {
			stdout: "key = 1\n".to_owned(),
			stderr: "warning [code]: message\n".to_owned(),
			document: json!({"key": 1}),
			warnings: vec![json!({"code": "code", "message": "message", "details": {}})],
		})?;

		assert_eq!(
			text,
			CommandOutcome {
				status: 0,
				stdout: "key = 1\n".to_owned(),
				stderr: "warning [code]: message\n".to_owned(),
				command_failed: false,
				diagnostic_log: None,
			}
		);
		assert!(json.stderr.is_empty());
		assert_eq!(json.status, 0);
		assert_eq!(
			document,
			json!({"key": 1, "warnings": [{"code": "code", "message": "message", "details": {}}]})
		);
		Ok(())
	}

	#[test]
	fn program_exit_statuses_stay_text_even_when_json_is_requested() {
		let exited = outcome(
			CommandResult::ProgramExited {
				status: 3,
				stderr: "warning [profile_state_invalid]: message\n".to_owned(),
			},
			context(CommandFamily::Execution, true),
		);

		assert_eq!(
			exited,
			CommandOutcome {
				status: 3,
				stdout: String::new(),
				stderr: "warning [profile_state_invalid]: message\n".to_owned(),
				command_failed: false,
				diagnostic_log: None,
			}
		);
	}

	#[test]
	fn diagnostic_session_decoration_follows_the_output_mode() -> Result<(), Box<dyn Error>> {
		let session = Uuid::new_v4();
		let decorated = |json: bool, quiet_success: bool| OutcomeContext {
			family: CommandFamily::Execution,
			json,
			quiet_success,
			diagnostic_logging_unavailable: true,
			diagnostic_session: Some(session),
			diagnostic_log: Some(PathBuf::from("logs/session.jsonl")),
		};
		let succeeded = || CommandResult::Succeeded {
			stdout: String::new(),
			stderr: String::new(),
			document: json!({}),
			warnings: vec![json!({"code": "first", "message": "first", "details": {}})],
		};
		let failed = || CommandResult::Failed(report!(ErrorMarker::vfs_failed()).into());
		let exited = |status| CommandResult::ProgramExited {
			status,
			stderr: String::new(),
		};
		let sink_warning = format!("{SINK_WARNING}\n");
		let session_line = format!("diagnostic session: {session}\n");

		assert_eq!(outcome(succeeded(), decorated(false, false)).stderr, sink_warning);
		assert_eq!(outcome(succeeded(), decorated(false, true)).stderr, "");
		assert_eq!(
			outcome(failed(), decorated(false, true)).stderr,
			format!("error [vfs_failed]: Virtual Game View setup failed\n{sink_warning}{session_line}")
		);
		assert_eq!(outcome(exited(0), decorated(true, false)).stderr, sink_warning);
		assert_eq!(
			outcome(exited(4), decorated(true, true)).stderr,
			format!("{sink_warning}{session_line}")
		);

		let success = outcome(succeeded(), decorated(true, false));
		assert_eq!(
			from_str::<Value>(&success.stdout)?["warnings"],
			json!([
				{"code": "first", "message": "first", "details": {}},
				{"code": "diagnostic_logging_unavailable", "message": "diagnostic session logging is unavailable", "details": {}},
			])
		);
		assert_eq!(success.diagnostic_log, Some(PathBuf::from("logs/session.jsonl")));

		let failure = outcome(failed(), decorated(true, false));
		let problem: Value = from_str(&failure.stderr)?;
		assert!(failure.stdout.is_empty());
		assert_eq!(
			problem["warnings"],
			json!([{"code": "diagnostic_logging_unavailable", "message": "diagnostic session logging is unavailable", "details": {}}])
		);
		assert_eq!(problem["instance"], session.to_string());
		assert_eq!(problem["detail"], "Virtual Game View setup failed");
		assert_eq!(failure.diagnostic_log, Some(PathBuf::from("logs/session.jsonl")));
		Ok(())
	}

	#[test]
	fn hidden_dialog_reports_failures_but_not_child_exit_codes() {
		let diagnosed = || OutcomeContext {
			diagnostic_log: Some(PathBuf::from("logs/session.jsonl")),
			..context(CommandFamily::Execution, false)
		};

		let failure = outcome(
			CommandResult::Failed(report!(ErrorMarker::program_launch_failed()).into()),
			diagnosed(),
		);
		let child_exit = outcome(
			CommandResult::ProgramExited {
				status: 126,
				stderr: String::new(),
			},
			diagnosed(),
		);

		assert_eq!(
			hidden_failure_dialog(&failure).as_deref(),
			Some(
				"error [program_launch_failed]: program could not be launched\nDiagnostic log: logs/session.jsonl"
			)
		);
		assert!(hidden_failure_dialog(&child_exit).is_none());
	}

	#[test]
	fn every_problem_detail_is_documented() -> Result<(), Box<dyn Error>> {
		let problems = include_str!("../../../../docs/cli/problems.md").replace("\r\n", "\n");
		let marker = ErrorMarker::invalid_selection(
			"choices",
			Some("group".to_owned()),
			Some("option".to_owned()),
			Some(1),
		)
		.with_phase("profile")
		.with_mod_name(ModName::new("Mod".into()).map_err(|_| "mod name fixture")?);
		let attachments = || {
			vec![
				load_order_file(),
				retained_profile("stage"),
				report!(RetainedExport { path: "output".into() }).into(),
				report!(CompletedExport { path: "output".into() }).into(),
			]
		};
		let results = [
			(
				CommandFamily::Execution,
				CommandResult::Failed(failure(marker.clone(), attachments())),
			),
			(
				CommandFamily::Ordinary,
				CommandResult::ExportFailed {
					report: failure(marker, attachments()),
					output: "output".into(),
				},
			),
			(
				CommandFamily::Ordinary,
				CommandResult::FileSelectionRequired(Vec::new()),
			),
		];

		let mut keys = Vec::new();
		for (family, result) in results {
			let problem: Value = from_str(&outcome(result, context(family, true)).stderr)?;
			keys.extend(problem["details"]
				.as_object()
				.into_iter()
				.flat_map(|details| details.keys().cloned()));
		}

		assert!(keys.len() > 10);
		for key in keys {
			assert!(
				problems.contains(&format!("- `{key}`")) || problems.contains(&format!(", `{key}`")),
				"{key}"
			);
		}
		Ok(())
	}
}
