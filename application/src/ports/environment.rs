use crate::ports::PortFuture;
use domain::EnvironmentRoot;
use domain::GameBinding;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitializationTargetAssessment {
	Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
	NothingToRecover,
	Committed,
	RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSource {
	pub name: &'static str,
	pub contents: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializationProfileSources {
	pub files: Vec<ProfileSource>,
	pub fallout_default_ini: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileFileDisposition {
	Imported,
	SeededFromGame,
	CreatedEmpty,
	Absent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileFileRecord {
	pub name: &'static str,
	pub disposition: ProfileFileDisposition,
}

#[derive(Debug, Clone)]
pub struct InitializationPlan {
	pub game_binding: GameBinding,
	pub profile_sources: InitializationProfileSources,
}

pub type RecoverEnvironment =
	Arc<dyn Fn(EnvironmentRoot, CancellationToken) -> PortFuture<RecoveryOutcome> + Send + Sync>;
pub type AssessInitializationTarget =
	Arc<dyn Fn(EnvironmentRoot, CancellationToken) -> PortFuture<InitializationTargetAssessment> + Send + Sync>;
pub type PublishEnvironment = Arc<
	dyn Fn(EnvironmentRoot, InitializationPlan, CancellationToken) -> PortFuture<Vec<ProfileFileRecord>>
		+ Send
		+ Sync,
>;
