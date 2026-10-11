use application::ErrorMarker;
use application::ports::InitializationPlan;
use domain::EnvironmentName;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::str::from_utf8;
use tokio::fs::read;
use tokio::fs::write;
use tokio_util::sync::CancellationToken;
use toml::from_str;
use toml::to_string_pretty;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
	#[serde(skip_serializing_if = "Option::is_none")]
	nexus_api_key: Option<String>,
	schema_version: u32,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	pub(crate) game_dir: String,
}

pub(crate) async fn write_manifest(stage: &Path, plan: &InitializationPlan) -> Result<(), ErrorMarker> {
	let game_dir = plan
		.game_binding
		.game_directory()
		.as_path()
		.to_str()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;

	let contents = to_string_pretty(&Manifest {
		nexus_api_key: None,
		schema_version: 1,
		name: None,
		game_dir: game_dir.to_owned(),
	})
	.context(ErrorMarker::environment_invalid(None))?;

	write(stage.join("mods.toml"), contents)
		.await
		.context(ErrorMarker::environment_root_unsafe())
}

pub(crate) async fn validate_manifest(directory: &Path, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	validate_manifest_file(directory, cancellation).await.map(drop)
}

pub(crate) async fn validate_manifest_file(
	directory: &Path,
	cancellation: &CancellationToken,
) -> Result<Manifest, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let contents = read(directory.join("mods.toml"))
		.await
		.context(ErrorMarker::environment_invalid(None))?;

	parse_manifest(&contents)
}

pub(crate) fn parse_manifest(contents: &[u8]) -> Result<Manifest, ErrorMarker> {
	let text = from_utf8(contents).context(ErrorMarker::environment_invalid(None))?;
	// TOML parse errors embed source lines, which can contain nexus_api_key.
	let manifest: Manifest = from_str(text).map_err(|_| report!(ErrorMarker::environment_invalid(None)))?;
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
	use tempfile::TempDir;

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
}
