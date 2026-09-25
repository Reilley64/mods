use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Reporting is observational and does not control cancellation or application success.
pub type ReportProgress = Arc<dyn Fn(ProgressEvent) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEvent {
	SettingsLoaded,
	ValidatingGameBinding,
	GameBindingStored,
	ScanningArchive,
	ArchiveIndexed,
	ArchiveScanCheckpoint,
	EvaluatingInstaller,
	InstallationPlanned,
	ScanningConflicts,
	ConflictsScanned,
	ExtractingFiles,
	FilesExtracted,
	ExtractionCheckpoint,
	InstallationPublished,
	PreparingExecution,
	ExecutionPrepared,
	ExecutionFinished,
}
