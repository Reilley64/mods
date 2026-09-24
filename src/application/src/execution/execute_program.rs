use crate::execution::ExecutionWarning;
use crate::ports::RunManagedProgram;
use domain::OutputTarget;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ExecuteProgramDependencies {
	pub run_managed_program: RunManagedProgram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecuteProgramOutput {
	pub status: ProcessStatus,
	pub warnings: Vec<ExecutionWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecuteProgramError;
impl fmt::Display for ExecuteProgramError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to execute program")
	}
}

#[tracing::instrument(skip_all)]
pub async fn execute_program(
	dependencies: ExecuteProgramDependencies,
	output_target: OutputTarget,
	working_directory: Option<WorkingDirectory>,
	program: Program,
	arguments: Vec<ProgramArgument>,
	cancellation: CancellationToken,
) -> Result<ExecuteProgramOutput, ExecuteProgramError> {
	dependencies
		.run_managed_program
		.call((output_target, working_directory, program, arguments, cancellation))
		.await
		.context(ExecuteProgramError)
}

#[cfg(test)]
mod tests {
	use super::ExecuteProgramDependencies;
	use super::ExecuteProgramOutput;
	use super::execute_program;
	use crate::ErrorMarker;
	use crate::execution::ExecutionWarning;
	use crate::ports::PortFuture;
	use domain::OutputTarget;
	use domain::ProcessStatus;
	use domain::Program;
	use domain::ProgramArgument;
	use rootcause::Result;
	use rootcause::report;
	use std::sync::Arc;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn forwards_typed_launch_values_and_preserves_nonzero_child_status() -> Result<(), ErrorMarker> {
		let cancellation = CancellationToken::new();
		let observed_cancellation = cancellation.clone();
		let dependencies = ExecuteProgramDependencies {
			run_managed_program: Arc::new(move |target, directory, program, arguments, token| {
				assert_eq!(target, OutputTarget::Overwrite);
				assert!(directory.is_none());
				assert_eq!(program.as_os_str(), "tool.exe");
				assert_eq!(arguments[0].as_os_str(), "");
				assert_eq!(arguments[1].as_os_str(), "--");
				token.cancel();
				Box::pin(async {
					Ok(ExecuteProgramOutput {
						status: ProcessStatus::new(259),
						warnings: vec![ExecutionWarning::LoadOrderNotEnforced],
					})
				}) as PortFuture<_>
			}),
		};
		let result = execute_program(
			dependencies,
			OutputTarget::Overwrite,
			None,
			Program::new("tool.exe".into())
				.map_err(|error| error.context(ErrorMarker::program_unsupported()))?,
			vec![
				ProgramArgument::new("".into())
					.map_err(|error| error.context(ErrorMarker::program_unsupported()))?,
				ProgramArgument::new("--".into())
					.map_err(|error| error.context(ErrorMarker::program_unsupported()))?,
			],
			cancellation,
		)
		.await
		.map_err(|error| error.context(ErrorMarker::execution_supervision_failed()))?;
		assert!(observed_cancellation.is_cancelled());
		assert_eq!(result.status.value(), 259);
		assert_eq!(result.warnings, vec![ExecutionWarning::LoadOrderNotEnforced]);
		Ok(())
	}

	#[tokio::test]
	async fn preserves_launcher_cause_under_fixed_use_case_context() -> Result<(), ErrorMarker> {
		let dependencies = ExecuteProgramDependencies {
			run_managed_program: Arc::new(|_, _, _, _, _| {
				Box::pin(async { Err(report!(ErrorMarker::program_not_found())) }) as PortFuture<_>
			}),
		};
		let result = execute_program(
			dependencies,
			OutputTarget::Overwrite,
			None,
			Program::new("missing.exe".into())
				.map_err(|error| error.context(ErrorMarker::program_unsupported()))?,
			vec![],
			CancellationToken::new(),
		)
		.await;
		let Err(error) = result else {
			return Err(report!(ErrorMarker::execution_supervision_failed()));
		};
		assert!(error
			.iter_reports()
			.any(|cause| cause.downcast_current_context::<ErrorMarker>()
				== Some(&ErrorMarker::program_not_found())));
		Ok(())
	}
}
