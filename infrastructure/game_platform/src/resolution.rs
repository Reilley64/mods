use crate::GamePlatformAdapter;
use crate::cancellation::check_cancelled;
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
		check_cancelled(cancellation)?;
		if let Some(path) = explicit {
			return self.validate_with_root(path, environment_root).map(|binding| {
				ResolvedGameInstallation {
					binding,
					source: GameInstallationSource::Explicit,
				}
			});
		}
		if let Some(path) = environment {
			return self.validate_with_root(path, environment_root).map(|binding| {
				ResolvedGameInstallation {
					binding,
					source: GameInstallationSource::Environment,
				}
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
			check_cancelled(cancellation)?;
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
		match first_invalid {
			Some(error) => Err(error),
			None => Err(report!(ErrorMarker::game_install_not_found())),
		}
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
