use application::ErrorMarker;
use application::ports::PortFuture;
use application::ports::PrepareExecutionEnvironment;
use application::ports::PreparedExecution;
use application::ports::ResolveLaunchInputs;
use application::ports::RunManagedProgram;
use domain::EnvironmentRoot;
use domain::OutputTarget;
use domain::ProviderIdentity;
use infrastructure_environment::EnvironmentAdapter;
#[cfg(windows)]
use infrastructure_execution::CallerSnapshot;
use infrastructure_execution::ExecutionCapture;
use infrastructure_game_platform::GamePlatformAdapter;
use infrastructure_settings::SettingsAdapter;
use rootcause::Result;
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
	root: EnvironmentRoot,
	#[cfg(windows)]
	caller: CallerSnapshot,
	settings: SettingsAdapter,
	platform: GamePlatformAdapter,
	force_cancellation: CancellationToken,
	capture: Option<Arc<ExecutionCapture>>,
}
impl ExecutionAdapter {
	/// Captures inherited lookup and settings inputs at the caller's startup directory.
	pub fn new(root: EnvironmentRoot, platform: GamePlatformAdapter, startup_directory: PathBuf) -> Self {
		#[cfg(not(windows))]
		let _ = startup_directory;
		Self {
			#[cfg(windows)]
			caller: CallerSnapshot::new(startup_directory),
			settings: SettingsAdapter::new(root.clone()),
			root,
			platform,
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

	pub fn resolve_port(&self) -> ResolveLaunchInputs {
		let adapter = self.clone();
		Arc::new(move |working_directory, program, arguments, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				#[cfg(not(windows))]
				{
					let _ = (adapter, working_directory, program, arguments);
					Err(report!(ErrorMarker::program_unsupported()))
				}
				#[cfg(windows)]
				{
					adapter.resolve(working_directory, program, arguments)
				}
			}) as PortFuture<_>
		})
	}

	pub fn prepare_port(&self) -> PrepareExecutionEnvironment {
		let adapter = self.clone();
		Arc::new(move |output_target, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.prepare(output_target, cancellation).await }) as PortFuture<_>
		})
	}

	async fn prepare(
		&self,
		output_target: OutputTarget,
		cancellation: CancellationToken,
	) -> Result<PreparedExecution, ErrorMarker> {
		let effective_binding = self.settings.load_execution_binding(&cancellation)?;
		let binding = self
			.platform
			.validate_effective_port(self.root.clone())
			.call((effective_binding, cancellation.clone()))
			.await?;
		let prepared = EnvironmentAdapter.prepare_execution(&self.root, &binding, &cancellation)?;
		if let OutputTarget::DataMod(name) = output_target {
			let provider = prepared
				.providers
				.iter()
				.find(
					|provider| matches!(&provider.identity, ProviderIdentity::DataMod { mod_name, .. } if *mod_name == name),
				)
				.ok_or_else(|| {
					report!(ErrorMarker::output_target_not_found().with_mod_name(name.clone()))
				})?;
			if !provider.enabled {
				return Err(report!(ErrorMarker::output_target_disabled().with_mod_name(name)));
			}
		}

		Ok(prepared)
	}

	pub fn run_port(&self) -> RunManagedProgram {
		let adapter = self.clone();
		Arc::new(move |output_target, launch, prepared, progress, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				#[cfg(not(windows))]
				{
					let _ = (adapter, output_target, launch, prepared, progress);
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
								launch,
								prepared,
								progress,
								cancellation,
							))
						})
					})
					.await
					.context(ErrorMarker::execution_supervision_failed())?
				}
			})
		})
	}
}
