use crate::GamePlatformAdapter;
use crate::cancellation::check_cancelled;
use application::ports::LoadProfileSources;
use application::ports::PortFuture;
use application::ports::ResolveGameInstallation;
use application::ports::ValidateEffectiveBinding;
use application::ports::ValidateGameDirectory;
use domain::EnvironmentRoot;
use std::sync::Arc;

impl GamePlatformAdapter {
	pub fn resolve_port(&self) -> ResolveGameInstallation {
		let adapter = self.clone();
		Arc::new(move |explicit, environment, root, cancellation| {
			let result = adapter.resolve(explicit, environment, &root, &cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn profile_sources_port(&self) -> LoadProfileSources {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let result = adapter.load_profile_sources(&binding, &cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn validate_directory_port(&self, root: EnvironmentRoot) -> ValidateGameDirectory {
		let adapter = self.clone();
		Arc::new(move |path, cancellation| {
			let result =
				check_cancelled(&cancellation).and_then(|()| adapter.validate_with_root(path, &root));
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn validate_effective_port(&self, root: EnvironmentRoot) -> ValidateEffectiveBinding {
		let adapter = self.clone();
		Arc::new(move |binding| {
			let result = adapter.validate_effective(binding, &root);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}
}
