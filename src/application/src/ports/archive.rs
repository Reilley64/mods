use crate::installation::ArchiveIndex;
use crate::ports::BeginInstallationFile;
use crate::ports::PortFuture;
use crate::ports::ReportProgress;
use domain::ArchiveIdentity;
use domain::ArchivePath;
use domain::InstallCandidate;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type IndexArchive = Arc<
	dyn Fn(ArchivePath, Option<ReportProgress>, CancellationToken) -> PortFuture<ArchiveIndex> + Send + Sync,
>;
pub type ExtractApprovedFiles = Arc<
	dyn Fn(
			ArchivePath,
			ArchiveIdentity,
			Vec<InstallCandidate>,
			BeginInstallationFile,
			Option<ReportProgress>,
			CancellationToken,
		) -> PortFuture<()>
		+ Send
		+ Sync,
>;
