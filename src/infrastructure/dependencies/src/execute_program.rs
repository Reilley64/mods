use crate::Resources;
use crate::execution_adapter::ExecutionAdapter;
use application::execution::ExecuteProgramDependencies;
use infrastructure_execution::ExecutionCapture;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

impl Resources {
	pub fn captured_execution_dependencies(
		&self,
		startup_directory: PathBuf,
		force_cancellation: CancellationToken,
	) -> (ExecuteProgramDependencies, Arc<ExecutionCapture>) {
		let capture = Arc::new(ExecutionCapture::new(&self.root.as_path().join("temp")));
		let adapter = ExecutionAdapter::new(self.root.clone(), self.game_platform.clone(), startup_directory)
			.with_force_cancellation(force_cancellation)
			.with_capture(capture.clone());

		let dependencies = ExecuteProgramDependencies {
			report_progress: None,
			resolve_launch_inputs: adapter.resolve_port(),
			prepare_execution_environment: adapter.prepare_port(),
			run_managed_program: adapter.run_port(),
		};

		(dependencies, capture)
	}

	pub fn execute_program_dependencies(
		&self,
		startup_directory: PathBuf,
		force_cancellation: CancellationToken,
	) -> ExecuteProgramDependencies {
		let adapter = ExecutionAdapter::new(self.root.clone(), self.game_platform.clone(), startup_directory)
			.with_force_cancellation(force_cancellation);

		ExecuteProgramDependencies {
			report_progress: None,
			resolve_launch_inputs: adapter.resolve_port(),
			prepare_execution_environment: adapter.prepare_port(),
			run_managed_program: adapter.run_port(),
		}
	}
}
