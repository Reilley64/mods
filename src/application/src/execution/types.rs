use crate::execution::ExecuteProgramError;
use crate::execution::ExecuteProgramOutput;
use crate::ports::PortFuture;
use crate::ports::ReportProgress;
use crate::preparation::PluginWarning;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionWarning {
	Plugin(PluginWarning),
	ProfileStateInvalid,
}

/// Runs one composed exec use case. Composition chooses the thread it runs on.
pub type ExecuteProgram = Box<
	dyn FnOnce(
			Option<ReportProgress>,
			OutputTarget,
			Option<WorkingDirectory>,
			Program,
			Vec<ProgramArgument>,
			CancellationToken,
		) -> PortFuture<ExecuteProgramOutput, ExecuteProgramError>
		+ Send,
>;
