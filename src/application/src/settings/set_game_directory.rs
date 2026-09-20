use crate::errors::ErrorCode;
use crate::ports::CheckSettingsReadiness;
use crate::ports::PreviewGameBinding;
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
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct SetGameDirectoryDependencies {
	pub check_settings_readiness: CheckSettingsReadiness,
	pub validate_game_directory: ValidateGameDirectory,
	pub preview_game_binding: PreviewGameBinding,
	pub validate_effective_binding: ValidateEffectiveBinding,
	pub store_game_binding: StoreGameBinding,
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
	dependencies
		.check_settings_readiness
		.call((cancellation.clone(),))
		.await
		.context(SetGameDirectoryError)?;

	let stored_binding = dependencies
		.validate_game_directory
		.call((game_directory, cancellation.clone()))
		.await
		.context(SetGameDirectoryError)?;

	let prospective = dependencies
		.preview_game_binding
		.call((stored_binding.clone(), cancellation.clone()))
		.await
		.context(SetGameDirectoryError)?;

	let (effective_binding, warnings) = if prospective.shadowed {
		match dependencies
			.validate_effective_binding
			.call((prospective.effective.clone(), cancellation.clone()))
			.await
		{
			Ok(_) => (EffectiveBinding::Valid, Vec::new()),
			Err(report) => {
				let marker = report.current_context();
				let warnings = match marker.code() {
					ErrorCode::GameBuildMismatch => {
						let Some((expected_build_id, actual_build_id)) = marker.build_ids()
						else {
							return Err(report.context(SetGameDirectoryError));
						};
						vec![SetGameDirectoryWarning::EffectiveGameBindingInvalid {
							variable: "MODS_GAME_DIR",
							expected_build_id,
							actual_build_id,
						}]
					}
					ErrorCode::GameInstallNotFound | ErrorCode::GameInstallInvalid => Vec::new(),
					_ => return Err(report.context(SetGameDirectoryError)),
				};
				(EffectiveBinding::Invalid, warnings)
			}
		}
	} else {
		(EffectiveBinding::Valid, Vec::new())
	};

	let outcome = dependencies
		.store_game_binding
		.call((stored_binding, cancellation))
		.await
		.context(SetGameDirectoryError)?;

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
	use super::SetGameDirectoryDependencies;
	use super::set_game_directory;
	use crate::ErrorCode;
	use crate::errors::ErrorMarker;
	use crate::ports::PortFuture;
	use crate::ports::StoredAndEffectiveBinding;
	use crate::settings::EffectiveBinding;
	use crate::settings::SetGameDirectoryWarning;
	use crate::settings::SettingSource;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use rootcause::report;
	use std::env::temp_dir;
	use std::error::Error;
	use std::result::Result as StdResult;
	use std::sync::Arc;
	use std::sync::atomic::AtomicUsize;
	use std::sync::atomic::Ordering;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn shadowed_invalid_effective_binding_is_validated_before_store_and_returns_warning()
	-> StdResult<(), Box<dyn Error>> {
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
		let store_observed_order = Arc::new(AtomicUsize::new(0));
		let dependencies = SetGameDirectoryDependencies {
			check_settings_readiness: Arc::new(|_| Box::pin(async { Ok(()) }) as PortFuture<_>),
			validate_game_directory: Arc::new({
				let stored = stored.clone();
				move |_, _| {
					let value = stored.clone();
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
			preview_game_binding: Arc::new({
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
				move |_, _| {
					validation_observed_order.store(order.load(Ordering::SeqCst), Ordering::SeqCst);
					order.store(2, Ordering::SeqCst);
					Box::pin(async { Err(report!(ErrorMarker::game_build_mismatch(2, 1))) })
						as PortFuture<_>
				}
			}),
			store_game_binding: Arc::new({
				let stored = stored.clone();
				let effective = effective.clone();
				let order = order.clone();
				let store_observed_order = store_observed_order.clone();
				move |_, _| {
					store_observed_order.store(order.load(Ordering::SeqCst), Ordering::SeqCst);
					order.store(3, Ordering::SeqCst);
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
		};
		let output =
			set_game_directory(dependencies, stored.game_directory().clone(), CancellationToken::new())
				.await
				.map_err(|_| "set failed")?;
		assert_eq!(output.effective_binding, EffectiveBinding::Invalid);
		assert_eq!(output.warnings.len(), 1);
		assert_eq!(validation_observed_order.load(Ordering::SeqCst), 1);
		assert_eq!(store_observed_order.load(Ordering::SeqCst), 2);
		assert_eq!(order.load(Ordering::SeqCst), 3);
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
	async fn shadowed_missing_effective_binding_is_stored_without_warning() -> StdResult<(), Box<dyn Error>> {
		let store_calls = Arc::new(AtomicUsize::new(0));
		let (dependencies, stored) =
			shadowed_dependencies(ErrorMarker::game_install_not_found(), store_calls.clone())?;

		let output =
			set_game_directory(dependencies, stored.game_directory().clone(), CancellationToken::new())
				.await
				.map_err(|_| "set failed")?;

		assert_eq!(store_calls.load(Ordering::SeqCst), 1);
		assert_eq!(output.stored_value, stored.game_directory().clone());
		assert!(output.shadowed);
		assert_eq!(output.effective_binding, EffectiveBinding::Invalid);
		assert!(output.warnings.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn shadowed_invalid_effective_binding_is_stored_without_warning() -> StdResult<(), Box<dyn Error>> {
		let store_calls = Arc::new(AtomicUsize::new(0));
		let (dependencies, stored) =
			shadowed_dependencies(ErrorMarker::game_install_invalid(), store_calls.clone())?;

		let output =
			set_game_directory(dependencies, stored.game_directory().clone(), CancellationToken::new())
				.await
				.map_err(|_| "set failed")?;

		assert_eq!(store_calls.load(Ordering::SeqCst), 1);
		assert!(output.shadowed);
		assert_eq!(output.effective_binding, EffectiveBinding::Invalid);
		assert!(output.warnings.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn shadowed_effective_binding_cancellation_stops_before_store() -> StdResult<(), Box<dyn Error>> {
		let store_calls = Arc::new(AtomicUsize::new(0));
		let (dependencies, stored) =
			shadowed_dependencies(ErrorMarker::operation_cancelled(), store_calls.clone())?;

		let result =
			set_game_directory(dependencies, stored.game_directory().clone(), CancellationToken::new())
				.await;

		assert!(matches!(result, Err(ref report) if report.iter_reports().any(|report| {
			report
				.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::OperationCancelled)
		})));
		assert_eq!(store_calls.load(Ordering::SeqCst), 0);
		Ok(())
	}

	fn shadowed_dependencies(
		validation_error: ErrorMarker,
		store_calls: Arc<AtomicUsize>,
	) -> StdResult<(SetGameDirectoryDependencies, GameBinding), Box<dyn Error>> {
		let stored = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("stored-shadowed-binding"))
				.map_err(|_| "invalid test game path")?,
			SteamBuildId::new(2).map_err(|_| "invalid test build ID")?,
		);
		let effective = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("effective-shadowed-binding"))
				.map_err(|_| "invalid test game path")?,
			SteamBuildId::new(2).map_err(|_| "invalid test build ID")?,
		);
		let dependencies = SetGameDirectoryDependencies {
			check_settings_readiness: Arc::new(|_| Box::pin(async { Ok(()) }) as PortFuture<_>),
			validate_game_directory: Arc::new({
				let stored = stored.clone();
				move |_, _| {
					let value = stored.clone();
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
			preview_game_binding: Arc::new({
				let effective = effective.clone();
				move |stored, _| {
					let value = StoredAndEffectiveBinding {
						stored,
						effective: effective.clone(),
						source: SettingSource::Environment {
							variable: "MODS_GAME_DIR",
						},
						shadowed: true,
					};
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
			validate_effective_binding: Arc::new(move |_, _| {
				let error = validation_error.clone();
				Box::pin(async move { Err(report!(error)) }) as PortFuture<_>
			}),
			store_game_binding: Arc::new({
				let effective = effective.clone();
				move |stored, _| {
					store_calls.fetch_add(1, Ordering::SeqCst);
					let value = StoredAndEffectiveBinding {
						stored,
						effective: effective.clone(),
						source: SettingSource::Environment {
							variable: "MODS_GAME_DIR",
						},
						shadowed: true,
					};
					Box::pin(async move { Ok(value) }) as PortFuture<_>
				}
			}),
		};
		Ok((dependencies, stored))
	}

	#[tokio::test]
	async fn unfinished_settings_work_stops_before_external_validation_and_publication()
	-> StdResult<(), Box<dyn Error>> {
		let game_directory = GameInstallationPath::new(temp_dir().join("missing-game"))
			.map_err(|_| "invalid test game path")?;
		let validation_calls = Arc::new(AtomicUsize::new(0));
		let store_calls = Arc::new(AtomicUsize::new(0));
		let dependencies = SetGameDirectoryDependencies {
			check_settings_readiness: Arc::new(|_| {
				Box::pin(async { Err(report!(ErrorMarker::manual_cleanup_required())) })
					as PortFuture<_>
			}),
			validate_game_directory: Arc::new({
				let validation_calls = validation_calls.clone();
				move |_, _| {
					validation_calls.fetch_add(1, Ordering::SeqCst);
					Box::pin(async { Err(report!(ErrorMarker::game_install_not_found())) })
						as PortFuture<_>
				}
			}),
			preview_game_binding: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::environment_invalid(None))) })
					as PortFuture<_>
			}),
			validate_effective_binding: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::game_install_invalid())) }) as PortFuture<_>
			}),
			store_game_binding: Arc::new({
				let store_calls = store_calls.clone();
				move |_, _| {
					store_calls.fetch_add(1, Ordering::SeqCst);
					Box::pin(async { Err(report!(ErrorMarker::environment_invalid(None))) })
						as PortFuture<_>
				}
			}),
		};

		let result = set_game_directory(dependencies, game_directory, CancellationToken::new()).await;

		assert!(matches!(result, Err(ref report) if report.iter_reports().any(|report| {
			report
				.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::ManualCleanupRequired)
		})));
		assert_eq!(validation_calls.load(Ordering::SeqCst), 0);
		assert_eq!(store_calls.load(Ordering::SeqCst), 0);
		Ok(())
	}
}
