use crate::Resources;
use application::conflicts::ExplainPathDependencies;
use domain::GameBinding;

impl Resources {
	pub fn explain_path_dependencies(&self, binding: GameBinding) -> ExplainPathDependencies {
		ExplainPathDependencies {
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
