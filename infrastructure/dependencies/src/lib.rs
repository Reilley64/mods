mod get_setting;
mod initialize_environment;
mod list_settings;
mod set_game_directory;

use domain::EnvironmentRoot;
use environment::EnvironmentAdapter;
use game_platform::GamePlatformAdapter;
use settings::SettingsAdapter;

#[derive(Clone)]
pub struct Resources {
	root: EnvironmentRoot,
	environment: EnvironmentAdapter,
	settings: SettingsAdapter,
	game_platform: GamePlatformAdapter,
}

impl Resources {
	pub fn system(root: EnvironmentRoot) -> Self {
		let environment = EnvironmentAdapter;
		let settings = SettingsAdapter::new(root.clone());
		let game_platform = GamePlatformAdapter::system();
		Self {
			root,
			environment,
			settings,
			game_platform,
		}
	}
}
