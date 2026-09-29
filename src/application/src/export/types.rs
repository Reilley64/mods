use crate::ports::AdapterState;
use crate::ports::EnvironmentPlan;
use crate::ports::PortFuture;
use crate::ports::StagedProfile;
use domain::DataRelativePath;
use domain::ProviderIdentity;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportProvider {
	Data(ProviderIdentity),
	Profile,
	GeneratedInvalidation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportFile {
	pub source_id: usize,
	pub path: DataRelativePath,
	pub provider: ExportProvider,
	pub bytes: u64,
}

/// The source of each listed file, indexed by [`ExportFile::source_id`].
pub struct ExportSources(pub AdapterState);

pub struct ExportListing {
	pub files: Vec<ExportFile>,
	pub sources: ExportSources,
}

/// A failed export retains this partial output directory. Presentation may
/// expose this typed path, never raw report formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedExport {
	pub path: PathBuf,
}
impl fmt::Display for RetainedExport {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("partial export output retained")
	}
}

/// Requires a new output directory outside the environment.
pub type ValidateExportDestination = Arc<dyn Fn(PathBuf) -> PortFuture<()> + Send + Sync>;
/// Lists every file the game sees, except the game's own Data files, with its size.
pub type ListExportFiles = Arc<
	dyn Fn(&EnvironmentPlan, &StagedProfile, bool, CancellationToken) -> PortFuture<ExportListing> + Send + Sync,
>;
/// Copies the files into a new output directory and gives each copy its source time.
pub type WriteExport =
	Arc<dyn Fn(ExportSources, Vec<ExportFile>, PathBuf, CancellationToken) -> PortFuture<()> + Send + Sync>;
