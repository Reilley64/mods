#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitializeEnvironmentWarning {
	BethesdaRegistryFallbackUsed,
}

mod initialize_environment;

pub use crate::ports::ProfileFileDisposition;
pub use crate::ports::ProfileFileRecord;
pub use initialize_environment::InitializeEnvironmentDependencies;
pub use initialize_environment::InitializeEnvironmentError;
pub use initialize_environment::InitializeEnvironmentOutput;
pub use initialize_environment::initialize_environment;
