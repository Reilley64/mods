use crate::Resources;
use crate::environment_preparation::PreparationPorts;
use application::export::ExportEnvironmentDependencies;
use domain::GameBinding;

impl Resources {
	pub fn export_environment_dependencies(&self, binding: GameBinding) -> ExportEnvironmentDependencies {
		let preparation = PreparationPorts::new(self.root.clone(), binding);

		ExportEnvironmentDependencies {
			validate_export_destination: self
				.environment
				.validate_export_destination_port(self.root.clone()),
			prepare_environment_plan: preparation.prepare_environment_plan,
			project_profile: preparation.project_profile,
			stage_profile: preparation.stage_profile,
			list_export_files: self.environment.list_export_files_port(),
			write_export: self.environment.write_export_port(self.root.clone()),
			discard_staged_profile: preparation.discard_staged_profile,
		}
	}
}
