use crate::Resources;
use application::settings::ListSettingsDependencies;

impl Resources {
	pub fn list_settings_dependencies(&self) -> ListSettingsDependencies {
		ListSettingsDependencies {
			report_progress: None,
			load_settings: self.settings.load_port(),
		}
	}
}
