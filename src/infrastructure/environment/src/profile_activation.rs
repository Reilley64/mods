use crate::active_code_page::decode as decode_active_code_page;
use crate::files::read_optional;
use crate::profile::PROFILE_FILES;
use crate::profile::is_activatable_plugin_name;
use application::ErrorMarker;
use domain::DataRelativePath;
use domain::profile_test_file_slots;
use encoding_rs::WINDOWS_1252;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashSet;
use std::path::Path;
use tokio::fs::read;
use tokio_util::sync::CancellationToken;

pub(crate) struct ProfileActivation {
	active_plugins: HashSet<String>,
}

impl ProfileActivation {
	pub(crate) async fn load(profile: &Path, cancellation: &CancellationToken) -> Result<Self, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let plugin_bytes = read(profile.join("plugins.txt"))
			.await
			.context(ErrorMarker::environment_invalid(None))?;

		let plugin_text = decode_active_code_page(&plugin_bytes)?;
		let mut active_plugins = plugin_text
			.split_terminator("\r\n")
			.filter(|line| !line.is_empty() && !line.starts_with('#'))
			.map(|name| {
				activation_plugin_key(name)
					.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))
			})
			.collect::<Result<HashSet<_>, _>>()?;

		for name in PROFILE_FILES.into_iter().take_while(|name| name.ends_with(".ini")) {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

			let Some(bytes) = read_optional(&profile.join(name))
				.await
				.context(ErrorMarker::environment_invalid(None))?
			else {
				continue;
			};
			let text = decode_ini(&bytes)?;

			active_plugins.extend(profile_test_file_slots(&text)
				.into_iter()
				.flatten()
				.filter_map(activation_plugin_key));
		}

		Ok(Self { active_plugins })
	}

	pub(crate) fn is_active(&self, path: &DataRelativePath) -> bool {
		path.comparison_key() == "falloutnv.esm" || self.active_plugins.contains(path.comparison_key())
	}
}

fn activation_plugin_key(name: &str) -> Option<String> {
	let path = DataRelativePath::new(name.to_owned()).ok()?;
	if path.components().count() != 1 || !is_activatable_plugin_name(path.as_str()) {
		return None;
	}
	Some(path.comparison_key().to_owned())
}

fn decode_ini(bytes: &[u8]) -> Result<String, ErrorMarker> {
	if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
		return String::from_utf8(bytes.to_vec()).context(ErrorMarker::environment_invalid(None));
	}
	if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
		if bytes.len() % 2 != 0 {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let mut values = Vec::with_capacity(bytes.len() / 2);
		let mut index = 0;
		while index < bytes.len() {
			values.push(u16::from_le_bytes([bytes[index], bytes[index + 1]]));
			index += 2;
		}
		return String::from_utf16(&values).context(ErrorMarker::environment_invalid(None));
	}
	match String::from_utf8(bytes.to_vec()) {
		Ok(text) => Ok(text),
		Err(_) => {
			let (text, _, _) = WINDOWS_1252.decode(bytes);
			Ok(text.into_owned())
		}
	}
}

#[cfg(test)]
mod tests {
	use super::ProfileActivation;
	use domain::DataRelativePath;
	use rootcause::Result as RootResult;
	use std::error::Error;
	use std::fs;
	use std::result::Result as StdResult;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn reads_numbered_slots_from_utf16_profile_ini() -> RootResult<()> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("plugins.txt"), b"")?;
		let mut bytes = vec![0xff, 0xfe];
		bytes.extend("[General]\nsTestFile1=Café.esp"
			.encode_utf16()
			.flat_map(u16::to_le_bytes));
		fs::write(temp.path().join("Fallout.ini"), bytes)?;
		let profile = temp.path().canonicalize()?;

		let activation = ProfileActivation::load(&profile, &CancellationToken::new()).await?;
		let path = DataRelativePath::new("café.esp".to_owned())?;
		assert!(activation.is_active(&path));
		Ok(())
	}

	#[tokio::test]
	async fn loads_additive_effective_test_file_slots_from_every_profile_ini() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("plugins.txt"), b"Listed.ESP\r\n")?;
		fs::write(
			temp.path().join("Fallout.ini"),
			concat!(
				"[General]\r\n",
				"sTestFile1=Superseded.esp\r\n",
				"sTestFile1=Fallout.esm\r\n",
				"sTestFile2=ActiveEsl.esl\r\n",
				"sTestFile10=Ten.esp\r\n",
				"sTestFile01=Leading.esp\r\n",
			),
		)?;
		fs::write(
			temp.path().join("FalloutPrefs.ini"),
			b"[general]\r\nsTESTfile1=Prefs.esp\r\n",
		)?;
		fs::write(
			temp.path().join("FalloutCustom.ini"),
			b"[General]\r\nsTestFile1=Custom.esm\r\n",
		)?;
		fs::write(
			temp.path().join("GECKCustom.ini"),
			b"[General]\r\nsTestFile1=GeckCustom.esp\r\n",
		)?;
		fs::write(
			temp.path().join("GECKPrefs.ini"),
			b"[General]\r\nsTestFile1=GeckPrefs.esm\r\n",
		)?;
		let profile = temp.path().canonicalize()?;

		let activation = ProfileActivation::load(&profile, &CancellationToken::new())
			.await
			.map_err(|_| "activation must load")?;
		for name in [
			"listed.esp",
			"fallout.esm",
			"ten.esp",
			"prefs.esp",
			"custom.esm",
			"geckcustom.esp",
			"geckprefs.esm",
			"activeesl.esl",
		] {
			let path = DataRelativePath::new(name.to_owned()).map_err(|_| "plugin path must be valid")?;
			assert!(activation.is_active(&path), "{name} must be active");
		}
		for name in ["superseded.esp", "leading.esp"] {
			let path = DataRelativePath::new(name.to_owned()).map_err(|_| "plugin path must be valid")?;
			assert!(!activation.is_active(&path), "{name} must not be active");
		}
		Ok(())
	}
}
