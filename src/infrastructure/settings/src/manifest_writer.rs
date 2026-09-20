use crate::SettingsAccess;
use crate::fs_access::create_dir;
use crate::fs_access::create_new_regular;
use crate::fs_access::open_dir;
use crate::fs_access::sync_dir;
use crate::refuse_unfinished_operation_in_temp;
use application::ErrorMarker;
use cap_std::fs::Dir;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use tokio_util::sync::CancellationToken;

const MANIFEST: &str = "mods.toml";
const OPERATION: &str = "operation";

pub(crate) fn replace_validated(
	root: &Dir,
	contents: &str,
	cancellation: &CancellationToken,
	validate: impl Fn(&str) -> bool,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let temp = open_dir(root, Path::new("temp")).context(ErrorMarker::environment_root_unsafe())?;
	refuse_unfinished_operation_in_temp(&temp, SettingsAccess::Mutation)?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let operation = create_dir(&temp, Path::new(OPERATION)).context(ErrorMarker::environment_root_unsafe())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mut staged =
		create_new_regular(&operation, Path::new(MANIFEST)).context(ErrorMarker::environment_root_unsafe())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	staged.write_all(contents.as_bytes())
		.context(ErrorMarker::environment_root_unsafe())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	staged.sync_all().context(ErrorMarker::environment_root_unsafe())?;
	sync_dir(&operation).context(ErrorMarker::environment_root_unsafe())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	staged.seek(SeekFrom::Start(0))
		.context(ErrorMarker::environment_root_unsafe())?;
	let mut staged_contents = String::new();
	staged.read_to_string(&mut staged_contents)
		.context(ErrorMarker::environment_root_unsafe())?;
	if !validate(&staged_contents) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	drop(staged);

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	operation
		.rename(Path::new(MANIFEST), root, Path::new(MANIFEST))
		.context(ErrorMarker::environment_root_unsafe())?;

	// This cross-parent rename is durable only after both changed directories are synced. Sync the
	// destination first so the canonical entry is durable before making its staging removal durable.
	sync_dir(root).context(ErrorMarker::environment_root_unsafe())?;
	sync_dir(&operation).context(ErrorMarker::environment_root_unsafe())?;

	// The committed setting no longer depends on removal of its empty staging directory.
	drop(operation);
	if temp.remove_dir(Path::new(OPERATION)).is_ok() {
		let _ = sync_dir(&temp);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::MANIFEST;
	use super::OPERATION;
	use super::replace_validated;
	use application::ErrorCode;
	use cap_std::ambient_authority;
	use cap_std::fs::Dir;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;
	use std::cell::Cell;
	use std::fs;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	fn fixture() -> Result<(TempDir, Dir)> {
		let fixture = TempDir::new().into_report()?;
		fs::create_dir(fixture.path().join("temp")).into_report()?;
		fs::write(fixture.path().join(MANIFEST), "old").into_report()?;
		let root = Dir::open_ambient_dir(fixture.path(), ambient_authority()).into_report()?;
		Ok((fixture, root))
	}

	#[test]
	fn replacement_uses_the_exclusive_operation_directory_and_cleans_it() -> Result<()> {
		let (fixture, root) = fixture()?;

		replace_validated(&root, "new", &CancellationToken::new(), |candidate| candidate == "new")?;

		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "new");
		assert!(fs::read_dir(fixture.path().join("temp"))
			.into_report()?
			.next()
			.is_none());
		Ok(())
	}

	#[test]
	fn existing_temp_entry_refuses_replacement_without_mutating_it() -> Result<()> {
		let (fixture, root) = fixture()?;
		let pending = fixture.path().join("temp/pending");
		fs::write(&pending, "keep").into_report()?;

		let result = replace_validated(&root, "new", &CancellationToken::new(), |_| true);

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::ManualCleanupRequired
		));
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		assert_eq!(fs::read_to_string(pending).into_report()?, "keep");
		Ok(())
	}

	#[test]
	fn failed_staged_validation_preserves_operation_state() -> Result<()> {
		let (fixture, root) = fixture()?;

		let result = replace_validated(&root, "invalid", &CancellationToken::new(), |_| false);

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		assert_eq!(
			fs::read_to_string(fixture.path().join("temp").join(OPERATION).join(MANIFEST)).into_report()?,
			"invalid"
		);
		Ok(())
	}

	#[test]
	fn final_cancellation_preserves_the_validated_stage() -> Result<()> {
		let (fixture, root) = fixture()?;
		let cancellation = CancellationToken::new();
		let cancel_during_validation = cancellation.clone();

		let result = replace_validated(&root, "new", &cancellation, |candidate| {
			cancel_during_validation.cancel();
			candidate == "new"
		});

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::OperationCancelled
		));
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		assert_eq!(
			fs::read_to_string(fixture.path().join("temp").join(OPERATION).join(MANIFEST)).into_report()?,
			"new"
		);
		Ok(())
	}

	#[test]
	fn cleanup_failure_after_commit_returns_success_and_leaves_refusal_state() -> Result<()> {
		let (fixture, root) = fixture()?;
		let operation_path = fixture.path().join("temp").join(OPERATION);
		let validation_calls = Cell::new(0);

		replace_validated(&root, "new", &CancellationToken::new(), |candidate| {
			validation_calls.set(validation_calls.get() + 1);
			fs::write(operation_path.join("leftover"), "keep").is_ok() && candidate == "new"
		})?;

		assert_eq!(validation_calls.get(), 1);
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "new");
		assert_eq!(
			fs::read_to_string(operation_path.join("leftover")).into_report()?,
			"keep"
		);
		let later = replace_validated(&root, "later", &CancellationToken::new(), |_| true);
		assert!(matches!(
			later,
			Err(report) if report.current_context().code() == ErrorCode::ManualCleanupRequired
		));
		Ok(())
	}
}
