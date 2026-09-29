use crate::errors::ErrorMarker;
use crate::ports::PortFuture;
use domain::ModName;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::ProviderIdentity;
use domain::WorkingDirectory;
use rootcause::Report;
use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Adapter-owned state behind an opaque execution handle.
///
/// Handles are `Send` but not `Sync`. A handle moves to one step at a time, so
/// adapter state behind it is never used concurrently.
pub struct AdapterState(Box<dyn Any + Send>);
impl AdapterState {
	pub fn new(state: impl Any + Send) -> Self {
		Self(Box::new(state))
	}

	/// Returns `None` when the state was created by a different adapter.
	pub fn downcast<T: Any>(self) -> Option<T> {
		self.0.downcast().ok().map(|state| *state)
	}

	/// Returns `None` when the state was created by a different adapter.
	pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
		self.0.downcast_ref()
	}
}

/// A resolved program, its child working directory, and its standard streams.
pub struct LaunchTarget(pub AdapterState);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchProvider {
	pub identity: ProviderIdentity,
	pub enabled: bool,
}

/// Providers, analytical winners, and canonical profile inputs for one launch.
pub struct LaunchPlan {
	pub providers: Vec<LaunchProvider>,
	pub state: AdapterState,
}

/// An advisory analytical plugin-projection diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileWarning {
	Unavailable {
		file: String,
		plugin: String,
	},
	Duplicate {
		file: String,
		plugin: String,
	},
	Unlisted {
		plugin: String,
	},
	/// The analytical projection is advisory; virtual timestamps do not enforce its order.
	LoadOrderNotEnforced,
}

/// Profile mappings for the virtual file system, kept opaque to the application.
pub struct ExecutionProfile(pub AdapterState);

pub struct ExecutionProfileProjection {
	pub profile: ExecutionProfile,
	pub warnings: Vec<ProfileWarning>,
}

/// Temporary profile INIs. Dropping the handle keeps `directory` on disk.
pub struct StagedExecutionProfile {
	pub directory: PathBuf,
	pub state: AdapterState,
}

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

pub type ResolveLaunchTarget = Arc<
	dyn Fn(Program, Vec<ProgramArgument>, Option<WorkingDirectory>) -> PortFuture<LaunchTarget> + Send + Sync,
>;
pub type PrepareLaunchPlan = Arc<dyn Fn(CancellationToken) -> PortFuture<LaunchPlan> + Send + Sync>;
pub type ProjectExecutionProfile = Arc<dyn Fn(&LaunchPlan) -> PortFuture<ExecutionProfileProjection> + Send + Sync>;
pub type StageExecutionProfile =
	Arc<dyn Fn(&LaunchPlan, CancellationToken) -> PortFuture<StagedExecutionProfile> + Send + Sync>;
pub type CreateVirtualFileSystem = Arc<
	dyn Fn(
			LaunchPlan,
			ExecutionProfile,
			&StagedExecutionProfile,
			Option<ModName>,
			CancellationToken,
		) -> PortFuture<VirtualFileSystem>
		+ Send
		+ Sync,
>;
pub type CloseVirtualFileSystem = Arc<dyn Fn(VirtualFileSystem) -> PortFuture<()> + Send + Sync>;
pub type LaunchProgram = Arc<dyn Fn(VirtualFileSystem, LaunchTarget) -> PortFuture<RunningProgram> + Send + Sync>;
/// Fails only for a handle from another adapter. Supervision and drain failures
/// are reported inside [`ProgramSupervision`] after the program handle is released.
pub type SuperviseProgram =
	Arc<dyn Fn(RunningProgram, CancellationToken) -> PortFuture<ProgramSupervision> + Send + Sync>;
pub type PreserveExecutionProfile = Arc<dyn Fn(StagedExecutionProfile) -> PortFuture<()> + Send + Sync>;
pub type FinishProgramOutput = Arc<dyn Fn(ProgramOutput) -> PortFuture<()> + Send + Sync>;
pub type CheckProfileState = Arc<dyn Fn(CancellationToken) -> PortFuture<()> + Send + Sync>;
