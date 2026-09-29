use crate::LayoutLocation;
use crate::validate_stage;
use application::ErrorMarker;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::Path;
use tokio::fs::metadata;
use tokio::fs::remove_dir_all;
use tokio::fs::rename;
use tokio::fs::try_exists;
use tokio_util::sync::CancellationToken;

pub(crate) const OPERATION_DIRECTORY: &str = "operation";

const INITIALIZATION_ARTIFACTS: [(&str, bool); 5] = [
	("mods", true),
	("profile", true),
	("overwrite", true),
	("cache", true),
	("mods.toml", false),
];

pub(crate) async fn publish_initialization(
	root: &Path,
	temp: &Path,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let stage = temp.join(OPERATION_DIRECTORY).join("stage");
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let validation = validate_stage(&stage, LayoutLocation::Stage, cancellation).await;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	validation.context(publication_failed())?;

	for (name, directory) in INITIALIZATION_ARTIFACTS {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		validate_entry(&stage.join(name), directory).await?;
		if try_exists(root.join(name)).await.context(publication_failed())? {
			return Err(report!(publication_failed()));
		}
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		rename(stage.join(name), root.join(name))
			.await
			.context(publication_failed())?;
	}

	// mods.toml is the last required canonical mutation. Caller cancellation is no longer
	// observable once that rename succeeds.
	validate_stage(root, LayoutLocation::Root, &CancellationToken::new())
		.await
		.context(publication_failed())?;

	cleanup_best_effort(temp, OPERATION_DIRECTORY).await;
	Ok(())
}

pub(crate) async fn cleanup_best_effort(temp: &Path, operation_name: &str) {
	let _ = remove_dir_all(temp.join(operation_name)).await;
}

async fn validate_entry(path: &Path, expected_directory: bool) -> Result<(), ErrorMarker> {
	let metadata = metadata(path).await.context(publication_failed())?;
	if metadata.is_dir() != expected_directory || metadata.is_file() == expected_directory {
		return Err(report!(publication_failed()));
	}
	Ok(())
}

fn publication_failed() -> ErrorMarker {
	ErrorMarker::environment_publication_failed(Some("publication"))
}
