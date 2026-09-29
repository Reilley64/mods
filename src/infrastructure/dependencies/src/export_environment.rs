use crate::Resources;
use crate::environment_preparation::PreparationPorts;
use application::export::ExportEnvironmentDependencies;
use application::ports::PortFuture;
use domain::GameBinding;
use std::sync::Arc;

impl Resources {
	pub fn export_environment_dependencies(&self, binding: GameBinding) -> ExportEnvironmentDependencies {
		let validate = self.game_platform.validate_effective_port();
		let preparation = PreparationPorts::new(self.root.clone(), binding.clone());
		let prepare = preparation.prepare_environment_plan;

		ExportEnvironmentDependencies {
			validate_export_destination: self
				.environment
				.validate_export_destination_port(self.root.clone()),
			// Unlike exec, export validates the Steam installation before it reads the environment.
			prepare_environment_plan: Arc::new(move |cancellation| {
				let validate = validate.clone();
				let prepare = prepare.clone();
				let binding = binding.clone();

				Box::pin(async move {
					validate.call((binding, cancellation.clone())).await?;

					prepare.call((cancellation,)).await
				}) as PortFuture<_>
			}),
			project_profile: preparation.project_profile,
			stage_profile: preparation.stage_profile,
			list_export_files: self.environment.list_export_files_port(),
			write_export: self.environment.write_export_port(self.root.clone()),
			discard_staged_profile: preparation.discard_staged_profile,
		}
	}
}
