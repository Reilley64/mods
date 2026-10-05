use crate::Resources;
use crate::execution_adapter::ExecutionAdapter;
use application::ErrorMarker;
use application::shortcut::CreateShortcutDependencies;
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
	pub fn create_shortcut_dependencies(&self, startup_directory: PathBuf) -> CreateShortcutDependencies {
		let execution = ExecutionAdapter::new(self.root.clone(), self.game_platform.clone(), startup_directory);
		#[cfg(not(windows))]
		{
			CreateShortcutDependencies {
				locate_launcher: Arc::new(|| {
					Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
				}),
				resolve_launch_inputs: execution.resolve_port(),
				prepare_execution_environment: execution.prepare_port(),
				load_settings: self.settings.load_port(),
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
				resolve_launch_inputs: execution.resolve_port(),
				prepare_execution_environment: execution.prepare_port(),
				load_settings: self.settings.load_port(),
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
