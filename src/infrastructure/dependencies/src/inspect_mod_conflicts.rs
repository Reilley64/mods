use crate::Resources;
use application::conflicts::InspectModConflictsDependencies;
use domain::GameBinding;

impl Resources {
	pub fn inspect_mod_conflicts_dependencies(&self, binding: GameBinding) -> InspectModConflictsDependencies {
		InspectModConflictsDependencies {
			report_progress: None,
			scan_environment: self
				.environment
				.scan_environment_conflicts_port(self.root.clone(), binding.clone()),
			read_conflict_content: self
				.environment
				.read_conflict_content_port(self.root.clone(), binding.clone()),
		}
	}
}
