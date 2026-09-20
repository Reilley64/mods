use crate::Resources;
use application::conflicts::InspectModConflictsDependencies;

impl Resources {
	pub fn inspect_mod_conflicts_dependencies(&self) -> InspectModConflictsDependencies {
		InspectModConflictsDependencies {
			scan_environment: self.environment.scan_environment_conflicts_port(self.root.clone()),
			read_conflict_content: self.environment.read_conflict_content_port(self.root.clone()),
		}
	}
}
