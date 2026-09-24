use crate::execution::ExecuteProgramOutput;
use crate::ports::PortFuture;
use crate::ports::ReportProgress;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub type RunManagedProgram = Arc<
	dyn Fn(
			OutputTarget,
			Option<WorkingDirectory>,
			Program,
			Vec<ProgramArgument>,
			Option<ReportProgress>,
			CancellationToken,
		) -> PortFuture<ExecuteProgramOutput>
		+ Send
		+ Sync,
>;
