use crate::GamePlatformAdapter;
use application::ErrorMarker;
use application::ports::LoadProfileSources;
use application::ports::PortFuture;
use application::ports::ReadGameVersion;
use application::ports::ReadXnvseVersion;
use application::ports::ResolveGameInstallation;
use application::ports::ValidateEffectiveBinding;
use application::ports::ValidateGameDirectory;
use domain::EnvironmentRoot;
use rootcause::report;
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

	pub fn game_version_port(&self) -> ReadGameVersion {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let result = adapter.read_game_version(&binding, &cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn xnvse_version_port(&self) -> ReadXnvseVersion {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let result = adapter.read_xnvse_version(&binding, &cancellation);
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn validate_directory_port(&self, root: EnvironmentRoot) -> ValidateGameDirectory {
		let adapter = self.clone();
		Arc::new(move |path, cancellation| {
			let result = (|| {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				let binding = adapter.validate_with_root(path, &root)?;

				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				Ok(binding)
			})();
			Box::pin(async move { result }) as PortFuture<_>
		})
	}

	pub fn validate_effective_port(&self, root: EnvironmentRoot) -> ValidateEffectiveBinding {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let result = (|| {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				let binding = adapter.validate_effective(binding, &root)?;

				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				Ok(binding)
			})();
			Box::pin(async move { result }) as PortFuture<_>
		})
	}
}

#[cfg(test)]
mod tests {
	use super::GamePlatformAdapter;
	use crate::adapter::KnownFolderSource;
	use application::ErrorCode;
	use application::ErrorMarker;
	use application::ports::PortFuture;
	use domain::EnvironmentRoot;
	use domain::GameInstallationPath;
	use rootcause::Result;
	use rootcause::report;
	use std::path::PathBuf;
	use std::sync::Arc;
	use std::task::Context;
	use std::task::Poll;
	use std::task::Waker;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn directory_validation_port_honors_forwarded_cancellation() -> Result<()> {
		let missing = if cfg!(windows) {
			PathBuf::from(r"C:\game-that-must-not-be-opened")
		} else {
			PathBuf::from("/game-that-must-not-be-opened")
		};
		let root = EnvironmentRoot::new(missing.join("environment"))?;
		let path = GameInstallationPath::new(missing)?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = ready(adapter_without_sources()
			.validate_directory_port(root)
			.call((path, cancellation)));

		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::OperationCancelled),
		);
		Ok(())
	}

	fn adapter_without_sources() -> GamePlatformAdapter {
		GamePlatformAdapter {
			steam_roots: Arc::new(Vec::new()),
			bethesda_hints: Arc::new(Vec::new()),
			known_folders: KnownFolderSource::System,
		}
	}

	fn ready<T>(mut future: PortFuture<T>) -> Result<T, ErrorMarker> {
		let waker = Waker::noop();
		let mut context = Context::from_waker(waker);
		let Poll::Ready(result) = future.as_mut().poll(&mut context) else {
			return Err(report!(ErrorMarker::io_failure()));
		};
		result
	}
}
