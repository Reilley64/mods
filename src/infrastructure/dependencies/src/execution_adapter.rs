use application::ErrorMarker;
use application::execution::ExecuteProgram;
use application::execution::ExecuteProgramError;
#[cfg(windows)]
use application::execution::execute_program;
use domain::EnvironmentRoot;
use domain::GameBinding;
#[cfg(windows)]
use infrastructure_execution::CallerSnapshot;
use infrastructure_execution::ExecutionCapture;
#[cfg(windows)]
use rootcause::prelude::ResultExt;
use rootcause::report;
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

	/// Runs the exec use case on one dedicated blocking thread.
	///
	/// The upstream session and hooked process must never be used concurrently.
	/// Ports are built and the whole use case runs on this thread with its own
	/// current-thread runtime; only the owned result crosses back to Tokio.
	pub fn into_execute_program(self) -> ExecuteProgram {
		Box::new(
			move |output_target, working_directory, program, arguments, cancellation| {
				Box::pin(async move {
					if cancellation.is_cancelled() {
						return Err(report!(ErrorMarker::operation_cancelled())
							.context(ExecuteProgramError));
					}

					#[cfg(not(windows))]
					{
						let _ = (self, output_target, working_directory, program, arguments);
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
									self.dependencies(),
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
