mod create_shortcut;
mod environment_preparation;
mod execute_program;
mod execution_adapter;
mod explain_path;
mod export_environment;
mod get_setting;
mod initialize_environment;
mod inspect_mod_conflicts;
mod install_archive;
mod list_effective_conflicts;
mod list_settings;
mod set_game_directory;

use application::ErrorMarker;
use domain::EnvironmentRoot;
use infrastructure_archive::ArchiveAdapter;
use infrastructure_environment::EnvironmentAdapter;
pub use infrastructure_execution::CapturedOutput;
pub use infrastructure_execution::CapturedStream;
pub use infrastructure_execution::ExecutionCapture;
#[cfg(windows)]
pub use infrastructure_execution::detach_console;
#[cfg(windows)]
pub use infrastructure_execution::show_error;
use infrastructure_game_platform::GamePlatformAdapter;
pub use infrastructure_settings::LoadedSettings;
use infrastructure_settings::SettingsAdapter;
pub use infrastructure_settings::SettingsLoadMode;
use rootcause::Result;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Resources {
	root: EnvironmentRoot,
	environment: EnvironmentAdapter,
	settings: SettingsAdapter,
	game_platform: GamePlatformAdapter,
	archive: ArchiveAdapter,
}

impl Resources {
	pub async fn load_settings(
		&self,
		mode: SettingsLoadMode,
		cancellation: &CancellationToken,
	) -> Result<LoadedSettings, ErrorMarker> {
		self.settings.load_command(mode, cancellation).await
	}

	pub fn system(root: EnvironmentRoot) -> Self {
		let environment = EnvironmentAdapter;
		let settings = SettingsAdapter::new(root.clone());
		let game_platform = GamePlatformAdapter::system();
		let archive = ArchiveAdapter;
		Self {
			root,
			environment,
			settings,
			game_platform,
			archive,
		}
	}
}
