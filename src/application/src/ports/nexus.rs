use crate::nexus::AcquiredNexusArchive;
use crate::nexus::NexusApiKey;
use crate::nexus::NexusMod;
use crate::nexus::NexusProvenance;
use crate::nexus::NexusRequest;
use crate::ports::PortFuture;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type ParseNexusSource = Arc<dyn Fn(String, Option<u64>) -> PortFuture<NexusRequest> + Send + Sync>;
pub type LoadNexusApiKey = Arc<dyn Fn() -> PortFuture<Option<NexusApiKey>> + Send + Sync>;
pub type ReadNexusCache =
	Arc<dyn Fn(NexusRequest, CancellationToken) -> PortFuture<Option<AcquiredNexusArchive>> + Send + Sync>;
pub type ResolveNexusMod =
	Arc<dyn Fn(NexusRequest, NexusApiKey, CancellationToken) -> PortFuture<NexusMod> + Send + Sync>;
pub type DownloadNexusArchive = Arc<
	dyn Fn(NexusProvenance, NexusApiKey, CancellationToken) -> PortFuture<AcquiredNexusArchive> + Send + Sync,
>;
