use crate::installation::ApprovedInstallation;
use crate::installation::InstallPlan;
use crate::installation::InstallationAssessment;
use crate::installation::InstallationState;
use crate::ports::PortFuture;
use domain::DataRelativePath;
use domain::EnvironmentRoot;
use domain::GameBinding;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitializationTargetAssessment {
	Available,
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

pub type AssessInitializationTarget =
	Arc<dyn Fn(EnvironmentRoot, CancellationToken) -> PortFuture<InitializationTargetAssessment> + Send + Sync>;
pub type PublishEnvironment = Arc<
	dyn Fn(EnvironmentRoot, InitializationPlan, CancellationToken) -> PortFuture<Vec<ProfileFileRecord>>
		+ Send
		+ Sync,
>;

pub type WriteInstallationChunk = Arc<dyn Fn(Vec<u8>, CancellationToken) -> PortFuture<()> + Send + Sync>;
pub type FinishInstallationFile = Arc<dyn Fn(CancellationToken) -> PortFuture<()> + Send + Sync>;

#[derive(Clone)]
pub struct InstallationFile {
	pub write_chunk: WriteInstallationChunk,
	pub finish: FinishInstallationFile,
}

pub type BeginInstallationFile =
	Arc<dyn Fn(DataRelativePath, CancellationToken) -> PortFuture<InstallationFile> + Send + Sync>;
pub type FinishInstallationChange = Arc<dyn Fn(CancellationToken) -> PortFuture<()> + Send + Sync>;

#[derive(Clone)]
pub struct InstallationChange {
	pub begin_file: BeginInstallationFile,
	pub finish: FinishInstallationChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationStateAccess {
	Preview,
	Mutation,
}

pub type LoadInstallationState =
	Arc<dyn Fn(InstallationStateAccess, CancellationToken) -> PortFuture<InstallationState> + Send + Sync>;
pub type AssessInstallation =
	Arc<dyn Fn(InstallPlan, CancellationToken) -> PortFuture<InstallationAssessment> + Send + Sync>;
pub type BeginInstallation =
	Arc<dyn Fn(ApprovedInstallation, CancellationToken) -> PortFuture<InstallationChange> + Send + Sync>;
