use crate::Resources;
use crate::execution_adapter::ExecutionAdapter;
use application::execution::ExecuteProgramDependencies;
use execution::ExecutionCapture;
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

		let dependencies = ExecuteProgramDependencies {
			report_progress: None,
			run_managed_program: ExecutionAdapter::new(self.root.clone(), startup_directory)
				.with_force_cancellation(force_cancellation)
				.with_capture(capture.clone())
				.run_port(),
		};

		(dependencies, capture)
	}

	pub fn execute_program_dependencies(
		&self,
		startup_directory: PathBuf,
		force_cancellation: CancellationToken,
	) -> ExecuteProgramDependencies {
		ExecuteProgramDependencies {
			report_progress: None,
			run_managed_program: ExecutionAdapter::new(self.root.clone(), startup_directory)
				.with_force_cancellation(force_cancellation)
				.run_port(),
		}
	}
}
