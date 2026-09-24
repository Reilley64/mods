use super::projection::project_inspection;
use crate::ports::ProgressEvent;
use crate::ports::ReadConflictContent;
use crate::ports::ReportProgress;
use crate::ports::ScanEnvironmentConflicts;
use domain::ConflictProblem;
use domain::ConflictRow;
use domain::ModName;
use domain::Participation;
use domain::ProviderSummary;
use domain::ResolutionStatus;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct InspectModConflictsDependencies {
	pub report_progress: Option<ReportProgress>,
	pub scan_environment: ScanEnvironmentConflicts,
	pub read_conflict_content: ReadConflictContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectModConflictsOutput {
	pub mod_name: ModName,
	pub participation: Participation,
	pub resolution_status: ResolutionStatus,
	pub provider_summary: ProviderSummary,
	pub rows: Vec<ConflictRow>,
	pub problems: Vec<ConflictProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InspectModConflictsError;

impl fmt::Display for InspectModConflictsError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to inspect mod conflicts")
	}
}

#[tracing::instrument(skip_all)]
pub async fn inspect_mod_conflicts(
	dependencies: InspectModConflictsDependencies,
	mod_name: ModName,
	compare_content: bool,
	cancellation: CancellationToken,
) -> Result<InspectModConflictsOutput, InspectModConflictsError> {
	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ScanningConflicts,)).await;
	}

	let scan = dependencies
		.scan_environment
		.call((cancellation.clone(),))
		.await
		.context(InspectModConflictsError)?;

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ConflictsScanned,)).await;
	}

	project_inspection(
		scan,
		mod_name,
		compare_content,
		dependencies.read_conflict_content,
		cancellation,
	)
	.await
	.context(InspectModConflictsError)
}
