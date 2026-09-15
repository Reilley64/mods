use super::InitializeEnvironmentWarning;
use crate::errors::ErrorMarker;
use crate::ports::AssessInitializationTarget;
use crate::ports::GameInstallationSource;
use crate::ports::InitializationPlan;
use crate::ports::LoadProfileSources;
use crate::ports::ProfileFileRecord;
use crate::ports::PublishEnvironment;
use crate::ports::ReadInitializationGameOverride;
use crate::ports::RecoverEnvironment;
use crate::ports::ResolveGameInstallation;
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
	pub recover_environment: RecoverEnvironment,
	pub assess_target: AssessInitializationTarget,
	pub read_game_override: ReadInitializationGameOverride,
	pub resolve_game_installation: ResolveGameInstallation,
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
		.recover_environment
		.call((environment_root.clone(), cancellation.clone()))
		.await
		.context(InitializeEnvironmentError)?;

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

	let resolved = dependencies
		.resolve_game_installation
		.call((
			game_installation,
			game_override,
			environment_root.clone(),
			cancellation.clone(),
		))
		.await
		.context(InitializeEnvironmentError)?;

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
	use super::*;
	use crate::ErrorCode;
	use crate::ports::InitializationProfileSources;
	use crate::ports::InitializationTargetAssessment;
	use crate::ports::PortFuture;
	use crate::ports::ProfileFileDisposition;
	use crate::ports::RecoveryOutcome;
	use crate::ports::ResolvedGameInstallation;
	use domain::SteamBuildId;
	use std::env::temp_dir;
	use std::error::Error;
	use std::result::Result as StdResult;
	use std::sync::Arc;
	use std::sync::atomic::AtomicUsize;
	use std::sync::atomic::Ordering;

	#[tokio::test]
	async fn use_case_returns_fixed_output_and_fallback_warning_through_ports() -> StdResult<(), Box<dyn Error>> {
		let root = EnvironmentRoot::new(temp_dir().join("application-init-test"))
			.map_err(|_| "invalid test environment root")?;
		let game = GameInstallationPath::new(temp_dir().join("fnv")).map_err(|_| "invalid test game path")?;
		let binding = GameBinding::new(game, SteamBuildId::new(4).map_err(|_| "invalid test build ID")?);
		let dependencies = InitializeEnvironmentDependencies {
			recover_environment: Arc::new(|_, _| {
				Box::pin(async { Ok(RecoveryOutcome::NothingToRecover) }) as PortFuture<_>
			}),
			assess_target: Arc::new(|_, _| {
				Box::pin(async { Ok(InitializationTargetAssessment::Available) }) as PortFuture<_>
			}),
			read_game_override: Arc::new(|| Box::pin(async { Ok(None) }) as PortFuture<_>),
			resolve_game_installation: Arc::new({
				let binding = binding.clone();
				move |_, _, _, _| {
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
		assert_eq!(output.game_binding.observed_build_id().get(), 4);
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
	async fn cancellation_after_recovery_is_typed_and_stops_before_assessment() -> StdResult<(), Box<dyn Error>> {
		let root = EnvironmentRoot::new(temp_dir().join("cancelled-application-init-test"))
			.map_err(|_| "invalid test environment root")?;
		let assessment_calls = Arc::new(AtomicUsize::new(0));
		let dependencies = InitializeEnvironmentDependencies {
			recover_environment: Arc::new(|_, cancellation| {
				cancellation.cancel();
				Box::pin(async { Ok(RecoveryOutcome::NothingToRecover) }) as PortFuture<_>
			}),
			assess_target: Arc::new({
				let assessment_calls = assessment_calls.clone();
				move |_, _| {
					assessment_calls.fetch_add(1, Ordering::SeqCst);
					Box::pin(async { Ok(InitializationTargetAssessment::Available) })
						as PortFuture<_>
				}
			}),
			read_game_override: Arc::new(|| Box::pin(async { Ok(None) }) as PortFuture<_>),
			resolve_game_installation: Arc::new(|_, _, _, _| {
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

		let Some(report) = initialize_environment(dependencies, root, None, CancellationToken::new())
			.await
			.err()
		else {
			return Err("cancelled initialization must fail".into());
		};

		assert_eq!(report.current_context(), &InitializeEnvironmentError);
		assert!(report.iter_reports().any(|report| {
			report.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::OperationCancelled)
		}));
		assert_eq!(assessment_calls.load(Ordering::SeqCst), 0);
		Ok(())
	}
}
