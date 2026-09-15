use crate::fs_access;
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
use uuid::Uuid;

const MANIFEST: &str = "mods.toml";
const STAGED_MANIFEST: &str = "mods.toml.tmp";

pub(crate) fn replace_validated(
	root: &Dir,
	contents: &str,
	cancellation: &CancellationToken,
	validate: impl Fn(&str) -> bool,
) -> Result<(), ErrorMarker> {
	continue_if_active(cancellation)?;
	if !validate(contents) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	continue_if_active(cancellation)?;
	let temp = fs_access::open_dir(root, Path::new("temp")).context(ErrorMarker::environment_root_unsafe())?;
	let operation_name = Uuid::new_v4().to_string();
	let operation_dir = fs_access::create_dir(&temp, Path::new(&operation_name))
		.context(ErrorMarker::environment_root_unsafe())?;

	let publication = (|| {
		continue_if_active(cancellation)?;
		let mut file = fs_access::create_new_regular(&operation_dir, Path::new(STAGED_MANIFEST))
			.context(ErrorMarker::environment_root_unsafe())?;
		continue_if_active(cancellation)?;
		file.write_all(contents.as_bytes())
			.context(ErrorMarker::environment_root_unsafe())?;
		continue_if_active(cancellation)?;
		file.sync_all().context(ErrorMarker::environment_root_unsafe())?;
		continue_if_active(cancellation)?;
		file.seek(SeekFrom::Start(0))
			.context(ErrorMarker::environment_root_unsafe())?;
		let mut staged = String::new();
		file.read_to_string(&mut staged)
			.context(ErrorMarker::environment_root_unsafe())?;
		continue_if_active(cancellation)?;
		if !validate(&staged) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		drop(file);
		continue_if_active(cancellation)?;

		operation_dir
			.rename(Path::new(STAGED_MANIFEST), root, Path::new(MANIFEST))
			.context(ErrorMarker::environment_root_unsafe())?;
		fs_access::sync_dir(root).context(ErrorMarker::environment_root_unsafe())
	})();

	// A failed UUID operation remains disposable recovery work. Returning before cleanup preserves
	// its exact state and avoids hiding the publication failure behind a second cleanup failure.
	publication?;

	drop(operation_dir);
	temp.remove_dir(Path::new(&operation_name))
		.context(ErrorMarker::environment_root_unsafe())?;
	fs_access::sync_dir(&temp).context(ErrorMarker::environment_root_unsafe())
}

fn continue_if_active(cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use application::ErrorCode;
	use cap_std::ambient_authority;
	use std::cell::Cell;
	use std::fs;
	use std::io::Result as IoResult;
	use tempfile::TempDir;

	#[test]
	fn replacement_stages_only_below_a_uuid_operation_directory() -> Result<()> {
		let fixture = TempDir::new().into_report()?;
		fs::create_dir(fixture.path().join("temp")).into_report()?;
		fs::write(fixture.path().join(MANIFEST), "old").into_report()?;
		let root = Dir::open_ambient_dir(fixture.path(), ambient_authority()).into_report()?;

		replace_validated(&root, "new", &CancellationToken::new(), |candidate| candidate == "new")?;

		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "new");
		assert!(fs::read_dir(fixture.path().join("temp"))
			.into_report()?
			.next()
			.is_none());
		assert!(fs::read_dir(fixture.path()).into_report()?.all(|entry| entry
			.is_ok_and(|entry| !entry.file_name().to_string_lossy().starts_with(".mods.toml."))));
		Ok(())
	}

	#[test]
	fn cancellation_at_publication_preserves_the_durable_stage() -> Result<()> {
		let fixture = TempDir::new().into_report()?;
		fs::create_dir(fixture.path().join("temp")).into_report()?;
		fs::write(fixture.path().join(MANIFEST), "old").into_report()?;
		let root = Dir::open_ambient_dir(fixture.path(), ambient_authority()).into_report()?;
		let cancellation = CancellationToken::new();
		let cancellation_during_validation = cancellation.clone();
		let calls = Cell::new(0);

		let result = replace_validated(&root, "new", &cancellation, |_| {
			calls.set(calls.get() + 1);
			if calls.get() == 2 {
				cancellation_during_validation.cancel();
			}
			true
		});

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::OperationCancelled
		));
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		let entries = fs::read_dir(fixture.path().join("temp"))
			.into_report()?
			.collect::<IoResult<Vec<_>>>()
			.into_report()?;
		assert_eq!(entries.len(), 1);
		assert!(entries[0].path().join(STAGED_MANIFEST).is_file());
		Ok(())
	}

	#[test]
	fn cancellation_before_staging_leaves_no_partial_work() -> Result<()> {
		let fixture = TempDir::new().into_report()?;
		fs::create_dir(fixture.path().join("temp")).into_report()?;
		fs::write(fixture.path().join(MANIFEST), "old").into_report()?;
		let root = Dir::open_ambient_dir(fixture.path(), ambient_authority()).into_report()?;
		let cancellation = CancellationToken::new();
		let cancellation_clone = cancellation.clone();
		cancellation_clone.cancel();

		let result = replace_validated(&root, "new", &cancellation, |candidate| candidate == "new");

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::OperationCancelled
		));
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		assert!(fs::read_dir(fixture.path().join("temp"))
			.into_report()?
			.next()
			.is_none());
		Ok(())
	}

	#[test]
	fn failed_staged_validation_leaves_recoverable_uuid_work_without_publishing() -> Result<()> {
		let fixture = TempDir::new().into_report()?;
		fs::create_dir(fixture.path().join("temp")).into_report()?;
		fs::write(fixture.path().join(MANIFEST), "old").into_report()?;
		let root = Dir::open_ambient_dir(fixture.path(), ambient_authority()).into_report()?;
		let calls = Cell::new(0);

		let result = replace_validated(&root, "new", &CancellationToken::new(), |_| {
			calls.set(calls.get() + 1);
			calls.get() == 1
		});

		assert!(result.is_err());
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		let entries = fs::read_dir(fixture.path().join("temp"))
			.into_report()?
			.collect::<IoResult<Vec<_>>>()
			.into_report()?;
		assert_eq!(entries.len(), 1);
		assert!(Uuid::parse_str(&entries[0].file_name().to_string_lossy()).is_ok());
		assert!(entries[0].path().join(STAGED_MANIFEST).is_file());
		Ok(())
	}
}
