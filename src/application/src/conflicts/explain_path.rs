use super::projection::project_path;
use crate::errors::ErrorMarker;
use crate::ports::ReadConflictContent;
use crate::ports::ScanEnvironmentConflicts;
use domain::ConflictProblem;
use domain::ContentComparison;
use domain::DataRelativePath;
use domain::EffectiveResult;
use domain::ProviderReference;
use domain::ResolutionReason;
use domain::ResolutionStatus;
use domain::TombstoneEffect;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ExplainPathDependencies {
	pub scan_environment: ScanEnvironmentConflicts,
	pub read_conflict_content: ReadConflictContent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainPathOutput {
	pub normalized_key: String,
	pub display_path: DataRelativePath,
	pub resolution_status: ResolutionStatus,
	pub effective_result: Option<EffectiveResult>,
	pub provider_stack: Vec<ProviderReference>,
	pub tombstone_effects: Vec<TombstoneEffect>,
	pub content_comparisons: Vec<ContentComparison>,
	pub reasons: Vec<ResolutionReason>,
	pub problems: Vec<ConflictProblem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExplainPathError;

impl fmt::Display for ExplainPathError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to explain path")
	}
}

#[tracing::instrument(skip_all)]
pub async fn explain_path(
	dependencies: ExplainPathDependencies,
	path: DataRelativePath,
	compare_content: bool,
	cancellation: CancellationToken,
) -> Result<ExplainPathOutput, ExplainPathError> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(ExplainPathError));
	}

	let scan = dependencies
		.scan_environment
		.call((cancellation.clone(),))
		.await
		.context(ExplainPathError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(ExplainPathError));
	}

	project_path(
		scan,
		path,
		compare_content,
		dependencies.read_conflict_content,
		cancellation,
	)
	.await
	.context(ExplainPathError)
}
