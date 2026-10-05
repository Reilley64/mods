use crate::Resources;
use crate::environment_preparation::PreparationPorts;
use crate::execution_adapter::ExecutionAdapter;
use application::ErrorMarker;
use application::shortcut::CreateShortcutDependencies;
use domain::GameBinding;
#[cfg(windows)]
use infrastructure_execution::persist_shortcut;
#[cfg(windows)]
use rootcause::prelude::ResultExt;
#[cfg(not(windows))]
use rootcause::report;
#[cfg(windows)]
use std::env::current_exe;
#[cfg(windows)]
use std::fs::canonicalize;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(windows)]
use tokio::task::spawn_blocking;

impl Resources {
	pub fn create_shortcut_dependencies(
		&self,
		binding: GameBinding,
		startup_directory: PathBuf,
	) -> CreateShortcutDependencies {
		let execution = ExecutionAdapter::new(self.root.clone(), binding.clone(), startup_directory);
		let preparation = PreparationPorts::new(self.root.clone(), binding);
		#[cfg(not(windows))]
		{
			CreateShortcutDependencies {
				locate_launcher: Arc::new(|| {
					Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
				}),
				resolve_launch_target: execution.resolve_launch_target_port(),
				prepare_environment_plan: preparation.prepare_environment_plan,
				project_profile: preparation.project_profile,
				locate_environment_root: Arc::new(|| {
					Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
				}),
				persist: Arc::new(|_| {
					Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
				}),
			}
		}
		#[cfg(windows)]
		{
			let root = self.root.clone();
			CreateShortcutDependencies {
				locate_launcher: Arc::new(|| {
					let launcher = current_exe()
						.and_then(canonicalize)
						.context(ErrorMarker::shortcut_launch_invalid());
					Box::pin(async move { launcher })
				}),
				resolve_launch_target: execution.resolve_launch_target_port(),
				prepare_environment_plan: preparation.prepare_environment_plan,
				project_profile: preparation.project_profile,
				locate_environment_root: Arc::new(move || {
					let environment = canonicalize(root.as_path())
						.context(ErrorMarker::shortcut_launch_invalid());
					Box::pin(async move { environment })
				}),
				persist: Arc::new(|definition| {
					Box::pin(async move {
						spawn_blocking(move || persist_shortcut(definition))
							.await
							.context(ErrorMarker::shortcut_failed())?
					})
				}),
			}
		}
	}
}
