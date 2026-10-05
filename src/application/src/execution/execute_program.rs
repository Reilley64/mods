use crate::execution::ExecutionWarning;
use crate::ports::PrepareExecutionEnvironment;
use crate::ports::ProgressEvent;
use crate::ports::ReportProgress;
use crate::ports::ResolveLaunchInputs;
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
	pub report_progress: Option<ReportProgress>,
	pub resolve_launch_inputs: ResolveLaunchInputs,
	pub prepare_execution_environment: PrepareExecutionEnvironment,
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
	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::PreparingExecution,)).await;
	}

	let launch = dependencies
		.resolve_launch_inputs
		.call((working_directory, program, arguments, cancellation.clone()))
		.await
		.context(ExecuteProgramError)?;
	let prepared = dependencies
		.prepare_execution_environment
		.call((output_target.clone(), cancellation.clone()))
		.await
		.context(ExecuteProgramError)?;

	let output = dependencies
		.run_managed_program
		.call((
			output_target,
			launch,
			prepared,
			dependencies.report_progress.clone(),
			cancellation,
		))
		.await
		.context(ExecuteProgramError)?;

	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::ExecutionFinished,)).await;
	}

	Ok(output)
}

#[cfg(test)]
mod tests {
	use super::ExecuteProgramDependencies;
	use super::ExecuteProgramOutput;
	use super::execute_program;
	use crate::ErrorMarker;
	use crate::execution::ExecutionWarning;
	use crate::ports::PortFuture;
	use crate::ports::PreparedExecution;
	use crate::ports::ResolvedLaunch;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ModName;
	use domain::OutputTarget;
	use domain::ProcessStatus;
	use domain::Program;
	use domain::ProgramArgument;
	use domain::SteamBuildId;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;
	use rootcause::report;
	use std::env::temp_dir;
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;
	use std::sync::atomic::Ordering;
	use tokio_util::sync::CancellationToken;

	fn resolved_launch() -> ResolvedLaunch {
		ResolvedLaunch {
			program: temp_dir().join("tool.exe"),
			working_directory: temp_dir(),
			command_line: "\"tool.exe\"".into(),
			target_lease: Arc::new(()),
		}
	}

	fn prepared_execution() -> Result<PreparedExecution, ErrorMarker> {
		let binding = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("game"))
				.context(ErrorMarker::game_install_invalid())?,
			SteamBuildId::new(1).context(ErrorMarker::game_install_invalid())?,
		);
		Ok(PreparedExecution {
			game_binding: binding.clone(),
			providers: Vec::new(),
			winners: Vec::new(),
			visible_files: Vec::new(),
			profile_files: Vec::new(),
			profile_directory: temp_dir().join("profile"),
			data_directory: temp_dir().join("game").join("Data"),
			cache_directory: temp_dir().join("cache"),
			revalidation_basis: Arc::new(()),
		})
	}

	#[tokio::test]
	async fn composes_launch_resolution_preparation_and_run_and_preserves_nonzero_child_status()
	-> Result<(), ErrorMarker> {
		let cancellation = CancellationToken::new();
		let observed_cancellation = cancellation.clone();
		let prepared = prepared_execution()?;
		let expected_basis = prepared.revalidation_basis.clone();
		let target = OutputTarget::DataMod(
			ModName::new("Output".into()).context(ErrorMarker::invalid_output_target())?,
		);
		let expected_target = target.clone();
		let prepared_target = target.clone();
		let dependencies = ExecuteProgramDependencies {
			report_progress: None,
			resolve_launch_inputs: Arc::new(|directory, program, arguments, token| {
				assert!(directory.is_none());
				assert_eq!(program.as_os_str(), "tool.exe");
				assert_eq!(arguments[0].as_os_str(), "");
				assert_eq!(arguments[1].as_os_str(), "--");
				assert!(!token.is_cancelled());
				Box::pin(async { Ok(resolved_launch()) }) as PortFuture<_>
			}),
			prepare_execution_environment: Arc::new(move |target, token| {
				assert_eq!(target, prepared_target);
				assert!(!token.is_cancelled());
				let prepared = prepared.clone();
				Box::pin(async move { Ok(prepared) }) as PortFuture<_>
			}),
			run_managed_program: Arc::new(move |target, launch, prepared, _, token| {
				assert_eq!(target, expected_target);
				assert_eq!(launch.program, temp_dir().join("tool.exe"));
				assert!(Arc::ptr_eq(&prepared.revalidation_basis, &expected_basis));
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
			target,
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
	async fn preserves_launch_resolution_cause_and_stops_before_preparation() -> Result<(), ErrorMarker> {
		let prepared = Arc::new(AtomicBool::new(false));
		let observed = prepared.clone();
		let dependencies = ExecuteProgramDependencies {
			report_progress: None,
			resolve_launch_inputs: Arc::new(|_, _, _, _| {
				Box::pin(async { Err(report!(ErrorMarker::program_not_found())) }) as PortFuture<_>
			}),
			prepare_execution_environment: Arc::new(move |_, _| {
				observed.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ErrorMarker::environment_invalid(None))) })
					as PortFuture<_>
			}),
			run_managed_program: Arc::new(|_, _, _, _, _| {
				Box::pin(async { Err(report!(ErrorMarker::vfs_failed())) }) as PortFuture<_>
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
		let error = result
			.err()
			.ok_or_else(|| report!(ErrorMarker::execution_supervision_failed()))?;
		assert!(error
			.iter_reports()
			.any(|cause| cause.downcast_current_context::<ErrorMarker>()
				== Some(&ErrorMarker::program_not_found())));
		assert!(!prepared.load(Ordering::SeqCst));
		Ok(())
	}

	#[tokio::test]
	async fn preparation_failure_is_preserved_and_never_runs() -> Result<(), ErrorMarker> {
		let ran = Arc::new(AtomicBool::new(false));
		let observed = ran.clone();
		let dependencies = ExecuteProgramDependencies {
			report_progress: None,
			resolve_launch_inputs: Arc::new(|_, _, _, _| {
				Box::pin(async { Ok(resolved_launch()) }) as PortFuture<_>
			}),
			prepare_execution_environment: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::output_target_disabled())) }) as PortFuture<_>
			}),
			run_managed_program: Arc::new(move |_, _, _, _, _| {
				observed.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ErrorMarker::vfs_failed())) }) as PortFuture<_>
			}),
		};
		let result = execute_program(
			dependencies,
			OutputTarget::Overwrite,
			None,
			Program::new("tool.exe".into())
				.map_err(|error| error.context(ErrorMarker::program_unsupported()))?,
			vec![],
			CancellationToken::new(),
		)
		.await;
		let error = result
			.err()
			.ok_or_else(|| report!(ErrorMarker::execution_supervision_failed()))?;
		assert!(error
			.iter_reports()
			.any(|cause| cause.downcast_current_context::<ErrorMarker>()
				== Some(&ErrorMarker::output_target_disabled())));
		assert!(!ran.load(Ordering::SeqCst));
		Ok(())
	}
}
