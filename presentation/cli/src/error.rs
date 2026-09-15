use application::ErrorMarker;
use rootcause::Report;

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
	if let Some((expected, actual)) = marker.build_ids() {
		text.push_str(&format!("\nexpected-build-id = {expected}\nactual-build-id = {actual}"));
	}
	text.push('\n');
	text
}

#[cfg(test)]
mod tests {
	use super::*;
	use application::ErrorCode;
	use application::settings::ListSettingsError;
	use rootcause::report;

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
}
