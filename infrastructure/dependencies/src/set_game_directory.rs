use crate::Resources;
use application::ports::PortFuture;
use application::ports::RecoverSettingsMutation;
use application::settings::SetGameDirectoryDependencies;
use std::sync::Arc;

impl Resources {
	pub fn set_game_directory_dependencies(&self) -> SetGameDirectoryDependencies {
		let environment = self.environment.clone();
		let root = self.root.clone();
		let recover: RecoverSettingsMutation = Arc::new(move |cancellation| {
			let result = environment.recover(&root, &cancellation).map(|_| ());
			Box::pin(async move { result }) as PortFuture<_>
		});
		SetGameDirectoryDependencies {
			recover_environment: recover,
			validate_game_directory: self.game_platform.validate_directory_port(self.root.clone()),
			store_game_binding: self.settings.store_port(),
			validate_effective_binding: self.game_platform.validate_effective_port(self.root.clone()),
		}
	}
}
