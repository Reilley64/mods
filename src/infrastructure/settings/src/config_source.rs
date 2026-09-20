use application::ErrorMarker;
use config::Config;
use config::Environment;
use config::File;
use config::FileFormat;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::OsString;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawManifest {
	pub schema_version: u32,
	pub name: Option<String>,
	pub steam_app_id: u32,
	pub game_dir: String,
	pub observed_build_id: u64,
}

pub(crate) fn read_sources(
	manifest: &str,
	environment: &[(OsString, OsString)],
	invalid_environment: ErrorMarker,
) -> Result<(RawManifest, RawManifest, bool), ErrorMarker> {
	let filtered = scan_environment(environment, invalid_environment)?;
	let shadowed = !filtered.is_empty();
	let manifest_only: RawManifest = Config::builder()
		.add_source(File::from_str(manifest, FileFormat::Toml))
		.build()
		.and_then(Config::try_deserialize)
		.context(ErrorMarker::environment_invalid(None))?;
	let effective: RawManifest = Config::builder()
		.add_source(File::from_str(manifest, FileFormat::Toml))
		.add_source(
			Environment::with_prefix("MODS")
				.prefix_separator("_")
				.separator("__")
				.ignore_empty(false)
				.try_parsing(false)
				.source(Some(filtered)),
		)
		.build()
		.and_then(Config::try_deserialize)
		.context(ErrorMarker::environment_invalid(None))?;
	Ok((manifest_only, effective, shadowed))
}

pub(crate) fn initialization_override(environment: &[(OsString, OsString)]) -> Result<Option<String>, ErrorMarker> {
	Ok(
		scan_environment(environment, ErrorMarker::initialization_environment_invalid())?
			.remove("MODS_GAME_DIR"),
	)
}

fn scan_environment(
	environment: &[(OsString, OsString)],
	invalid: ErrorMarker,
) -> Result<HashMap<String, String>, ErrorMarker> {
	let mut accepted = None;
	for (key, value) in environment {
		let Some(key) = key.to_str() else { continue };
		if !key.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("MODS_")) {
			continue;
		}
		if !key.eq_ignore_ascii_case("MODS_GAME_DIR") {
			return Err(report!(invalid.clone()));
		}
		if accepted.is_some() {
			return Err(report!(invalid.clone()));
		}
		let value = value.to_str().ok_or_else(|| report!(invalid.clone()))?;
		if value.is_empty() {
			return Err(report!(invalid.clone()));
		}
		accepted = Some(value.to_owned());
	}
	let mut result = HashMap::new();
	if let Some(value) = accepted {
		result.insert("MODS_GAME_DIR".to_owned(), value);
	}
	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::initialization_override;
	use std::ffi::OsString;
	#[cfg(unix)]
	use std::os::unix::ffi::OsStringExt as _;

	#[test]
	fn case_insensitive_environment_name_is_accepted_as_a_string() {
		let environment = vec![(OsString::from("mods_game_dir"), OsString::from("true"))];
		assert_eq!(
			initialization_override(&environment).ok(),
			Some(Some("true".to_owned()))
		);
	}

	#[test]
	fn unknown_duplicate_and_empty_mods_values_fail() {
		let unknown = vec![(OsString::from("MODS_GAMEDIR"), OsString::from("x"))];
		assert!(initialization_override(&unknown).is_err());
		let duplicate = vec![
			(OsString::from("MODS_GAME_DIR"), OsString::from("a")),
			(OsString::from("mods_game_dir"), OsString::from("b")),
		];
		assert!(initialization_override(&duplicate).is_err());
		let empty = vec![(OsString::from("MODS_GAME_DIR"), OsString::from(""))];
		assert!(initialization_override(&empty).is_err());
	}

	#[cfg(unix)]
	#[test]
	fn non_unicode_accepted_value_fails() {
		let environment = vec![(OsString::from("MODS_GAME_DIR"), OsString::from_vec(vec![0xff]))];
		assert!(initialization_override(&environment).is_err());
	}
}
