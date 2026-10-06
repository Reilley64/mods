use crate::Resources;
use application::environment::InitializeEnvironmentDependencies;

impl Resources {
	pub fn initialize_environment_dependencies(&self) -> InitializeEnvironmentDependencies {
		InitializeEnvironmentDependencies {
			assess_target: self.environment.assess_port(),
			read_game_override: self.settings.initialization_override_port(),
			validate_game_directory: self.game_platform.validate_directory_port(),
			discover_game_installation: self.game_platform.discover_port(),
			load_profile_sources: self.game_platform.profile_sources_port(),
			publish_environment: self.environment.publish_port(),
		}
	}
}
