use application::ErrorMarker;
use rootcause::Result;
use rootcause::report;
use tokio_util::sync::CancellationToken;

pub(crate) fn check_cancelled(cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		Err(report!(ErrorMarker::operation_cancelled()))
	} else {
		Ok(())
	}
}
