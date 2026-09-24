use crate::execution::ExecuteProgramOutput;
use crate::ports::PortFuture;
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
			CancellationToken,
		) -> PortFuture<ExecuteProgramOutput>
		+ Send
		+ Sync,
>;
