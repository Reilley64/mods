use crate::ports::InitializationProfileSources;
use crate::ports::PortFuture;
use domain::EnvironmentRoot;
use domain::GameBinding;
use domain::GameInstallationPath;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameInstallationSource {
	Explicit,
	Environment,
	Steam,
	BethesdaRegistryFallback,
}

#[derive(Debug, Clone)]
pub struct ResolvedGameInstallation {
	pub binding: GameBinding,
	pub source: GameInstallationSource,
}

pub type ResolveGameInstallation = Arc<
	dyn Fn(
			Option<GameInstallationPath>,
			Option<GameInstallationPath>,
			EnvironmentRoot,
			CancellationToken,
		) -> PortFuture<ResolvedGameInstallation>
		+ Send
		+ Sync,
>;
pub type ValidateGameInstallation =
	Arc<dyn Fn(GameInstallationPath, EnvironmentRoot) -> PortFuture<GameBinding> + Send + Sync>;
pub type ValidateGameDirectory =
	Arc<dyn Fn(GameInstallationPath, CancellationToken) -> PortFuture<GameBinding> + Send + Sync>;
pub type LoadProfileSources =
	Arc<dyn Fn(GameBinding, CancellationToken) -> PortFuture<InitializationProfileSources> + Send + Sync>;
pub type ValidateEffectiveBinding = Arc<dyn Fn(GameBinding) -> PortFuture<GameBinding> + Send + Sync>;
