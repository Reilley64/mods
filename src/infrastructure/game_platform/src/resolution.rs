use crate::GamePlatformAdapter;
use crate::steam;
use application::ErrorCode;
use application::ErrorMarker;
use application::ports::GameInstallationSource;
use application::ports::ResolvedGameInstallation;
use domain::GameBinding;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::report;
use tokio_util::sync::CancellationToken;

impl GamePlatformAdapter {
	pub(crate) async fn resolve(
		&self,
		explicit: Option<GameInstallationPath>,
		environment: Option<GameInstallationPath>,
		cancellation: &CancellationToken,
	) -> Result<ResolvedGameInstallation, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		if let Some(path) = explicit {
			let binding = self.validate(path).await?;
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			return Ok(ResolvedGameInstallation {
				binding,
				source: GameInstallationSource::Explicit,
			});
		}
		if let Some(path) = environment {
			let binding = self.validate(path).await?;
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			return Ok(ResolvedGameInstallation {
				binding,
				source: GameInstallationSource::Environment,
			});
		}

		let mut first_invalid = None;
		match steam::discover(&self.steam_roots, cancellation).await {
			Ok(Some(binding)) => {
				return Ok(ResolvedGameInstallation {
					binding,
					source: GameInstallationSource::Steam,
				});
			}
			Ok(None) => {}
			Err(error) if error.current_context().code() == ErrorCode::GameInstallInvalid => {
				first_invalid = Some(error);
			}
			Err(error) => return Err(error),
		}
		for hint in self.bethesda_hints.iter() {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

			match steam::validate(hint).await {
				Ok(binding) => {
					return Ok(ResolvedGameInstallation {
						binding,
						source: GameInstallationSource::BethesdaRegistryFallback,
					});
				}
				Err(error) if first_invalid.is_none() => first_invalid = Some(error),
				Err(_) => {}
			}
		}
		if let Some(error) = first_invalid {
			return Err(error);
		}
		Err(report!(ErrorMarker::game_install_not_found()))
	}

	pub(crate) async fn validate(&self, path: GameInstallationPath) -> Result<GameBinding, ErrorMarker> {
		steam::validate(path.as_path()).await
	}

	pub(crate) async fn validate_effective(&self, expected: GameBinding) -> Result<GameBinding, ErrorMarker> {
		self.validate(expected.game_directory().clone()).await
	}
}

#[cfg(test)]
mod tests {
	use super::GameInstallationSource;
	use super::GamePlatformAdapter;
	use crate::adapter::KnownFolderSource;
	use application::ErrorCode;
	use domain::GameInstallationPath;
	use rootcause::Result;
	use std::fs;
	use std::io::Error as IoError;
	use std::path::Path;
	use std::path::PathBuf;
	use std::sync::Arc;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn cancelled_resolution_stops_before_validation() -> Result<()> {
		let (_temp, game) = fixture()?;
		let path = GameInstallationPath::new(game)?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = adapter_without_sources().resolve(Some(path), None, &cancellation).await;
		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::OperationCancelled),
		);
		Ok(())
	}

	#[tokio::test]
	async fn explicit_precedes_environment_and_discovery() -> Result<()> {
		let (_temp, game) = fixture()?;
		let path = GameInstallationPath::new(game)?;
		let resolved = adapter_without_sources()
			.resolve(Some(path.clone()), Some(path), &CancellationToken::new())
			.await?;
		assert_eq!(resolved.source, GameInstallationSource::Explicit);
		Ok(())
	}

	#[tokio::test]
	async fn automatic_discovery_precedes_bethesda_hint_and_fallback_is_typed() -> Result<()> {
		let (steam_fixture, steam_game) = fixture()?;
		let (bethesda_fixture, bethesda_game) = fixture()?;
		let steam_root = steam_game
			.parent()
			.and_then(Path::parent)
			.and_then(Path::parent)
			.ok_or_else(|| IoError::other("missing steam root"))?
			.to_path_buf();
		let resolved = adapter_with_discovery(vec![steam_root], vec![bethesda_game.clone()])
			.resolve(None, None, &CancellationToken::new())
			.await?;
		assert_eq!(resolved.source, GameInstallationSource::Steam);
		assert_eq!(resolved.binding.game_directory().as_path(), steam_game);
		drop(steam_fixture);

		let fallback = adapter_with_discovery(Vec::new(), vec![bethesda_game])
			.resolve(None, None, &CancellationToken::new())
			.await?;
		assert_eq!(fallback.source, GameInstallationSource::BethesdaRegistryFallback);
		drop(bethesda_fixture);
		Ok(())
	}

	#[tokio::test]
	async fn libraryfolders_path_participates_in_discovery() -> Result<()> {
		let (library, game) = fixture()?;
		let primary = TempDir::new()?;
		let primary_root = fs::canonicalize(primary.path())?;
		fs::create_dir_all(primary_root.join("steamapps"))?;
		let library_root = game
			.parent()
			.and_then(Path::parent)
			.and_then(Path::parent)
			.ok_or_else(|| IoError::other("missing library"))?;
		let library_path = library_root.to_string_lossy().replace('\\', "\\\\");
		fs::write(
			primary_root.join("steamapps/libraryfolders.vdf"),
			format!("\"libraryfolders\"\n{{\n\"1\"\n{{\n\"path\" \"{library_path}\"\n}}\n}}"),
		)?;
		let resolved = adapter_with_discovery(vec![primary_root], Vec::new())
			.resolve(None, None, &CancellationToken::new())
			.await?;
		assert_eq!(resolved.source, GameInstallationSource::Steam);
		assert_eq!(resolved.binding.game_directory().as_path(), fs::canonicalize(game)?);
		drop(library);
		Ok(())
	}

	fn fixture() -> Result<(TempDir, PathBuf)> {
		let temp = TempDir::new()?;
		let game = fs::canonicalize(temp.path())?.join("steam/steamapps/common/Fallout New Vegas");
		fs::create_dir_all(game.join("Data"))?;
		fs::write(game.join("FalloutNV.exe"), b"exe")?;
		fs::write(game.join("Fallout_default.ini"), b"[Archive]\n")?;
		fs::write(
			game.parent()
				.and_then(Path::parent)
				.ok_or_else(|| IoError::other("missing steamapps"))?
				.join("appmanifest_22380.acf"),
			concat!(
				"\"AppState\"\n{\n",
				"\"appid\" \"22380\"\n",
				"\"buildid\" \"88\"\n",
				"\"installdir\" \"Fallout New Vegas\"\n}"
			),
		)?;
		Ok((temp, game))
	}

	fn adapter_without_sources() -> GamePlatformAdapter {
		adapter_with_discovery(Vec::new(), Vec::new())
	}

	fn adapter_with_discovery(steam_roots: Vec<PathBuf>, bethesda_hints: Vec<PathBuf>) -> GamePlatformAdapter {
		GamePlatformAdapter {
			steam_roots: Arc::new(steam_roots),
			bethesda_hints: Arc::new(bethesda_hints),
			known_folders: KnownFolderSource::System,
		}
	}
}
