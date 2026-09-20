use crate::LayoutLocation;
use crate::safe_fs::SafeDir;
use crate::validate_stage;
use application::ErrorMarker;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use tokio_util::sync::CancellationToken;

pub(crate) const OPERATION_DIRECTORY: &str = "operation";

const INITIALIZATION_ARTIFACTS: [(&str, bool); 5] = [
	("mods", true),
	("profile", true),
	("overwrite", true),
	("cache", true),
	("mods.toml", false),
];

pub(crate) fn publish_initialization(
	root: &SafeDir,
	temp: &SafeDir,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let operation = temp.open_dir(OPERATION_DIRECTORY).context(publication_failed())?;
	let stage = operation.open_dir("stage").context(publication_failed())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let validation = validate_stage(&stage, LayoutLocation::Stage, cancellation);
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	validation.context(publication_failed())?;

	for (name, directory) in INITIALIZATION_ARTIFACTS {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		validate_entry(&stage, name, directory)?;
		if root.exists(name).context(publication_failed())? {
			return Err(report!(publication_failed()));
		}
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		stage.rename_durable_to(name, root, name)
			.context(publication_failed())?;
	}

	// mods.toml is the last required canonical mutation. Caller cancellation is no longer
	// observable once that durable rename succeeds.
	validate_stage(root, LayoutLocation::Root, &CancellationToken::new()).context(publication_failed())?;

	drop(stage);
	drop(operation);
	cleanup_best_effort(root, temp, OPERATION_DIRECTORY);
	Ok(())
}

pub(crate) fn cleanup_best_effort(root: &SafeDir, temp: &SafeDir, operation_name: &str) {
	if temp.remove_dir_all(operation_name).is_ok() {
		let _ = temp.sync();
		let _ = root.sync();
	}
}

fn validate_entry(directory: &SafeDir, name: &str, expected_directory: bool) -> Result<(), ErrorMarker> {
	let metadata = directory.symlink_metadata(name).context(publication_failed())?;
	if metadata.is_dir() != expected_directory || metadata.is_file() == expected_directory {
		return Err(report!(publication_failed()));
	}
	if expected_directory {
		directory.open_dir(name).context(publication_failed())?;
	} else {
		directory.open_regular(name).context(publication_failed())?;
	}
	Ok(())
}

fn publication_failed() -> ErrorMarker {
	ErrorMarker::environment_publication_failed(Some("publication"))
}
