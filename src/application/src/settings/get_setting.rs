use crate::errors::ErrorMarker;
use crate::ports::LoadSettings;
use crate::settings::types::SettingKey;
use crate::settings::types::SettingRecord;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt;

#[derive(Clone)]
pub struct GetSettingDependencies {
	pub load_settings: LoadSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetSettingOutput {
	pub setting: SettingRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetSettingError;

impl fmt::Display for GetSettingError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to get setting")
	}
}

#[tracing::instrument(skip_all)]
pub async fn get_setting(
	dependencies: GetSettingDependencies,
	key: SettingKey,
) -> Result<GetSettingOutput, GetSettingError> {
	let resolved = dependencies.load_settings.call(()).await.context(GetSettingError)?;

	let setting = resolved
		.settings
		.into_iter()
		.find(|record| record.key == key)
		.ok_or_else(|| report!(ErrorMarker::setting_unknown()))
		.context(GetSettingError)?;

	Ok(GetSettingOutput { setting })
}

#[cfg(test)]
mod tests {
	use super::GetSettingDependencies;
	use super::get_setting;
	use crate::ports::PortFuture;
	use crate::settings::ResolvedSettings;
	use crate::settings::SettingKey;
	use crate::settings::SettingRecord;
	use crate::settings::SettingSource;
	use crate::settings::SettingValue;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use std::env::temp_dir;
	use std::error::Error;
	use std::result::Result as StdResult;
	use std::sync::Arc;

	#[tokio::test]
	async fn get_selects_the_requested_typed_key() -> StdResult<(), Box<dyn Error>> {
		let binding = GameBinding::new(
			GameInstallationPath::new(temp_dir().join("game")).map_err(|_| "invalid test game path")?,
			SteamBuildId::new(1).map_err(|_| "invalid test build ID")?,
		);
		let records = SettingKey::ALL
			.into_iter()
			.map(|key| SettingRecord {
				key,
				value: SettingValue::Unset,
				source: SettingSource::Manifest,
				manifest_value: SettingValue::Unset,
				manifest_path: key.manifest_path(),
				shadowed: false,
				writable: key == SettingKey::GameDir,
			})
			.collect();
		let resolved = ResolvedSettings {
			settings: records,
			effective_binding: binding.clone(),
			manifest_binding: binding,
		};
		let dependencies = GetSettingDependencies {
			load_settings: Arc::new(move || {
				let value = resolved.clone();
				Box::pin(async move { Ok(value) }) as PortFuture<_>
			}),
		};
		assert_eq!(
			get_setting(dependencies, SettingKey::GameDir)
				.await
				.map_err(|_| "get failed")?
				.setting
				.key,
			SettingKey::GameDir
		);
		Ok(())
	}
}
