use crate::ports::PortFuture;
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

pub type PublishExport = Arc<dyn Fn(Vec<ExportFile>, CancellationToken) -> PortFuture<()> + Send + Sync>;
pub type PrepareExport = Arc<dyn Fn(PathBuf, bool, CancellationToken) -> PortFuture<PreparedExport> + Send + Sync>;

pub struct PreparedExport {
	pub files: Vec<ExportFile>,
	pub publish: PublishExport,
}

/// A failed export retains this unpublished sibling directory. Presentation may
/// expose this typed path, never raw report formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedExport {
	pub path: PathBuf,
}
impl fmt::Display for RetainedExport {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("export staging directory retained")
	}
}
