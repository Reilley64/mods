use crate::errors::ErrorMarker;
use rootcause::Result;
use std::future::Future;
use std::pin::Pin;

mod archive;
mod environment;
mod execution;
mod game_platform;
mod settings;

pub use environment::AssessInitializationTarget;
pub use environment::InitializationPlan;
pub use environment::InitializationProfileSources;
pub use environment::InitializationTargetAssessment;
pub use environment::ProfileFileDisposition;
pub use environment::ProfileFileRecord;
pub use environment::ProfileSource;
pub use environment::PublishEnvironment;
pub use environment::RecoverEnvironment;
pub use environment::RecoveryOutcome;
pub use game_platform::GameInstallationSource;
pub use game_platform::LoadProfileSources;
pub use game_platform::ResolveGameInstallation;
pub use game_platform::ResolvedGameInstallation;
pub use game_platform::ValidateEffectiveBinding;
pub use game_platform::ValidateGameDirectory;
pub use game_platform::ValidateGameInstallation;
pub use settings::LoadSettings;
pub use settings::ReadInitializationGameOverride;
pub use settings::RecoverSettingsMutation;
pub use settings::StoreGameBinding;
pub use settings::StoredAndEffectiveBinding;

pub type PortFuture<T, ErrorContext = ErrorMarker> = Pin<Box<dyn Future<Output = Result<T, ErrorContext>> + Send>>;
