use crate::Resources;
use application::conflicts::ListEffectiveConflictsDependencies;
use domain::GameBinding;

impl Resources {
	pub fn list_effective_conflicts_dependencies(
		&self,
		binding: GameBinding,
	) -> ListEffectiveConflictsDependencies {
		ListEffectiveConflictsDependencies {
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
