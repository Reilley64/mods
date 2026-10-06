use crate::output::quote;
use application::ErrorCode;
use application::ErrorMarker;
use application::export::CompletedExport;
use application::export::RetainedExport;
use application::ports::LoadOrderFile;
use application::ports::RetainedProfile;
use rootcause::Report;
use std::path::Path;

pub(crate) const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;

pub(crate) fn exit_status(code: ErrorCode) -> u32 {
	match code {
		ErrorCode::OperationCancelled => STATUS_CONTROL_C_EXIT,
		ErrorCode::InvalidSelection => 2,
		_ => 1,
	}
}

pub(crate) fn execution_exit_status(code: ErrorCode) -> u32 {
	match code {
		ErrorCode::OperationCancelled => STATUS_CONTROL_C_EXIT,
		ErrorCode::ProgramNotFound => 127,
		ErrorCode::ProgramUnsupported | ErrorCode::ProgramLaunchFailed | ErrorCode::InvalidWorkingDirectory => {
			126
		}
		_ => 125,
	}
}

/// Names the plugin or archive whose load-order time could not be read or set.
fn load_order_file<C>(report: &Report<C>) -> String {
	report.iter_reports()
		.find_map(|entry| entry.downcast_current_context::<LoadOrderFile>())
		.map(|file| format!("load_order_file = {}\n", quote(&file.path.display().to_string())))
		.unwrap_or_default()
}

pub(crate) fn execution_error<C>(report: &Report<C>) -> String {
	let mut text = application_error(report);
	text.push_str(&load_order_file(report));

	if let Some(retained) = report
		.iter_reports()
		.find_map(|entry| entry.downcast_current_context::<RetainedProfile>())
	{
		text.push_str(&format!(
			"retained_execution_inis = {}\n",
			quote(&retained.path.display().to_string())
		));
		text.push_str(
			"Inspect retained INI edits after all managed processes have stopped. Do not discard them blindly.\n",
		);
	}

	text
}

pub(crate) fn export_error<C>(report: &Report<C>, output: &Path) -> String {
	let mut text = application_error(report);
	text.push_str(&load_order_file(report));

	if let Some(retained) = report
		.iter_reports()
		.find_map(|entry| entry.downcast_current_context::<RetainedExport>())
	{
		text.push_str(&format!(
			"retained_partial_output = {}\n",
			quote(&retained.path.display().to_string())
		));
		text.push_str(
			"This is the partial output folder; export wrote into it directly and did not finish. Inspect it before manual cleanup or a retry.\n",
		);
	} else {
		text.push_str(&format!("output = {}\n", quote(&output.display().to_string())));
	}

	if let Some(retained) = report
		.iter_reports()
		.find_map(|entry| entry.downcast_current_context::<RetainedProfile>())
	{
		text.push_str(&format!(
			"retained_export_stage = {}\n",
			quote(&retained.path.display().to_string())
		));
		let completed = report
			.iter_reports()
			.any(|entry| entry.downcast_current_context::<CompletedExport>().is_some());
		if completed {
			text.push_str("The export output is complete. Only the temp stage remains.\n");
		}
		text.push_str(
			"The stage holds only derived profile copies. Delete it before the next exec or export.\n",
		);
	}

	let advice = match application_marker(report).map(ErrorMarker::code) {
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
		text.push_str(advice);
		text.push('\n');
	}

	text
}

pub(crate) fn application_error<C>(report: &Report<C>) -> String {
	application_marker(report).map_or_else(|| "error: operation failed\n".to_owned(), marker)
}

pub(crate) fn application_marker<C>(report: &Report<C>) -> Option<&ErrorMarker> {
	report.iter_reports()
		.find_map(|report| report.downcast_current_context::<ErrorMarker>())
}

pub(crate) fn marker(marker: &ErrorMarker) -> String {
	let mut text = format!("error [{}]: {}", marker.code().as_str(), marker.message());
	if let Some(phase) = marker.phase() {
		text.push_str(&format!("\nphase = {phase}"));
	}
	if let Some(field) = marker.field() {
		text.push_str(&format!("\nfield = {}", quote(field)));
	}
	if let Some(mod_name) = marker.mod_name() {
		text.push_str(&format!("\nmod_name = {}", quote(mod_name.as_str())));
	}
	if let Some(group_id) = marker.group_id() {
		text.push_str(&format!("\ngroup_id = {}", quote(group_id)));
	}
	if let Some(option_id) = marker.option_id() {
		text.push_str(&format!("\noption_id = {}", quote(option_id)));
	}
	if let Some(sequence) = marker.supplied_sequence() {
		text.push_str(&format!("\nsequence = {sequence}"));
	}

	text.push('\n');
	text
}

#[cfg(test)]
mod tests {
	use super::application_error;
	use super::application_marker;
	use super::execution_exit_status;
	use super::marker;
	use application::ErrorCode;
	use application::ErrorMarker;
	use application::execution::ExecuteProgramError;
	use application::ports::LoadOrderFile;
	use application::ports::RetainedProfile;
	use application::settings::ListSettingsError;
	use domain::ModName;
	use rootcause::report;
	use std::error::Error;
	use std::path::Path;
	use std::path::PathBuf;

	#[test]
	fn execution_error_exposes_only_typed_retained_ini_path() {
		let mut report = report!(ErrorMarker::execution_supervision_failed().with_phase("profile_retained"))
			.context(ExecuteProgramError);
		report.children_mut().push(report!(RetainedProfile {
			path: PathBuf::from("C:\\private\\inis\\line\n")
		})
		.into_dynamic()
		.into_cloneable());

		let text = super::execution_error(&report);
		assert!(text.contains("error [execution_supervision_failed]"));
		assert!(text.contains("phase = profile_retained"));
		assert!(text.contains("retained_execution_inis = \"C:\\\\private\\\\inis\\\\line\\n\""));
		assert!(text.contains("after all managed processes have stopped"));
		assert!(!text.contains("ExecuteProgramError"));
		assert!(!text.contains("RetainedProfile"));
	}

	#[test]
	fn load_order_errors_name_the_failing_file() {
		let mut report =
			report!(ErrorMarker::io_failure().with_phase("load_order")).context(ExecuteProgramError);
		report.children_mut().push(report!(LoadOrderFile {
			path: PathBuf::from("C:\\Game\\Data\\FalloutNV.esm")
		})
		.into_dynamic()
		.into_cloneable());

		let text = super::execution_error(&report);
		assert!(text.contains("phase = load_order"));
		assert!(text.contains("load_order_file = \"C:\\\\Game\\\\Data\\\\FalloutNV.esm\"\n"));
		assert!(super::export_error(&report, Path::new("C:\\output")).contains("load_order_file = "));
	}

	#[test]
	fn export_error_allowlists_retained_stage_and_never_formats_the_report_tree() {
		let report = report!(ErrorMarker::io_failure()).context(application::export::ExportEnvironmentError);
		let mut report = report;
		report.children_mut().push(report!(application::export::RetainedExport {
			path: PathBuf::from("C:\\private\\partial")
		})
		.into_dynamic()
		.into_cloneable());
		let text = super::export_error(&report, Path::new("C:\\private\\output"));
		assert!(text.contains("error [io_failure]"));
		assert!(text.contains("retained_partial_output = \"C:\\\\private\\\\partial\""));
		assert!(!text.contains("ExportEnvironmentError"));
		assert!(!text.lines().any(|line| line.starts_with("output =")));
	}

	#[test]
	fn export_error_names_a_retained_stage_with_export_advice() {
		let mut report =
			report!(ErrorMarker::io_failure()).context(application::export::ExportEnvironmentError);
		report.children_mut().push(report!(RetainedProfile {
			path: PathBuf::from("C:\\env\\temp\\export-inis-1")
		})
		.into_dynamic()
		.into_cloneable());

		let text = super::export_error(&report, Path::new("C:\\private\\output"));

		assert!(text.contains("retained_export_stage = \"C:\\\\env\\\\temp\\\\export-inis-1\""));
		assert!(text.contains("Delete it before the next exec or export"));
		assert!(!text.contains("The export output is complete"));
		assert!(!text.contains("retained_execution_inis"));
		assert!(!text.contains("RetainedProfile"));
	}

	#[test]
	fn export_error_says_a_complete_output_when_only_the_stage_remains() {
		let mut report =
			report!(ErrorMarker::io_failure()).context(application::export::ExportEnvironmentError);
		for child in [
			report!(RetainedProfile {
				path: PathBuf::from("/env/temp/export-inis-1")
			})
			.into_dynamic()
			.into_cloneable(),
			report!(application::export::CompletedExport {
				path: PathBuf::from("/output")
			})
			.into_dynamic()
			.into_cloneable(),
		] {
			report.children_mut().push(child);
		}

		let text = super::export_error(&report, Path::new("/output"));

		assert!(text.contains("retained_export_stage = \"/env/temp/export-inis-1\""));
		assert!(text.contains("The export output is complete. Only the temp stage remains."));
		assert!(!text.contains("partial output"));
	}

	#[test]
	fn existing_export_destination_has_specific_safe_next_step() {
		let report = report!(ErrorMarker::environment_already_initialized())
			.context(application::export::ExportEnvironmentError);
		let text = super::export_error(&report, Path::new("/existing"));
		assert!(text.contains("output = \"/existing\""));
		assert!(text.contains("Choose a new output folder"));
	}

	#[test]
	fn execution_statuses_distinguish_lookup_launch_management_and_cancellation() {
		assert_eq!(execution_exit_status(ErrorCode::ProgramNotFound), 127);
		assert_eq!(execution_exit_status(ErrorCode::ProgramUnsupported), 126);
		assert_eq!(execution_exit_status(ErrorCode::ProgramLaunchFailed), 126);
		assert_eq!(execution_exit_status(ErrorCode::VfsFailed), 125);
		assert_eq!(execution_exit_status(ErrorCode::ExecutionSupervisionFailed), 125);
		assert_eq!(execution_exit_status(ErrorCode::EnvironmentInvalid), 125);
		assert_eq!(execution_exit_status(ErrorCode::OperationCancelled), 0xC000_013A);
	}

	#[test]
	fn application_error_uses_the_safe_marker_below_the_use_case_context() {
		let report = report!(ErrorMarker::environment_invalid(Some("read"))).context(ListSettingsError);

		assert_eq!(
			application_marker(&report).map(ErrorMarker::code),
			Some(ErrorCode::EnvironmentInvalid)
		);
		assert_eq!(
			application_error(&report),
			"error [environment_invalid]: environment is invalid\nphase = read\n"
		);
	}

	#[test]
	fn invalid_selection_renders_only_allowlisted_choice_details() {
		let report = report!(ErrorMarker::invalid_selection(
			"choices",
			Some("group\nvalue".to_owned()),
			Some("option".to_owned()),
			Some(3),
		))
		.context(ListSettingsError);

		assert_eq!(
			application_error(&report),
			concat!(
				"error [invalid_selection]: FOMOD selection is invalid\n",
				"field = \"choices\"\n",
				"group_id = \"group\\nvalue\"\n",
				"option_id = \"option\"\n",
				"sequence = 3\n"
			)
		);
	}

	#[test]
	fn a_marker_with_a_mod_name_renders_the_name() -> Result<(), Box<dyn Error>> {
		let missing = ModName::new("Missing Mod".to_owned()).map_err(|_| "valid test mod name")?;
		let report = report!(ErrorMarker::environment_invalid(None).with_mod_name(missing))
			.context(ListSettingsError);

		assert_eq!(
			application_error(&report),
			"error [environment_invalid]: environment is invalid\nmod_name = \"Missing Mod\"\n"
		);
		Ok(())
	}

	#[test]
	fn manual_cleanup_required_renders_only_the_safe_code_and_message() {
		let error_marker = ErrorMarker::manual_cleanup_required();

		assert_eq!(
			marker(&error_marker),
			"error [manual_cleanup_required]: unfinished operation requires manual cleanup\n"
		);
	}
}
