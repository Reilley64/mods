use crate::ports::PortFuture;
use crate::settings::ResolvedSettings;
use crate::settings::SettingSource;
use domain::GameBinding;
use domain::GameInstallationPath;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct StoredAndEffectiveBinding {
	pub stored: GameBinding,
	pub effective: GameBinding,
	pub source: SettingSource,
	pub shadowed: bool,
}

pub type CheckSettingsReadiness = Arc<dyn Fn(CancellationToken) -> PortFuture<()> + Send + Sync>;
pub type LoadSettings = Arc<dyn Fn() -> PortFuture<ResolvedSettings> + Send + Sync>;
pub type PreviewGameBinding =
	Arc<dyn Fn(GameBinding, CancellationToken) -> PortFuture<StoredAndEffectiveBinding> + Send + Sync>;
pub type ReadInitializationGameOverride = Arc<dyn Fn() -> PortFuture<Option<GameInstallationPath>> + Send + Sync>;
pub type StoreGameBinding =
	Arc<dyn Fn(GameBinding, CancellationToken) -> PortFuture<StoredAndEffectiveBinding> + Send + Sync>;
