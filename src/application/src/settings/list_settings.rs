use crate::ports::ProgressEvent;
use crate::ports::ReportProgress;
use crate::settings::types::SettingRecord;
use rootcause::Result;
use std::fmt;

#[derive(Clone)]
pub struct ListSettingsDependencies {
	pub report_progress: Option<ReportProgress>,
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
pub async fn list_settings(
	dependencies: ListSettingsDependencies,
	settings: Vec<SettingRecord>,
) -> Result<ListSettingsOutput, ListSettingsError> {
	if let Some(progress) = &dependencies.report_progress {
		progress.call((ProgressEvent::SettingsLoaded,)).await;
	}

	Ok(ListSettingsOutput { settings })
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
		let output =
			list_settings(ListSettingsDependencies { report_progress: None }, vec![record.clone()]).await?;
		assert_eq!(output.settings, vec![record]);
		Ok(())
	}
}
