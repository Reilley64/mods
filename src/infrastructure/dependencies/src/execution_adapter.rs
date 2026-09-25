use application::ErrorMarker;
use application::ports::RunManagedProgram;
use domain::EnvironmentRoot;
#[cfg(windows)]
use infrastructure_execution::CallerSnapshot;
use infrastructure_execution::ExecutionCapture;
#[cfg(windows)]
use infrastructure_settings::SettingsAdapter;
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
	settings: SettingsAdapter,
	force_cancellation: CancellationToken,
	capture: Option<Arc<ExecutionCapture>>,
}
impl ExecutionAdapter {
	/// Captures inherited lookup and settings inputs at the caller's startup directory.
	pub fn new(root: EnvironmentRoot, startup_directory: PathBuf) -> Self {
		#[cfg(not(windows))]
		let _ = (root, startup_directory);
		Self {
			#[cfg(windows)]
			caller: CallerSnapshot::new(startup_directory),
			#[cfg(windows)]
			settings: SettingsAdapter::new(root.clone()),
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

	pub fn run_port(&self) -> RunManagedProgram {
		let adapter = self.clone();
		Arc::new(
			move |output_target, working_directory, program, arguments, progress, cancellation| {
				let adapter = adapter.clone();
				Box::pin(async move {
					if cancellation.is_cancelled() {
						return Err(report!(ErrorMarker::operation_cancelled()));
					}
					#[cfg(not(windows))]
					{
						let _ = (
							adapter,
							output_target,
							working_directory,
							program,
							arguments,
							progress,
						);
						Err(report!(ErrorMarker::program_unsupported()))
					}
					#[cfg(windows)]
					{
						// The upstream session is thread-affine. Its complete lifetime stays on
						// this blocking worker; only the owned result crosses back to Tokio.
						let dispatcher = get_default(Clone::clone);
						let span = Span::current();
						spawn_blocking(move || {
							with_default(&dispatcher, || {
								let _entered = span.enter();
								let runtime = Builder::new_current_thread()
								.enable_time()
								.build()
								.context(ErrorMarker::execution_supervision_failed())?;
								runtime.block_on(adapter.execute(
									output_target,
									working_directory,
									program,
									arguments,
									progress,
									cancellation,
								))
							})
						})
						.await
						.context(ErrorMarker::execution_supervision_failed())?
					}
				})
			},
		)
	}
}
