use crate::GamePlatformAdapter;
use crate::separation::prove_separation;
use crate::steam;
use application::ErrorCode;
use application::ErrorMarker;
use application::ports::GameInstallationSource;
use application::ports::ResolvedGameInstallation;
use domain::EnvironmentRoot;
use domain::GameBinding;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::report;
use tokio_util::sync::CancellationToken;

impl GamePlatformAdapter {
	pub(crate) fn resolve(
		&self,
		explicit: Option<GameInstallationPath>,
		environment: Option<GameInstallationPath>,
		environment_root: &EnvironmentRoot,
		cancellation: &CancellationToken,
	) -> Result<ResolvedGameInstallation, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		if let Some(path) = explicit {
			let binding = self.validate_with_root(path, environment_root)?;
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			return Ok(ResolvedGameInstallation {
				binding,
				source: GameInstallationSource::Explicit,
			});
		}
		if let Some(path) = environment {
			let binding = self.validate_with_root(path, environment_root)?;
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			return Ok(ResolvedGameInstallation {
				binding,
				source: GameInstallationSource::Environment,
			});
		}

		let mut first_invalid = None;
		match steam::discover(&self.steam_roots, cancellation) {
			Ok(Some(binding)) => {
				prove_separation(environment_root, binding.game_directory())?;
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

			match steam::validate(hint).and_then(|binding| {
				prove_separation(environment_root, binding.game_directory())?;
				Ok(binding)
			}) {
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

	pub(crate) fn validate_with_root(
		&self,
		path: GameInstallationPath,
		root: &EnvironmentRoot,
	) -> Result<GameBinding, ErrorMarker> {
		let binding = steam::validate(path.as_path())?;
		prove_separation(root, binding.game_directory())?;
		Ok(binding)
	}

	pub(crate) fn validate_effective(
		&self,
		expected: GameBinding,
		root: &EnvironmentRoot,
	) -> Result<GameBinding, ErrorMarker> {
		let actual = self.validate_with_root(expected.game_directory().clone(), root)?;
		if actual.observed_build_id() != expected.observed_build_id() {
			return Err(report!(ErrorMarker::game_build_mismatch(
				expected.observed_build_id().get(),
				actual.observed_build_id().get(),
			)));
		}
		Ok(actual)
	}
}

#[cfg(test)]
mod tests {
	use super::GameInstallationSource;
	use super::GamePlatformAdapter;
	use crate::adapter::KnownFolderSource;
	use application::ErrorCode;
	use domain::EnvironmentRoot;
	use domain::GameInstallationPath;
	use rootcause::Result;
	use std::env;
	use std::fs;
	use std::io::Error as IoError;
	use std::path::Path;
	use std::path::PathBuf;
	use std::sync::Arc;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn cancelled_resolution_stops_before_validation() -> Result<()> {
		let (temp, game) = fixture()?;
		let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?.join("environment"))?;
		let path = GameInstallationPath::new(game)?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = adapter_without_sources().resolve(Some(path), None, &root, &cancellation);
		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::OperationCancelled),
		);
		Ok(())
	}

	#[test]
	fn explicit_precedes_environment_and_discovery() -> Result<()> {
		let (temp, game) = fixture()?;
		let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?.join("environment"))?;
		let path = GameInstallationPath::new(game)?;
		let resolved = adapter_without_sources().resolve(
			Some(path.clone()),
			Some(path),
			&root,
			&CancellationToken::new(),
		)?;
		assert_eq!(resolved.source, GameInstallationSource::Explicit);
		Ok(())
	}

	#[test]
	fn automatic_discovery_precedes_bethesda_hint_and_fallback_is_typed() -> Result<()> {
		let (steam_fixture, steam_game) = fixture()?;
		let (bethesda_fixture, bethesda_game) = fixture()?;
		let root = EnvironmentRoot::new(
			fs::canonicalize(env::temp_dir())?.join("game-platform-discovery-environment"),
		)?;
		let steam_root = steam_game
			.parent()
			.and_then(Path::parent)
			.and_then(Path::parent)
			.ok_or_else(|| IoError::other("missing steam root"))?
			.to_path_buf();
		let resolved = adapter_with_discovery(vec![steam_root], vec![bethesda_game.clone()]).resolve(
			None,
			None,
			&root,
			&CancellationToken::new(),
		)?;
		assert_eq!(resolved.source, GameInstallationSource::Steam);
		assert_eq!(resolved.binding.observed_build_id().get(), 88);
		drop(steam_fixture);

		let fallback = adapter_with_discovery(Vec::new(), vec![bethesda_game]).resolve(
			None,
			None,
			&root,
			&CancellationToken::new(),
		)?;
		assert_eq!(fallback.source, GameInstallationSource::BethesdaRegistryFallback);
		drop(bethesda_fixture);
		Ok(())
	}

	#[test]
	fn libraryfolders_path_participates_in_discovery() -> Result<()> {
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
		let root = EnvironmentRoot::new(primary_root.join("environment"))?;
		let resolved = adapter_with_discovery(vec![primary_root], Vec::new()).resolve(
			None,
			None,
			&root,
			&CancellationToken::new(),
		)?;
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
