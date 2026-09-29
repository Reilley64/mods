use crate::safe_fs::SafeDir;
use crate::safe_fs::read_bounded;
use application::ErrorMarker;
use application::ports::InitializationPlan;
use domain::EnvironmentName;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;
use std::str::from_utf8;
use tokio_util::sync::CancellationToken;
use toml::from_str;
use toml::to_string_pretty;

pub(crate) const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
	schema_version: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	pub(crate) game_dir: String,
}

pub(crate) fn write_manifest(stage: &SafeDir, plan: &InitializationPlan) -> Result<(), ErrorMarker> {
	let game_dir = plan
		.game_binding
		.game_directory()
		.as_path()
		.to_str()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	let contents = to_string_pretty(&Manifest {
		schema_version: 1,
		name: None,
		game_dir: game_dir.to_owned(),
	})
	.context(ErrorMarker::environment_invalid(None))?;
	stage.write_new("mods.toml", contents.as_bytes())
		.context(ErrorMarker::environment_root_unsafe())
}

pub(crate) fn validate_manifest(directory: &SafeDir, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	validate_manifest_file(directory, cancellation).map(drop)
}

pub(crate) fn validate_manifest_file(
	directory: &SafeDir,
	cancellation: &CancellationToken,
) -> Result<Manifest, ErrorMarker> {
	let contents = read_bounded(
		directory,
		"mods.toml",
		MAX_MANIFEST_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)?;
	parse_manifest(&contents)
}

pub(crate) fn parse_manifest(contents: &[u8]) -> Result<Manifest, ErrorMarker> {
	let text = from_utf8(contents).context(ErrorMarker::environment_invalid(None))?;
	let manifest: Manifest = from_str(text).context(ErrorMarker::environment_invalid(None))?;
	if manifest.schema_version != 1
		|| GameInstallationPath::new(manifest.game_dir.clone().into()).is_err()
		|| manifest
			.name
			.as_deref()
			.is_some_and(|name| EnvironmentName::new(name.to_owned()).is_err())
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(manifest)
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
mod tests {
	use super::MAX_MANIFEST_BYTES;
	use super::validate_manifest_file;
	use crate::safe_fs::SafeDir;
	use application::ErrorCode;
	use std::fs;
	use std::io;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn manifest_contains_only_current_binding_fields_and_rejects_legacy_ids() {
		let temp = TempDir::new().expect("temporary directory must be created");
		let path = temp.path().canonicalize().expect("temporary directory must resolve");
		let text = format!(
			"schema_version = 1\ngame_dir = {}\n",
			toml::Value::String(path.to_string_lossy().into_owned())
		);
		assert!(super::parse_manifest(text.as_bytes()).is_ok());
		for field in ["steam_app_id = 22380\n", "observed_build_id = 42\n"] {
			assert!(super::parse_manifest(format!("{text}{field}").as_bytes()).is_err());
		}
	}

	#[test]
	fn manifest_cap_is_deliberate_and_preserves_limit_cause() {
		assert_eq!(MAX_MANIFEST_BYTES, 64 * 1024);
		let temp = TempDir::new().expect("temporary directory must be created");
		fs::write(temp.path().join("mods.toml"), vec![b'x'; MAX_MANIFEST_BYTES + 1])
			.expect("oversized manifest must be written");
		let directory =
			SafeDir::open_absolute(&temp.path().canonicalize().expect("temporary directory must resolve"))
				.expect("safe directory must open");

		let error = validate_manifest_file(&directory, &CancellationToken::new())
			.expect_err("oversized manifest must be rejected");
		assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
		assert!(error.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
		}));
	}
}
