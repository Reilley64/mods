use crate::GamePlatformAdapter;
use application::ErrorMarker;
use application::ports::LoadProfileSources;
use application::ports::PortFuture;
use application::ports::ReadGameVersion;
use application::ports::ReadXnvseVersion;
use application::ports::ResolveGameInstallation;
use application::ports::ValidateEffectiveBinding;
use application::ports::ValidateGameDirectory;
use rootcause::report;
use std::sync::Arc;

impl GamePlatformAdapter {
	pub fn resolve_port(&self) -> ResolveGameInstallation {
		let adapter = self.clone();
		Arc::new(move |explicit, environment, _root, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.resolve(explicit, environment, &cancellation).await })
				as PortFuture<_>
		})
	}

	pub fn profile_sources_port(&self) -> LoadProfileSources {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.load_profile_sources(&binding, &cancellation).await })
				as PortFuture<_>
		})
	}

	pub fn game_version_port(&self) -> ReadGameVersion {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.read_game_version(&binding, &cancellation).await })
				as PortFuture<_>
		})
	}

	pub fn xnvse_version_port(&self) -> ReadXnvseVersion {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move { adapter.read_xnvse_version(&binding, &cancellation).await })
				as PortFuture<_>
		})
	}

	pub fn validate_directory_port(&self) -> ValidateGameDirectory {
		let adapter = self.clone();
		Arc::new(move |path, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				let binding = adapter.validate(path).await?;

				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				Ok(binding)
			}) as PortFuture<_>
		})
	}

	pub fn validate_effective_port(&self) -> ValidateEffectiveBinding {
		let adapter = self.clone();
		Arc::new(move |binding, cancellation| {
			let adapter = adapter.clone();
			Box::pin(async move {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				let binding = adapter.validate_effective(binding).await?;

				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}

				Ok(binding)
			}) as PortFuture<_>
		})
	}
}

#[cfg(test)]
mod tests {
	use super::GamePlatformAdapter;
	use crate::adapter::KnownFolderSource;
	use application::ErrorCode;
	use domain::GameInstallationPath;
	use rootcause::Result;
	use std::path::PathBuf;
	use std::sync::Arc;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn directory_validation_port_honors_forwarded_cancellation() -> Result<()> {
		let missing = if cfg!(windows) {
			PathBuf::from(r"C:\game-that-must-not-be-opened")
		} else {
			PathBuf::from("/game-that-must-not-be-opened")
		};
		let path = GameInstallationPath::new(missing)?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = adapter_without_sources()
			.validate_directory_port()
			.call((path, cancellation))
			.await;

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
}
