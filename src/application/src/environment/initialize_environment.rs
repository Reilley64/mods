use super::InitializeEnvironmentWarning;
use crate::errors::ErrorMarker;
use crate::ports::AssessInitializationTarget;
use crate::ports::DiscoverGameInstallation;
use crate::ports::GameInstallationSource;
use crate::ports::InitializationPlan;
use crate::ports::LoadProfileSources;
use crate::ports::ProfileFileRecord;
use crate::ports::PublishEnvironment;
use crate::ports::ReadInitializationGameOverride;
use crate::ports::ResolvedGameInstallation;
use crate::ports::ValidateGameDirectory;
use domain::EnvironmentRoot;
use domain::GameBinding;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct InitializeEnvironmentDependencies {
	pub assess_target: AssessInitializationTarget,
	pub read_game_override: ReadInitializationGameOverride,
	pub validate_game_directory: ValidateGameDirectory,
	pub discover_game_installation: DiscoverGameInstallation,
	pub load_profile_sources: LoadProfileSources,
	pub publish_environment: PublishEnvironment,
}

#[derive(Debug, Clone)]
pub struct InitializeEnvironmentOutput {
	pub game_binding: GameBinding,
	pub profile_files: Vec<ProfileFileRecord>,
	pub warnings: Vec<InitializeEnvironmentWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitializeEnvironmentError;

impl fmt::Display for InitializeEnvironmentError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to initialize environment")
	}
}

#[tracing::instrument(skip_all)]
pub async fn initialize_environment(
	dependencies: InitializeEnvironmentDependencies,
	environment_root: EnvironmentRoot,
	game_installation: Option<GameInstallationPath>,
	cancellation: CancellationToken,
) -> Result<InitializeEnvironmentOutput, InitializeEnvironmentError> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(InitializeEnvironmentError));
	}

	dependencies
		.assess_target
		.call((environment_root.clone(), cancellation.clone()))
		.await
		.context(InitializeEnvironmentError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(InitializeEnvironmentError));
	}

	let game_override = dependencies
		.read_game_override
		.call(())
		.await
		.context(InitializeEnvironmentError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(InitializeEnvironmentError));
	}

	let selected_installation = game_installation
		.map(|path| (path, GameInstallationSource::Explicit))
		.or_else(|| game_override.map(|path| (path, GameInstallationSource::Environment)));
	let resolved = if let Some((path, source)) = selected_installation {
		let binding = dependencies
			.validate_game_directory
			.call((path, cancellation.clone()))
			.await
			.context(InitializeEnvironmentError)?;
		ResolvedGameInstallation { binding, source }
	} else {
		dependencies
			.discover_game_installation
			.call((cancellation.clone(),))
			.await
			.context(InitializeEnvironmentError)?
	};

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(InitializeEnvironmentError));
	}

	let sources = dependencies
		.load_profile_sources
		.call((resolved.binding.clone(), cancellation.clone()))
		.await
		.context(InitializeEnvironmentError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(InitializeEnvironmentError));
	}

	let plan = InitializationPlan {
		game_binding: resolved.binding.clone(),
		profile_sources: sources,
	};

	let profile_files = dependencies
		.publish_environment
		.call((environment_root, plan, cancellation))
		.await
		.context(InitializeEnvironmentError)?;

	let warnings = if resolved.source == GameInstallationSource::BethesdaRegistryFallback {
		vec![InitializeEnvironmentWarning::BethesdaRegistryFallbackUsed]
	} else {
		Vec::new()
	};

	Ok(InitializeEnvironmentOutput {
		game_binding: resolved.binding,
		profile_files,
		warnings,
	})
}

#[cfg(test)]
mod tests {
	use super::InitializeEnvironmentDependencies;
	use super::InitializeEnvironmentError;
	use super::InitializeEnvironmentWarning;
	use super::initialize_environment;
	use crate::ErrorCode;
	use crate::ErrorMarker;
	use crate::ports::GameInstallationSource;
	use crate::ports::InitializationProfileSources;
	use crate::ports::InitializationTargetAssessment;
	use crate::ports::PortFuture;
	use crate::ports::ProfileFileDisposition;
	use crate::ports::ProfileFileRecord;
	use crate::ports::ResolvedGameInstallation;
	use domain::EnvironmentRoot;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use rootcause::report;
	use std::env::temp_dir;
	use std::error::Error;
	use std::result::Result as StdResult;
	use std::sync::Arc;
	use std::sync::Mutex;
	use std::sync::atomic::AtomicUsize;
	use std::sync::atomic::Ordering;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn use_case_returns_fixed_output_and_fallback_warning_through_ports() -> StdResult<(), Box<dyn Error>> {
		let root = EnvironmentRoot::new(temp_dir().join("application-init-test"))
			.map_err(|_| "invalid test environment root")?;
		let game = GameInstallationPath::new(temp_dir().join("fnv")).map_err(|_| "invalid test game path")?;
		let binding = GameBinding::new(game);
		let dependencies = InitializeEnvironmentDependencies {
			assess_target: Arc::new(|_, _| {
				Box::pin(async { Ok(InitializationTargetAssessment::Available) }) as PortFuture<_>
			}),
			read_game_override: Arc::new(|| Box::pin(async { Ok(None) }) as PortFuture<_>),
			validate_game_directory: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::game_install_invalid())) }) as PortFuture<_>
			}),
			discover_game_installation: Arc::new({
				let binding = binding.clone();
				move |_| {
					let binding = binding.clone();
					Box::pin(async move {
						Ok(ResolvedGameInstallation {
							binding,
							source: GameInstallationSource::BethesdaRegistryFallback,
						})
					}) as PortFuture<_>
				}
			}),
			load_profile_sources: Arc::new(|_, _| {
				Box::pin(async {
					Ok(InitializationProfileSources {
						files: Vec::new(),
						fallout_default_ini: Vec::new(),
					})
				}) as PortFuture<_>
			}),
			publish_environment: Arc::new(|_, _, _| {
				Box::pin(async {
					Ok(vec![ProfileFileRecord {
						name: "Fallout.ini",
						disposition: ProfileFileDisposition::SeededFromGame,
					}])
				}) as PortFuture<_>
			}),
		};
		let output = initialize_environment(dependencies, root, None, CancellationToken::new())
			.await
			.map_err(|_| "initialize failed")?;
		assert_eq!(output.game_binding, binding);
		assert_eq!(
			output.warnings,
			vec![InitializeEnvironmentWarning::BethesdaRegistryFallbackUsed]
		);
		assert_eq!(
			output.profile_files[0].disposition,
			ProfileFileDisposition::SeededFromGame
		);
		Ok(())
	}

	#[tokio::test]
	async fn cancellation_after_assessment_is_typed_and_stops_before_reading_override()
	-> StdResult<(), Box<dyn Error>> {
		let root = EnvironmentRoot::new(temp_dir().join("cancelled-application-init-test"))
			.map_err(|_| "invalid test environment root")?;
		let override_calls = Arc::new(AtomicUsize::new(0));
		let dependencies = InitializeEnvironmentDependencies {
			assess_target: Arc::new(|_, cancellation| {
				cancellation.cancel();
				Box::pin(async { Ok(InitializationTargetAssessment::Available) }) as PortFuture<_>
			}),
			read_game_override: Arc::new({
				let override_calls = override_calls.clone();
				move || {
					override_calls.fetch_add(1, Ordering::SeqCst);
					Box::pin(async { Ok(None) }) as PortFuture<_>
				}
			}),
			validate_game_directory: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::game_install_invalid())) }) as PortFuture<_>
			}),
			discover_game_installation: Arc::new(|_| {
				Box::pin(async { Err(report!(ErrorMarker::game_install_not_found())) }) as PortFuture<_>
			}),
			load_profile_sources: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::environment_invalid(None))) })
					as PortFuture<_>
			}),
			publish_environment: Arc::new(|_, _, _| {
				Box::pin(async { Err(report!(ErrorMarker::environment_invalid(None))) })
					as PortFuture<_>
			}),
		};

		let report = initialize_environment(dependencies, root, None, CancellationToken::new())
			.await
			.err()
			.ok_or("cancelled initialization must fail")?;

		assert_eq!(report.current_context(), &InitializeEnvironmentError);
		assert!(report.iter_reports().any(|report| {
			report.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::OperationCancelled)
		}));
		assert_eq!(override_calls.load(Ordering::SeqCst), 0);
		Ok(())
	}

	#[tokio::test]
	async fn explicit_path_precedes_override_and_discovery() -> StdResult<(), Box<dyn Error>> {
		let explicit = GameInstallationPath::new(temp_dir().join("explicit-fnv"))
			.map_err(|_| "invalid explicit path")?;
		let game_override = GameInstallationPath::new(temp_dir().join("override-fnv"))
			.map_err(|_| "invalid override path")?;
		let validated = Arc::new(Mutex::new(Vec::new()));

		let output = initialize_environment(
			selection_dependencies(Some(game_override), validated.clone()),
			test_root("explicit-application-init-test")?,
			Some(explicit.clone()),
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "initialize failed")?;

		assert_eq!(output.game_binding, GameBinding::new(explicit.clone()));
		assert!(output.warnings.is_empty());
		assert_eq!(*validated.lock().map_err(|_| "poisoned validation log")?, [explicit]);
		Ok(())
	}

	#[tokio::test]
	async fn override_is_validated_before_discovery() -> StdResult<(), Box<dyn Error>> {
		let game_override = GameInstallationPath::new(temp_dir().join("override-fnv"))
			.map_err(|_| "invalid override path")?;
		let validated = Arc::new(Mutex::new(Vec::new()));

		let output = initialize_environment(
			selection_dependencies(Some(game_override.clone()), validated.clone()),
			test_root("override-application-init-test")?,
			None,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "initialize failed")?;

		assert_eq!(output.game_binding, GameBinding::new(game_override.clone()));
		assert_eq!(
			*validated.lock().map_err(|_| "poisoned validation log")?,
			[game_override]
		);
		Ok(())
	}

	#[tokio::test]
	async fn discovery_runs_only_without_explicit_path_or_override() -> StdResult<(), Box<dyn Error>> {
		let validated = Arc::new(Mutex::new(Vec::new()));

		let output = initialize_environment(
			selection_dependencies(None, validated.clone()),
			test_root("discovery-application-init-test")?,
			None,
			CancellationToken::new(),
		)
		.await
		.map_err(|_| "initialize failed")?;

		assert_eq!(output.game_binding, GameBinding::new(discovered_path()?));
		assert!(validated.lock().map_err(|_| "poisoned validation log")?.is_empty());
		Ok(())
	}

	fn test_root(name: &str) -> StdResult<EnvironmentRoot, Box<dyn Error>> {
		Ok(EnvironmentRoot::new(temp_dir().join(name)).map_err(|_| "invalid test environment root")?)
	}

	fn discovered_path() -> StdResult<GameInstallationPath, Box<dyn Error>> {
		Ok(GameInstallationPath::new(temp_dir().join("discovered-fnv"))
			.map_err(|_| "invalid discovered path")?)
	}

	fn selection_dependencies(
		game_override: Option<GameInstallationPath>,
		validated: Arc<Mutex<Vec<GameInstallationPath>>>,
	) -> InitializeEnvironmentDependencies {
		InitializeEnvironmentDependencies {
			assess_target: Arc::new(|_, _| {
				Box::pin(async { Ok(InitializationTargetAssessment::Available) }) as PortFuture<_>
			}),
			read_game_override: Arc::new(move || {
				let game_override = game_override.clone();
				Box::pin(async move { Ok(game_override) }) as PortFuture<_>
			}),
			validate_game_directory: Arc::new(move |path, _| {
				let validated = validated.clone();
				Box::pin(async move {
					validated
						.lock()
						.map_err(|_| report!(ErrorMarker::environment_invalid(None)))?
						.push(path.clone());
					Ok(GameBinding::new(path))
				}) as PortFuture<_>
			}),
			discover_game_installation: Arc::new(|_| {
				Box::pin(async {
					let path = GameInstallationPath::new(temp_dir().join("discovered-fnv"))
						.map_err(|_| report!(ErrorMarker::game_install_invalid()))?;
					Ok(ResolvedGameInstallation {
						binding: GameBinding::new(path),
						source: GameInstallationSource::Steam,
					})
				}) as PortFuture<_>
			}),
			load_profile_sources: Arc::new(|_, _| {
				Box::pin(async {
					Ok(InitializationProfileSources {
						files: Vec::new(),
						fallout_default_ini: Vec::new(),
					})
				}) as PortFuture<_>
			}),
			publish_environment: Arc::new(|_, _, _| Box::pin(async { Ok(Vec::new()) }) as PortFuture<_>),
		}
	}
}
