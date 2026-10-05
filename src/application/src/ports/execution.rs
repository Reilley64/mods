use crate::execution::ExecuteProgramOutput;
use crate::installation::InstallationState;
use crate::ports::PortFuture;
use crate::ports::ReportProgress;
use domain::DataRelativePath;
use domain::GameBinding;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::ProviderIdentity;
use domain::ProviderReference;
use domain::WorkingDirectory;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ResolvedLaunch {
	pub program: PathBuf,
	pub working_directory: PathBuf,
	pub command_line: OsString,
	pub target_lease: Arc<dyn Send + Sync>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProvider {
	pub identity: ProviderIdentity,
	pub root: PathBuf,
	pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionVisibleFile {
	pub path: DataRelativePath,
	pub physical_path: PathBuf,
	pub modified: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionProfileText {
	pub name: &'static str,
	pub text: String,
}

/// Validated, read-only inputs for managed execution.
/// Winners and visible files describe the analytical Data projection, not observed
/// runtime visibility. Profile texts come from canonical Profile State.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedExecution {
	pub game_binding: GameBinding,
	pub providers: Vec<ExecutionProvider>,
	pub winners: Vec<ProviderReference>,
	pub visible_files: Vec<ExecutionVisibleFile>,
	pub profile_files: Vec<ExecutionProfileText>,
	pub profile_directory: PathBuf,
	pub data_directory: PathBuf,
	pub cache_directory: PathBuf,
	pub consumed_state: InstallationState,
	pub consumed_bytes: Vec<(PathBuf, Vec<u8>)>,
	pub file_lengths: Vec<u64>,
}

pub type ResolveLaunchInputs = Arc<
	dyn Fn(
			Option<WorkingDirectory>,
			Program,
			Vec<ProgramArgument>,
			CancellationToken,
		) -> PortFuture<ResolvedLaunch>
		+ Send
		+ Sync,
>;
pub type PrepareExecutionEnvironment =
	Arc<dyn Fn(OutputTarget, CancellationToken) -> PortFuture<PreparedExecution> + Send + Sync>;
pub type RunManagedProgram = Arc<
	dyn Fn(
			OutputTarget,
			ResolvedLaunch,
			PreparedExecution,
			Option<ReportProgress>,
			CancellationToken,
		) -> PortFuture<ExecuteProgramOutput>
		+ Send
		+ Sync,
>;
