use crate::types::NexusApiKey;
use crate::types::NexusMod;
use crate::types::NexusRequest;
use application::installation::DownloadedMod;
use application::installation::NexusProvenance;
use application::ports::PortFuture;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub(crate) type ParseNexusSource = Arc<dyn Fn(String, Option<u64>) -> PortFuture<NexusRequest> + Send + Sync>;
pub(crate) type LoadNexusApiKey = Arc<dyn Fn() -> PortFuture<Option<NexusApiKey>> + Send + Sync>;
pub(crate) type ReadNexusCache =
	Arc<dyn Fn(NexusRequest, CancellationToken) -> PortFuture<Option<DownloadedMod>> + Send + Sync>;
pub(crate) type ResolveNexusMod =
	Arc<dyn Fn(NexusRequest, NexusApiKey, CancellationToken) -> PortFuture<NexusMod> + Send + Sync>;
pub(crate) type DownloadNexusArchive =
	Arc<dyn Fn(NexusProvenance, NexusApiKey, CancellationToken) -> PortFuture<DownloadedMod> + Send + Sync>;
