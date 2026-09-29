use crate::Resources;
use application::ErrorMarker;
use application::export::ExportEnvironmentDependencies;
use application::export::PreparedExport;
use application::ports::PortFuture;
use domain::GameBinding;
use infrastructure_settings::LoadedSettings;
use rootcause::report;
use std::sync::Arc;

impl Resources {
	pub fn export_environment_dependencies(
		&self,
		loaded: LoadedSettings,
		binding: GameBinding,
	) -> ExportEnvironmentDependencies {
		let settings = self.settings.clone();
		let validate = self.game_platform.validate_effective_port(self.root.clone());
		let environment = self.environment.clone();
		let root = self.root.clone();

		ExportEnvironmentDependencies {
			prepare_export: Arc::new(move |output, include_saves, cancellation| {
				let settings = settings.clone();
				let validate = validate.clone();
				let environment = environment.clone();
				let root = root.clone();
				let binding = binding.clone();
				let loaded = loaded.clone();
				Box::pin(async move {
					settings.verify_source(&loaded, &cancellation)?;
					let binding = validate.call((binding, cancellation.clone())).await?;

					let prepared = environment
						.prepare_export_port(root, binding.clone())
						.call((output, include_saves, cancellation))
						.await?;

					let original_publish = prepared.publish;
					let publish = Arc::new(move |files, cancellation| {
						let settings = settings.clone();
						let validate = validate.clone();
						let binding = binding.clone();
						let loaded = loaded.clone();
						let original_publish = original_publish.clone();
						Box::pin(async move {
							settings.verify_source(&loaded, &cancellation)?;
							let current = validate
								.call((binding.clone(), cancellation.clone()))
								.await?;

							if current != binding {
								return Err(report!(ErrorMarker::environment_invalid(
									Some("export_source_changed")
								)));
							}

							original_publish.call((files, cancellation)).await
						}) as PortFuture<_>
					});

					Ok(PreparedExport {
						files: prepared.files,
						publish,
					})
				}) as PortFuture<_>
			}),
		}
	}
}
