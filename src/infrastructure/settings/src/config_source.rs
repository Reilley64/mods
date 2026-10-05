use application::ErrorMarker;
use config::Config;
use config::Environment;
use config::File;
use config::FileFormat;
use rootcause::Result;
use rootcause::report;
use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::OsString;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawManifest {
	pub nexus_api_key: Option<String>,
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
	let shadowed = filtered.contains_key("MODS_GAME_DIR");
	// Config parser errors can include the complete secret-bearing source document.
	// The public marker intentionally replaces that unsafe external cause.
	let manifest_only: RawManifest = Config::builder()
		.add_source(File::from_str(manifest, FileFormat::Toml))
		.build()
		.and_then(Config::try_deserialize)
		.map_err(|_| report!(ErrorMarker::environment_invalid(None)))?;
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
		.map_err(|_| report!(ErrorMarker::environment_invalid(None)))?;
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
	let mut result = HashMap::new();
	for (key, value) in environment {
		let Some(key) = key.to_str() else { continue };
		if !key.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("MODS_")) {
			continue;
		}
		let canonical = if key.eq_ignore_ascii_case("MODS_GAME_DIR") {
			"MODS_GAME_DIR"
		} else if key.eq_ignore_ascii_case("MODS_NEXUS_API_KEY") {
			"MODS_NEXUS_API_KEY"
		} else {
			continue;
		};
		if value.is_empty() {
			continue;
		}
		if result.contains_key(canonical) {
			return Err(report!(invalid.clone()));
		}
		let value = value.to_str().ok_or_else(|| report!(invalid.clone()))?;
		result.insert(canonical.to_owned(), value.to_owned());
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
	fn nexus_override_is_secret_and_does_not_shadow_game_directory() {
		let manifest = "schema_version=1\nsteam_app_id=22380\ngame_dir='game'\nobserved_build_id=1\nnexus_api_key='synthetic-stored-key'";
		let environment = vec![(
			OsString::from("MODS_NEXUS_API_KEY"),
			OsString::from("synthetic-override-key"),
		)];
		let result = super::read_sources(
			manifest,
			&environment,
			application::ErrorMarker::settings_environment_invalid(),
		);
		assert!(result.is_ok());
		if let Ok((stored, effective, shadowed)) = result {
			assert_eq!(stored.nexus_api_key.as_deref(), Some("synthetic-stored-key"));
			assert_eq!(effective.nexus_api_key.as_deref(), Some("synthetic-override-key"));
			assert!(!shadowed);
		}
		for malformed in [
			format!("{manifest}\nbroken = '"),
			manifest.replace(
				"nexus_api_key='synthetic-stored-key'",
				"nexus_api_key=['synthetic-stored-key']",
			),
		] {
			let error = super::read_sources(
				&malformed,
				&[],
				application::ErrorMarker::settings_environment_invalid(),
			)
			.err();
			assert!(error.is_some());
			assert!(!format!("{error:?}").contains("synthetic-stored-key"));
		}
	}

	#[test]
	fn case_insensitive_environment_name_is_accepted_as_a_string() {
		let environment = vec![(OsString::from("mods_game_dir"), OsString::from("true"))];
		assert_eq!(
			initialization_override(&environment).ok(),
			Some(Some("true".to_owned()))
		);
	}

	#[test]
	fn unknown_mods_variables_are_ignored() {
		let environment = vec![
			(OsString::from("MODS_GAMEDIR"), OsString::from("x")),
			(OsString::from("MODS_USVFS_ARTIFACTS"), OsString::from("artifacts")),
			(OsString::from("MODS_GAME_DIR"), OsString::from("game")),
		];
		assert_eq!(
			initialization_override(&environment).ok(),
			Some(Some("game".to_owned()))
		);
	}

	#[test]
	fn empty_mods_values_are_unset() {
		let empty = vec![(OsString::from("MODS_GAME_DIR"), OsString::from(""))];
		assert_eq!(initialization_override(&empty).ok(), Some(None));
		let empty_and_set = vec![
			(OsString::from("mods_game_dir"), OsString::from("")),
			(OsString::from("MODS_GAME_DIR"), OsString::from("game")),
		];
		assert_eq!(
			initialization_override(&empty_and_set).ok(),
			Some(Some("game".to_owned()))
		);
		let manifest = "schema_version=1\nsteam_app_id=22380\ngame_dir='game'\nobserved_build_id=1\nnexus_api_key='synthetic-stored-key'";
		let environment = vec![
			(OsString::from("MODS_GAME_DIR"), OsString::from("")),
			(OsString::from("MODS_NEXUS_API_KEY"), OsString::from("")),
		];
		let result = super::read_sources(
			manifest,
			&environment,
			application::ErrorMarker::settings_environment_invalid(),
		);
		assert!(result.is_ok());
		if let Ok((_, effective, shadowed)) = result {
			assert_eq!(effective.game_dir, "game");
			assert_eq!(effective.nexus_api_key.as_deref(), Some("synthetic-stored-key"));
			assert!(!shadowed);
		}
	}

	#[test]
	fn duplicate_mods_values_fail() {
		let duplicate = vec![
			(OsString::from("MODS_GAME_DIR"), OsString::from("a")),
			(OsString::from("mods_game_dir"), OsString::from("b")),
		];
		assert!(initialization_override(&duplicate).is_err());
	}

	#[cfg(unix)]
	#[test]
	fn non_unicode_accepted_value_fails() {
		let environment = vec![(OsString::from("MODS_GAME_DIR"), OsString::from_vec(vec![0xff]))];
		assert!(initialization_override(&environment).is_err());
	}
}
