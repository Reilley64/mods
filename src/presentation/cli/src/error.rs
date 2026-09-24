use crate::output::quote;
use application::ErrorCode;
use application::ErrorMarker;
use rootcause::Report;

pub(crate) fn exit_status(code: ErrorCode) -> u32 {
	match code {
		ErrorCode::OperationCancelled => 0xC000_013A,
		ErrorCode::InvalidSelection => 2,
		_ => 1,
	}
}

pub(crate) fn execution_exit_status(code: ErrorCode) -> u32 {
	match code {
		ErrorCode::OperationCancelled => 0xC000_013A,
		ErrorCode::ProgramNotFound => 127,
		ErrorCode::ProgramUnsupported | ErrorCode::ProgramLaunchFailed | ErrorCode::InvalidWorkingDirectory => {
			126
		}
		_ => 125,
	}
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
	if let Some(group_id) = marker.group_id() {
		text.push_str(&format!("\ngroup_id = {}", quote(group_id)));
	}
	if let Some(option_id) = marker.option_id() {
		text.push_str(&format!("\noption_id = {}", quote(option_id)));
	}
	if let Some(sequence) = marker.supplied_sequence() {
		text.push_str(&format!("\nsequence = {sequence}"));
	}
	if let Some((expected, actual)) = marker.build_ids() {
		text.push_str(&format!("\nexpected-build-id = {expected}\nactual-build-id = {actual}"));
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
	use application::settings::ListSettingsError;
	use rootcause::report;

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
	fn manual_cleanup_required_renders_only_the_safe_code_and_message() {
		let error_marker = ErrorMarker::manual_cleanup_required();

		assert_eq!(
			marker(&error_marker),
			"error [manual_cleanup_required]: unfinished operation requires manual cleanup\n"
		);
	}
}
