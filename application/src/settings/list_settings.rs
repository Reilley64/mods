use crate::ports::LoadSettings;
use crate::settings::types::SettingRecord;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::fmt;

#[derive(Clone)]
pub struct ListSettingsDependencies {
	pub load_settings: LoadSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListSettingsOutput {
	pub settings: Vec<SettingRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListSettingsError;

impl fmt::Display for ListSettingsError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to list settings")
	}
}

#[tracing::instrument(skip_all)]
pub async fn list_settings(dependencies: ListSettingsDependencies) -> Result<ListSettingsOutput, ListSettingsError> {
	let resolved = dependencies.load_settings.call(()).await.context(ListSettingsError)?;

	Ok(ListSettingsOutput {
		settings: resolved.settings,
	})
}

#[cfg(test)]
mod tests {
	use super::ListSettingsDependencies;
	use super::ListSettingsError;
	use super::list_settings;
	use crate::ErrorCode;
	use crate::errors::ErrorMarker;
	use crate::ports::PortFuture;
	use crate::settings::ResolvedSettings;
	use crate::settings::SettingKey;
	use crate::settings::SettingRecord;
	use crate::settings::SettingSource;
	use crate::settings::SettingValue;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use rootcause::report;
	use std::env::temp_dir;
	use std::error::Error;
	use std::io::Error as IoError;
	use std::result::Result as StdResult;
	use std::sync::Arc;

	#[tokio::test]
	async fn list_returns_port_records_unchanged() -> StdResult<(), Box<dyn Error>> {
		let binding = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("game")).map_err(|_| "invalid test game path")?,
			SteamBuildId::new(1).map_err(|_| "invalid test build ID")?,
		);
		let record = SettingRecord {
			key: SettingKey::SchemaVersion,
			value: SettingValue::UnsignedInteger(1),
			source: SettingSource::Manifest,
			manifest_value: SettingValue::UnsignedInteger(1),
			manifest_path: "schema_version",
			shadowed: false,
			writable: false,
		};
		let resolved = ResolvedSettings {
			settings: vec![record.clone()],
			effective_binding: binding.clone(),
			manifest_binding: binding,
		};
		let dependencies = ListSettingsDependencies {
			load_settings: Arc::new(move || {
				let value = resolved.clone();
				Box::pin(async move { Ok(value) }) as PortFuture<_>
			}),
		};
		assert_eq!(
			list_settings(dependencies).await.map_err(|_| "list failed")?.settings,
			vec![record]
		);
		Ok(())
	}

	#[tokio::test]
	async fn use_case_context_preserves_the_port_report_tree() {
		let dependencies = ListSettingsDependencies {
			load_settings: Arc::new(|| {
				Box::pin(async {
					Err(report!(IoError::other("manifest read failed"))
						.context(ErrorMarker::environment_invalid(None)))
				}) as PortFuture<_>
			}),
		};

		let report = list_settings(dependencies).await.err();
		assert!(report.is_some(), "the port failure must reach the use case");
		let Some(report) = report else {
			return;
		};

		assert_eq!(report.current_context(), &ListSettingsError);
		assert!(report.iter_reports().any(|report| {
			report.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::EnvironmentInvalid)
		}));
		assert!(report
			.iter_reports()
			.any(|report| { report.downcast_current_context::<IoError>().is_some() }));
		assert!(report
			.iter_reports()
			.any(|report| report.downcast_current_context::<ErrorMarker>().is_some()));
	}
}
