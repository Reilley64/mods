use crate::Resources;
use crate::execution_adapter::ExecutionAdapter;
use application::execution::ExecuteProgram;
use domain::GameBinding;
use infrastructure_execution::ExecutionCapture;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

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
