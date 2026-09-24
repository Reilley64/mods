use crate::Resources;
use crate::execution_adapter::ExecutionAdapter;
use application::execution::ExecuteProgramDependencies;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

impl Resources {
	pub fn execute_program_dependencies(
		&self,
		startup_directory: PathBuf,
		force_cancellation: CancellationToken,
	) -> ExecuteProgramDependencies {
		ExecuteProgramDependencies {
			run_managed_program: ExecutionAdapter::new(self.root.clone(), startup_directory)
				.with_force_cancellation(force_cancellation)
				.run_port(),
		}
	}
}
