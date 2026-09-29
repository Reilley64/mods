use crate::Resources;
use application::export::ExportEnvironmentDependencies;
use application::ports::PortFuture;
use domain::GameBinding;
use std::sync::Arc;

impl Resources {
	pub fn export_environment_dependencies(&self, binding: GameBinding) -> ExportEnvironmentDependencies {
		let validate = self.game_platform.validate_effective_port();
		let environment = self.environment.clone();
		let root = self.root.clone();

		ExportEnvironmentDependencies {
			prepare_export: Arc::new(move |output, include_saves, cancellation| {
				let validate = validate.clone();
				let environment = environment.clone();
				let root = root.clone();
				let binding = binding.clone();
				Box::pin(async move {
					let binding = validate.call((binding, cancellation.clone())).await?;

					environment
						.prepare_export_port(root, binding)
						.call((output, include_saves, cancellation))
						.await
				}) as PortFuture<_>
			}),
		}
	}
}
