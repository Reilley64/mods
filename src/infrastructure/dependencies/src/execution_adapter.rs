use application::ErrorMarker;
use application::execution::ExecuteProgram;
use application::execution::ExecuteProgramError;
#[cfg(windows)]
use application::execution::execute_program;
use application::ports::PortFuture;
use application::ports::ResolveLaunchTarget;
use domain::EnvironmentRoot;
use domain::GameBinding;
#[cfg(windows)]
use infrastructure_execution::CallerSnapshot;
use infrastructure_execution::ExecutionCapture;
#[cfg(windows)]
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::future::ready;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(windows)]
use tokio::runtime::Builder;
#[cfg(windows)]
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;
#[cfg(windows)]
use tracing::Span;
#[cfg(windows)]
use tracing::dispatcher::get_default;
#[cfg(windows)]
use tracing::dispatcher::with_default;

#[cfg(windows)]
mod native;

/// Composes one managed upstream execution without a shell or detached lifetime.
#[derive(Clone)]
pub(crate) struct ExecutionAdapter {
	#[cfg(windows)]
	root: EnvironmentRoot,
	#[cfg(windows)]
	caller: CallerSnapshot,
	#[cfg(windows)]
	binding: GameBinding,
	force_cancellation: CancellationToken,
	capture: Option<Arc<ExecutionCapture>>,
}
impl ExecutionAdapter {
	/// Captures inherited lookup and supplied binding at the caller's startup directory.
	pub fn new(root: EnvironmentRoot, binding: GameBinding, startup_directory: PathBuf) -> Self {
		#[cfg(not(windows))]
		let _ = (root, binding, startup_directory);
		Self {
			#[cfg(windows)]
			caller: CallerSnapshot::new(startup_directory),
			#[cfg(windows)]
			binding,
			#[cfg(windows)]
			root,
			force_cancellation: CancellationToken::new(),
			capture: None,
		}
	}
	/// Supplies the second console interrupt separately from cooperative cancellation.
	pub fn with_force_cancellation(mut self, cancellation: CancellationToken) -> Self {
		self.force_cancellation = cancellation;
		self
	}
	pub fn with_capture(mut self, capture: Arc<ExecutionCapture>) -> Self {
		self.capture = Some(capture);
		self
	}

	pub fn resolve_launch_target_port(&self) -> ResolveLaunchTarget {
		let adapter = self.clone();
		Arc::new(
			move |program, arguments, working_directory, cancellation: CancellationToken| {
				#[cfg(not(windows))]
				let result = {
					let _ = (&adapter, program, arguments, working_directory);
					if cancellation.is_cancelled() {
						Err(report!(ErrorMarker::operation_cancelled()))
					} else {
						Err(report!(ErrorMarker::program_unsupported()))
					}
				};
				#[cfg(windows)]
				let result = adapter.resolve_target(program, arguments, working_directory, &cancellation);

				Box::pin(ready(result)) as PortFuture<_>
			},
		)
	}

	/// Runs the exec use case on one dedicated blocking thread.
	///
	/// The upstream session and hooked process must never be used concurrently.
	/// Ports are built and the whole use case runs on this thread with its own
	/// current-thread runtime; only the owned result crosses back to Tokio.
	pub fn into_execute_program(self) -> ExecuteProgram {
		Box::new(
			move |report_progress, output_target, working_directory, program, arguments, cancellation| {
				Box::pin(async move {
					if cancellation.is_cancelled() {
						return Err(report!(ErrorMarker::operation_cancelled())
							.context(ExecuteProgramError));
					}

					#[cfg(not(windows))]
					{
						let _ = (
							self,
							report_progress,
							output_target,
							working_directory,
							program,
							arguments,
						);
						Err(report!(ErrorMarker::program_unsupported())
							.context(ExecuteProgramError))
					}
					#[cfg(windows)]
					{
						let dispatcher = get_default(Clone::clone);
						let span = Span::current();

						spawn_blocking(move || {
							with_default(&dispatcher, || {
								let _entered = span.enter();

								let runtime = Builder::new_current_thread()
								.enable_time()
								.build()
								.context(ErrorMarker::execution_supervision_failed())
								.context(ExecuteProgramError)?;

								runtime.block_on(execute_program(
									self.dependencies(report_progress),
									self.binding.clone(),
									output_target,
									working_directory,
									program,
									arguments,
									cancellation,
								))
							})
						})
						.await
						.context(ErrorMarker::execution_supervision_failed())
						.context(ExecuteProgramError)?
					}
				})
			},
		)
	}
}

#[cfg(test)]
mod tests {
	use super::ExecutionAdapter;
	use application::ErrorCode;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::Program;
	use domain::WorkingDirectory;
	use std::env::temp_dir;
	use std::error::Error;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn cancelled_launch_resolution_reports_cancellation() -> Result<(), Box<dyn Error>> {
		let root = EnvironmentRoot::new(temp_dir().join("environment")).map_err(|_| "root fixture")?;
		let game = GameInstallationPath::new(temp_dir().join("game")).map_err(|_| "game fixture")?;
		let program = Program::new("tool.exe".into()).map_err(|_| "program fixture")?;
		let working_directory = WorkingDirectory::new(temp_dir()).map_err(|_| "directory fixture")?;
		let resolve =
			ExecutionAdapter::new(root, GameBinding::new(game), temp_dir()).resolve_launch_target_port();
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = resolve
			.call((program, Vec::new(), working_directory, cancellation))
			.await;

		assert!(result.is_err_and(|error| error.current_context().code() == ErrorCode::OperationCancelled));
		Ok(())
	}
}
