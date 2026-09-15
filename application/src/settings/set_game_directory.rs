use crate::errors::ErrorMarker;
use crate::ports::RecoverSettingsMutation;
use crate::ports::StoreGameBinding;
use crate::ports::ValidateEffectiveBinding;
use crate::ports::ValidateGameDirectory;
use crate::settings::types::EffectiveBinding;
use crate::settings::types::SetGameDirectoryWarning;
use crate::settings::types::SettingSource;
use domain::GameInstallationPath;
use domain::SteamBuildId;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct SetGameDirectoryDependencies {
	pub recover_environment: RecoverSettingsMutation,
	pub validate_game_directory: ValidateGameDirectory,
	pub store_game_binding: StoreGameBinding,
	pub validate_effective_binding: ValidateEffectiveBinding,
}

#[derive(Debug, Clone)]
pub struct SetGameDirectoryOutput {
	pub stored_value: GameInstallationPath,
	pub stored_observed_build_id: SteamBuildId,
	pub effective_value: GameInstallationPath,
	pub source: SettingSource,
	pub shadowed: bool,
	pub effective_binding: EffectiveBinding,
	pub warnings: Vec<SetGameDirectoryWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetGameDirectoryError;

impl fmt::Display for SetGameDirectoryError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to set game directory")
	}
}

#[tracing::instrument(skip_all)]
pub async fn set_game_directory(
	dependencies: SetGameDirectoryDependencies,
	game_directory: GameInstallationPath,
	cancellation: CancellationToken,
) -> Result<SetGameDirectoryOutput, SetGameDirectoryError> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(SetGameDirectoryError));
	}

	dependencies
		.recover_environment
		.call((cancellation.clone(),))
		.await
		.context(SetGameDirectoryError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(SetGameDirectoryError));
	}

	let stored_binding = dependencies
		.validate_game_directory
		.call((game_directory, cancellation.clone()))
		.await
		.context(SetGameDirectoryError)?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()).context(SetGameDirectoryError));
	}

	let outcome = dependencies
		.store_game_binding
		.call((stored_binding, cancellation))
		.await
		.context(SetGameDirectoryError)?;

	// The manifest replacement above is the final irreversible mutation. Cancellation is now too late, so the
	// post-commit effective-binding check deliberately runs to completion and returns the committed result.
	let (effective_binding, warnings) = if outcome.shadowed {
		match dependencies
			.validate_effective_binding
			.call((outcome.effective.clone(),))
			.await
		{
			Ok(_) => (EffectiveBinding::Valid, Vec::new()),
			Err(report) => {
				let marker = report.current_context().clone();
				let Some((expected_build_id, actual_build_id)) = marker.build_ids() else {
					return Err(report.context(SetGameDirectoryError));
				};
				(
					EffectiveBinding::Invalid,
					vec![SetGameDirectoryWarning::EffectiveGameBindingInvalid {
						variable: "MODS_GAME_DIR",
						expected_build_id,
						actual_build_id,
					}],
				)
			}
		}
	} else {
		(EffectiveBinding::Valid, Vec::new())
	};

	Ok(SetGameDirectoryOutput {
		stored_value: outcome.stored.game_directory().clone(),
		stored_observed_build_id: outcome.stored.observed_build_id(),
		effective_value: outcome.effective.game_directory().clone(),
		source: outcome.source,
		shadowed: outcome.shadowed,
		effective_binding,
		warnings,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ErrorCode;
	use crate::errors::ErrorMarker;
	use crate::ports::PortFuture;
	use crate::ports::StoredAndEffectiveBinding;
	use crate::settings::SettingSource;
	use domain::GameBinding;
	use rootcause::report;
	use std::env::temp_dir;
	use std::error::Error;
	use std::result::Result as StdResult;
	use std::sync::Arc;
	use std::sync::atomic::AtomicUsize;
	use std::sync::atomic::Ordering;

	#[tokio::test]
	async fn shadowed_invalid_effective_binding_is_a_success_with_warning() -> StdResult<(), Box<dyn Error>> {
		let stored = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("stored")).map_err(|_| "invalid test game path")?,
			SteamBuildId::new(2).map_err(|_| "invalid test build ID")?,
		);
		let effective = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("effective"))
				.map_err(|_| "invalid test game path")?,
			SteamBuildId::new(2).map_err(|_| "invalid test build ID")?,
		);
		let order = Arc::new(AtomicUsize::new(0));
		let validation_observed_order = Arc::new(AtomicUsize::new(0));
		let dependencies = SetGameDirectoryDependencies {
			recover_environment: Arc::new(|_| Box::pin(async { Ok(()) }) as PortFuture<_>),
			validate_game_directory: Arc::new({
				let stored = stored.clone();
				move |_, _| {
					let value = stored.clone();
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
			store_game_binding: Arc::new({
				let stored = stored.clone();
				let effective = effective.clone();
				let order = order.clone();
				move |_, _| {
					order.store(1, Ordering::SeqCst);
					let value = StoredAndEffectiveBinding {
						stored: stored.clone(),
						effective: effective.clone(),
						source: SettingSource::Environment {
							variable: "MODS_GAME_DIR",
						},
						shadowed: true,
					};
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
			validate_effective_binding: Arc::new({
				let order = order.clone();
				let validation_observed_order = validation_observed_order.clone();
				move |_| {
					validation_observed_order.store(order.load(Ordering::SeqCst), Ordering::SeqCst);
					order.store(2, Ordering::SeqCst);
					Box::pin(async { Err(report!(ErrorMarker::game_build_mismatch(2, 1))) })
						as PortFuture<_>
				}
			}),
		};
		let output =
			set_game_directory(dependencies, stored.game_directory().clone(), CancellationToken::new())
				.await
				.map_err(|_| "set failed")?;
		assert_eq!(output.effective_binding, EffectiveBinding::Invalid);
		assert_eq!(output.warnings.len(), 1);
		assert_eq!(validation_observed_order.load(Ordering::SeqCst), 1);
		assert_eq!(order.load(Ordering::SeqCst), 2);
		assert!(matches!(
			&output.warnings[0],
			SetGameDirectoryWarning::EffectiveGameBindingInvalid {
				variable: "MODS_GAME_DIR",
				expected_build_id: 2,
				actual_build_id: 1
			}
		));
		Ok(())
	}

	#[tokio::test]
	async fn shadowed_non_mismatch_failure_is_reported_after_store() -> StdResult<(), Box<dyn Error>> {
		let stored = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("stored-not-found"))
				.map_err(|_| "invalid test game path")?,
			SteamBuildId::new(2).map_err(|_| "invalid test build ID")?,
		);
		let effective = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("effective-not-found"))
				.map_err(|_| "invalid test game path")?,
			SteamBuildId::new(2).map_err(|_| "invalid test build ID")?,
		);
		let stored_order = Arc::new(AtomicUsize::new(0));
		let validated_order = Arc::new(AtomicUsize::new(0));
		let dependencies = SetGameDirectoryDependencies {
			recover_environment: Arc::new(|_| Box::pin(async { Ok(()) }) as PortFuture<_>),
			validate_game_directory: Arc::new({
				let stored = stored.clone();
				move |_, _| {
					let value = stored.clone();
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
			store_game_binding: Arc::new({
				let stored = stored.clone();
				let effective = effective.clone();
				let stored_order = stored_order.clone();
				move |_, _| {
					stored_order.store(1, Ordering::SeqCst);
					let value = StoredAndEffectiveBinding {
						stored: stored.clone(),
						effective: effective.clone(),
						source: SettingSource::Environment {
							variable: "MODS_GAME_DIR",
						},
						shadowed: true,
					};
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
			validate_effective_binding: Arc::new({
				let stored_order = stored_order.clone();
				let validated_order = validated_order.clone();
				move |_| {
					validated_order.store(stored_order.load(Ordering::SeqCst), Ordering::SeqCst);
					Box::pin(async { Err(report!(ErrorMarker::game_install_not_found())) })
						as PortFuture<_>
				}
			}),
		};
		let result =
			set_game_directory(dependencies, stored.game_directory().clone(), CancellationToken::new())
				.await;
		assert!(matches!(result, Err(ref report) if report.iter_reports().any(|report| {
		    report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::GameInstallNotFound)
		})));
		assert_eq!(stored_order.load(Ordering::SeqCst), 1);
		assert_eq!(validated_order.load(Ordering::SeqCst), 1);
		Ok(())
	}

	#[tokio::test]
	async fn cancellation_after_recovery_is_typed_and_stops_before_validation() -> StdResult<(), Box<dyn Error>> {
		let game_directory = GameInstallationPath::new(temp_dir().join("cancelled-set"))
			.map_err(|_| "invalid test game path")?;
		let validation_calls = Arc::new(AtomicUsize::new(0));
		let dependencies = SetGameDirectoryDependencies {
			recover_environment: Arc::new(|cancellation| {
				cancellation.cancel();
				Box::pin(async { Ok(()) }) as PortFuture<_>
			}),
			validate_game_directory: Arc::new({
				let validation_calls = validation_calls.clone();
				move |_, _| {
					validation_calls.fetch_add(1, Ordering::SeqCst);
					Box::pin(async { Err(report!(ErrorMarker::game_install_invalid())) })
						as PortFuture<_>
				}
			}),
			store_game_binding: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::environment_invalid(None))) })
					as PortFuture<_>
			}),
			validate_effective_binding: Arc::new(|binding| {
				Box::pin(async move { Ok(binding) }) as PortFuture<_>
			}),
		};

		let Some(report) = set_game_directory(dependencies, game_directory, CancellationToken::new())
			.await
			.err()
		else {
			return Err("cancelled set must fail".into());
		};

		assert_eq!(report.current_context(), &SetGameDirectoryError);
		assert!(report.iter_reports().any(|report| {
			report.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::OperationCancelled)
		}));
		assert_eq!(validation_calls.load(Ordering::SeqCst), 0);
		Ok(())
	}
}
