use crate::Resources;
use application::settings::SetGameDirectoryDependencies;
use infrastructure_settings::LoadedSettings;

impl Resources {
	pub fn set_game_directory_dependencies(&self, loaded: LoadedSettings) -> SetGameDirectoryDependencies {
		SetGameDirectoryDependencies {
			report_progress: None,
			check_settings_readiness: self.settings.readiness_port(),
			validate_game_directory: self.game_platform.validate_directory_port(self.root.clone()),
			preview_game_binding: self.settings.preview_port(loaded.clone()),
			validate_effective_binding: self.game_platform.validate_effective_port(self.root.clone()),
			store_game_binding: self.settings.store_port(loaded),
		}
	}
}
