use crate::Resources;
use application::conflicts::ListEffectiveConflictsDependencies;

impl Resources {
	pub fn list_effective_conflicts_dependencies(&self) -> ListEffectiveConflictsDependencies {
		ListEffectiveConflictsDependencies {
			scan_environment: self.environment.scan_environment_conflicts_port(self.root.clone()),
			read_conflict_content: self.environment.read_conflict_content_port(self.root.clone()),
		}
	}
}
