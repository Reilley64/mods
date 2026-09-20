use crate::Resources;
use application::settings::GetSettingDependencies;

impl Resources {
	pub fn get_setting_dependencies(&self) -> GetSettingDependencies {
		GetSettingDependencies {
			load_settings: self.settings.load_port(),
		}
	}
}
