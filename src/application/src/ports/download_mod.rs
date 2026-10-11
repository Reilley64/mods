use crate::installation::DownloadModOutput;
use crate::installation::RemoteModSource;
use crate::ports::PortFuture;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type DownloadMod = Arc<dyn Fn(RemoteModSource, CancellationToken) -> PortFuture<DownloadModOutput> + Send + Sync>;
