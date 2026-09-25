use super::projection::project_list;
use crate::ports::ProgressEvent;
use crate::ports::ReadConflictContent;
use crate::ports::ReportProgress;
use crate::ports::ScanEnvironmentConflicts;
use domain::ConflictProblem;
use domain::ConflictRow;
use domain::ResolutionStatus;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ListEffectiveConflictsDependencies {
	pub report_progress: Option<ReportProgress>,
	pub scan_environment: ScanEnvironmentConflicts,
	pub read_conflict_content: ReadConflictContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEffectiveConflictsOutput {
	pub resolution_status: ResolutionStatus,
	pub rows: Vec<ConflictRow>,
	pub problems: Vec<ConflictProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListEffectiveConflictsError;

impl fmt::Display for ListEffectiveConflictsError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to list effective conflicts")
	}
}

#[tracing::instrument(skip_all)]
pub async fn list_effective_conflicts(
	dependencies: ListEffectiveConflictsDependencies,
	compare_content: bool,
	cancellation: CancellationToken,
) -> Result<ListEffectiveConflictsOutput, ListEffectiveConflictsError> {
	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ScanningConflicts,)).await;
	}

	let scan = dependencies
		.scan_environment
		.call((cancellation.clone(),))
		.await
		.context(ListEffectiveConflictsError)?;

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ConflictsScanned,)).await;
	}

	project_list(scan, compare_content, dependencies.read_conflict_content, cancellation)
		.await
		.context(ListEffectiveConflictsError)
}
