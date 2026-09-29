use crate::errors::ErrorMarker;
use crate::ports::AdapterState;
use crate::ports::EnvironmentPlan;
use crate::ports::PortFuture;
use crate::ports::StagedProfile;
use domain::ModName;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use rootcause::Report;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// A resolved program, its child working directory, and its standard streams.
pub struct LaunchTarget(pub AdapterState);

/// A configured virtual file system. Dropping it before launch closes it.
pub struct VirtualFileSystem(pub AdapterState);

/// A launched program that has not been resumed yet.
pub struct RunningProgram(pub AdapterState);

/// Private output streams that must be finished after profile preservation.
pub struct ProgramOutput(pub AdapterState);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramExit {
	pub status: ProcessStatus,
	/// Supervision terminated the Job after cancellation.
	pub forced: bool,
}

/// Supervision results after the program handle is released.
///
/// `job_drained` is `Ok(true)` only when every managed process is known to have
/// stopped. The staged profile may be preserved only in that case.
pub struct ProgramSupervision {
	pub exit: Result<ProgramExit, Report<ErrorMarker>>,
	pub job_drained: Result<bool, Report>,
	pub output: ProgramOutput,
}

pub type ResolveLaunchTarget =
	Arc<dyn Fn(Program, Vec<ProgramArgument>, WorkingDirectory) -> PortFuture<LaunchTarget> + Send + Sync>;
pub type CreateVirtualFileSystem = Arc<
	dyn Fn(EnvironmentPlan, &StagedProfile, Option<ModName>, CancellationToken) -> PortFuture<VirtualFileSystem>
		+ Send
		+ Sync,
>;
pub type LaunchProgram = Arc<dyn Fn(VirtualFileSystem, LaunchTarget) -> PortFuture<RunningProgram> + Send + Sync>;
/// Fails only for a handle from another adapter. Supervision and drain failures
/// are reported inside [`ProgramSupervision`] after the program handle is released.
pub type SuperviseProgram =
	Arc<dyn Fn(RunningProgram, CancellationToken) -> PortFuture<ProgramSupervision> + Send + Sync>;
pub type PreserveExecutionProfile = Arc<dyn Fn(StagedProfile) -> PortFuture<()> + Send + Sync>;
pub type FinishProgramOutput = Arc<dyn Fn(ProgramOutput) -> PortFuture<()> + Send + Sync>;
pub type CheckProfileState = Arc<dyn Fn(CancellationToken) -> PortFuture<()> + Send + Sync>;
