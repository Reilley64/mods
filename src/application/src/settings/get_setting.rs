use crate::errors::ErrorMarker;
use crate::ports::ProgressEvent;
use crate::ports::ReportProgress;
use crate::settings::types::SettingKey;
use crate::settings::types::SettingRecord;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fmt;

#[derive(Clone)]
pub struct GetSettingDependencies {
	pub report_progress: Option<ReportProgress>,
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
	settings: Vec<SettingRecord>,
	key: SettingKey,
) -> Result<GetSettingOutput, GetSettingError> {
	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::SettingsLoaded,)).await;
	}

	let setting = settings
		.into_iter()
		.find(|record| record.key == key)
		.ok_or_else(|| report!(ErrorMarker::setting_unknown()))
		.context(GetSettingError)?;

	Ok(GetSettingOutput { setting })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::settings::SettingKey;
	use crate::settings::SettingSource;
	use crate::settings::SettingValue;
	use rootcause::Result;

	#[tokio::test]
	async fn supplied_records_preserve_provenance() -> Result<()> {
		let record = SettingRecord {
			key: SettingKey::GameDir,
			value: SettingValue::Path("effective".into()),
			source: SettingSource::Environment {
				variable: "MODS_GAME_DIR",
			},
			manifest_value: SettingValue::Path("stored".into()),
			manifest_path: "game_dir",
			shadowed: true,
			writable: true,
		};
		let output = get_setting(
			GetSettingDependencies { report_progress: None },
			vec![record.clone()],
			SettingKey::GameDir,
		)
		.await?;
		assert_eq!(output.setting, record);
		assert!(get_setting(
			GetSettingDependencies { report_progress: None },
			Vec::new(),
			SettingKey::Name
		)
		.await
		.is_err());
		Ok(())
	}
}
