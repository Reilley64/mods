use crate::Resources;
use application::conflicts::ExplainPathDependencies;

impl Resources {
	pub fn explain_path_dependencies(&self) -> ExplainPathDependencies {
		ExplainPathDependencies {
			scan_environment: self.environment.scan_environment_conflicts_port(self.root.clone()),
			read_conflict_content: self.environment.read_conflict_content_port(self.root.clone()),
		}
	}
}
