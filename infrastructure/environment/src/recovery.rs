use crate::LayoutLocation;
use crate::NoPublicationFailure;
use crate::PublicationFailure;
use crate::PublicationPoint;
use crate::check_cancelled;
use crate::combine_errors;
use crate::safe_fs::SafeDir;
use crate::safe_fs::is_reparse;
use crate::validate_recovery;
use crate::validate_stage;
use application::ErrorCode;
use application::ErrorMarker;
use application::ports::RecoveryOutcome;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::path::Path;
use std::str::from_utf8;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const OPERATION_FILE: &str = "operation.toml";
const OPERATION_TEMP_FILE: &str = "operation.toml.tmp";
const ARTIFACTS: [(&str, bool, PublicationPoint); 5] = [
	("mods", true, PublicationPoint::ModsPublished),
	("profile", true, PublicationPoint::ProfilePublished),
	("overwrite", true, PublicationPoint::OverwritePublished),
	("cache", true, PublicationPoint::CachePublished),
	("mods.toml", false, PublicationPoint::ManifestPublished),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OperationRecord {
	schema_version: u32,
	operation: OperationKind,
	operation_id: Uuid,
	phase: OperationPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperationKind {
	Initialize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OperationPhase {
	Publishing,
}

pub(crate) fn recover(root_path: &Path, cancellation: &CancellationToken) -> Result<RecoveryOutcome, ErrorMarker> {
	check_cancelled(cancellation)?;
	let root = match SafeDir::open_absolute(root_path) {
		Ok(root) => root,
		Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {
			return Ok(RecoveryOutcome::NothingToRecover);
		}
		Err(error) => {
			return Err(error.context(ErrorMarker::environment_root_unsafe()));
		}
	};
	check_cancelled(cancellation)?;
	let temp = match root.open_dir("temp") {
		Ok(temp) => temp,
		Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {
			return Ok(RecoveryOutcome::NothingToRecover);
		}
		Err(error) => return Err(error.context(recovery_failed())),
	};
	check_cancelled(cancellation)?;

	let mut pending = None;
	let mut orphaned = Vec::new();
	for name in temp.entries().context(recovery_failed())? {
		check_cancelled(cancellation)?;
		let name = name.to_str().ok_or_else(|| report!(recovery_failed()))?;
		let operation_id = match Uuid::parse_str(name) {
			Ok(operation_id) => operation_id,
			Err(error) => {
				return Err(report!(error).context(recovery_failed()));
			}
		};
		let operation_dir = temp.open_dir(name).context(recovery_failed())?;
		if !operation_dir.exists(OPERATION_FILE).context(recovery_failed())? {
			orphaned.push(name.to_owned());
			continue;
		}
		read_record(&operation_dir, operation_id)?;
		if pending.replace(name.to_owned()).is_some() {
			return Err(report!(recovery_failed()));
		}
	}

	for name in orphaned {
		check_cancelled(cancellation)?;
		temp.remove_dir_all(&name).context(recovery_failed())?;
		temp.sync().context(recovery_failed())?;
		check_cancelled(cancellation)?;
	}
	let Some(operation_name) = pending else {
		check_cancelled(cancellation)?;
		remove_empty_temp(&root, temp)?;
		return Ok(RecoveryOutcome::NothingToRecover);
	};

	check_cancelled(cancellation)?;
	let mut no_failure = NoPublicationFailure;
	match complete_initialization(&root, &temp, &operation_name, &mut no_failure, Some(cancellation)) {
		Ok(()) => Ok(RecoveryOutcome::Committed),
		Err(completion_error) if completion_error.current_context().code() == ErrorCode::OperationCancelled => {
			Err(completion_error)
		}
		Err(completion_error) if cancellation.is_cancelled() => Err(combine_errors(
			report!(ErrorMarker::operation_cancelled()),
			completion_error,
		)),
		Err(completion_error) => {
			match rollback_initialization(&root, &temp, &operation_name, Some(cancellation)) {
				Ok(()) => {
					check_cancelled(cancellation)?;
					remove_empty_temp(&root, temp)?;
					Ok(RecoveryOutcome::RolledBack)
				}
				Err(rollback_error) => Err(combine_errors(completion_error, rollback_error)),
			}
		}
	}
}

pub(crate) fn write_record(
	operation_dir: &SafeDir,
	operation_id: Uuid,
	failure: &mut dyn PublicationFailure,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let record = OperationRecord {
		schema_version: 1,
		operation: OperationKind::Initialize,
		operation_id,
		phase: OperationPhase::Publishing,
	};
	let contents = toml::to_string(&record).context(recovery_failed())?;

	operation_dir
		.write_new(OPERATION_TEMP_FILE, contents.as_bytes())
		.context(recovery_failed())?;
	let validated = read_record_file(operation_dir, OPERATION_TEMP_FILE)?;
	if validated != record {
		return Err(report!(recovery_failed()));
	}
	failure.check(PublicationPoint::MarkerTempDurable)
		.context(recovery_failed())?;
	check_cancelled(cancellation)?;

	if operation_dir.exists(OPERATION_FILE).context(recovery_failed())? {
		return Err(report!(recovery_failed()));
	}
	operation_dir
		.rename_to(OPERATION_TEMP_FILE, operation_dir, OPERATION_FILE)
		.context(recovery_failed())?;
	operation_dir.sync().context(recovery_failed())?;
	if read_record_file(operation_dir, OPERATION_FILE)? != record {
		return Err(report!(recovery_failed()));
	}
	check_cancelled(cancellation)
}

pub(crate) fn complete_initialization(
	root: &SafeDir,
	temp: &SafeDir,
	operation_name: &str,
	failure: &mut dyn PublicationFailure,
	cancellation: Option<&CancellationToken>,
) -> Result<(), ErrorMarker> {
	let operation_id = Uuid::parse_str(operation_name).context(recovery_failed())?;
	let operation_dir = temp.open_dir(operation_name).context(recovery_failed())?;
	read_record(&operation_dir, operation_id)?;
	let stage = operation_dir.open_dir("stage").context(recovery_failed())?;
	validate_recovery(root, &stage).context(recovery_failed())?;
	if let Some(cancellation) = cancellation {
		check_cancelled(cancellation)?;
	}

	for (name, directory, point) in ARTIFACTS {
		if let Some(cancellation) = cancellation {
			check_cancelled(cancellation)?;
		}
		move_once(&stage, root, name, directory)?;
		root.sync().context(recovery_failed())?;
		failure.check(point).context(recovery_failed())?;
		if point != PublicationPoint::ManifestPublished
			&& let Some(cancellation) = cancellation
		{
			check_cancelled(cancellation)?;
		}
	}

	failure.check(PublicationPoint::BeforeFinalValidation)
		.context(recovery_failed())?;
	validate_stage(root, LayoutLocation::Root).context(recovery_failed())?;

	drop(stage);
	drop(operation_dir);
	temp.remove_dir_all(operation_name).context(recovery_failed())?;
	temp.sync().context(recovery_failed())?;
	root.sync().context(recovery_failed())?;
	Ok(())
}

pub(crate) fn rollback_initialization(
	root: &SafeDir,
	temp: &SafeDir,
	operation_name: &str,
	cancellation: Option<&CancellationToken>,
) -> Result<(), ErrorMarker> {
	if let Some(cancellation) = cancellation {
		check_cancelled(cancellation)?;
	}
	let operation_id = Uuid::parse_str(operation_name).context(recovery_failed())?;
	let operation_dir = temp.open_dir(operation_name).context(recovery_failed())?;
	read_record(&operation_dir, operation_id)?;
	drop(operation_dir);

	for (name, directory, _) in ARTIFACTS.into_iter().rev() {
		if let Some(cancellation) = cancellation {
			check_cancelled(cancellation)?;
		}
		let metadata = match root.symlink_metadata(name) {
			Ok(metadata) => metadata,
			Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => {
				continue;
			}
			Err(error) => {
				return Err(error.context(recovery_failed()));
			}
		};
		if is_reparse(&metadata) || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
			return Err(report!(recovery_failed()));
		}
		if directory {
			root.remove_dir_all(name).context(recovery_failed())?;
		} else {
			root.remove_file(name).context(recovery_failed())?;
		}
		root.sync().context(recovery_failed())?;
		if let Some(cancellation) = cancellation {
			check_cancelled(cancellation)?;
		}
	}

	if let Some(cancellation) = cancellation {
		check_cancelled(cancellation)?;
	}
	temp.remove_dir_all(operation_name).context(recovery_failed())?;
	temp.sync().context(recovery_failed())?;
	for (name, _, _) in ARTIFACTS {
		if root.exists(name).context(recovery_failed())? {
			return Err(report!(recovery_failed()));
		}
	}
	root.sync().context(recovery_failed())
}

fn read_record(operation_dir: &SafeDir, expected_operation_id: Uuid) -> Result<OperationRecord, ErrorMarker> {
	let record = read_record_file(operation_dir, OPERATION_FILE)?;
	if record.schema_version != 1
		|| record.operation != OperationKind::Initialize
		|| record.operation_id != expected_operation_id
		|| record.phase != OperationPhase::Publishing
	{
		return Err(report!(recovery_failed()));
	}
	Ok(record)
}

fn read_record_file(operation_dir: &SafeDir, name: &str) -> Result<OperationRecord, ErrorMarker> {
	let contents = operation_dir.read_regular(name).context(recovery_failed())?;
	let text = from_utf8(&contents).context(recovery_failed())?;
	toml::from_str(text).context(recovery_failed())
}

fn move_once(stage: &SafeDir, root: &SafeDir, name: &str, directory: bool) -> Result<(), ErrorMarker> {
	let source_exists = stage.exists(name).context(recovery_failed())?;
	let destination_exists = root.exists(name).context(recovery_failed())?;
	match (source_exists, destination_exists) {
		(true, false) => {
			validate_entry(stage, name, directory)?;
			stage.rename_to(name, root, name).context(recovery_failed())?;
			validate_entry(root, name, directory)
		}
		(false, true) => validate_entry(root, name, directory),
		_ => Err(report!(recovery_failed())),
	}
}

fn validate_entry(directory: &SafeDir, name: &str, expected_dir: bool) -> Result<(), ErrorMarker> {
	let metadata = directory.symlink_metadata(name).context(recovery_failed())?;
	if is_reparse(&metadata) || metadata.is_dir() != expected_dir || metadata.is_file() == expected_dir {
		return Err(report!(recovery_failed()));
	}
	if expected_dir {
		directory.open_dir(name).context(recovery_failed())?;
	} else {
		directory.read_regular(name).context(recovery_failed())?;
	}
	Ok(())
}

fn remove_empty_temp(root: &SafeDir, temp: SafeDir) -> Result<(), ErrorMarker> {
	if root.exists("mods.toml").context(recovery_failed())? {
		return Ok(());
	}
	if !temp.entries().context(recovery_failed())?.is_empty() {
		return Err(report!(recovery_failed()));
	}
	drop(temp);
	root.remove_dir("temp").context(recovery_failed())?;
	root.sync().context(recovery_failed())
}

fn recovery_failed() -> ErrorMarker {
	ErrorMarker::environment_recovery_failed(Some("recovery"))
}
