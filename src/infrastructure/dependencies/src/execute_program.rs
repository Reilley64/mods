use crate::Resources;
use crate::execution_adapter::ExecutionAdapter;
use application::execution::ExecuteProgramError;
use application::execution::ExecuteProgramOutput;
use application::ports::PortFuture;
use domain::GameBinding;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use infrastructure_execution::ExecutionCapture;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Runs the composed exec use case on its own execution thread.
pub type ExecuteProgram = Box<
	dyn FnOnce(
			OutputTarget,
			Option<WorkingDirectory>,
			Program,
			Vec<ProgramArgument>,
			CancellationToken,
		) -> PortFuture<ExecuteProgramOutput, ExecuteProgramError>
		+ Send,
>;

impl Resources {
	/// Composes exec with private output capture. The use case runs on its own
	/// execution thread when the returned entry point is called.
	pub fn captured_execute_program(
		&self,
		binding: GameBinding,
		startup_directory: PathBuf,
		force_cancellation: CancellationToken,
	) -> (ExecuteProgram, Arc<ExecutionCapture>) {
		let capture = Arc::new(ExecutionCapture::new(&self.root.as_path().join("temp")));

		let execute_program = ExecutionAdapter::new(self.root.clone(), binding, startup_directory)
			.with_force_cancellation(force_cancellation)
			.with_capture(capture.clone())
			.into_execute_program();

		(execute_program, capture)
	}

	/// Composes exec with inherited standard streams. The use case runs on its
	/// own execution thread when the returned entry point is called.
	pub fn execute_program(
		&self,
		binding: GameBinding,
		startup_directory: PathBuf,
		force_cancellation: CancellationToken,
	) -> ExecuteProgram {
		ExecutionAdapter::new(self.root.clone(), binding, startup_directory)
			.with_force_cancellation(force_cancellation)
			.into_execute_program()
	}
}
