use crate::SettingsAccess;
use crate::refuse_unfinished_operation_in_temp;
use application::ErrorMarker;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::Path;
use tokio::fs::write;
use tokio_util::sync::CancellationToken;

const MANIFEST: &str = "mods.toml";

/// Writes a validated manifest directly over the canonical file.
///
/// Pending work in `temp` still blocks the write. There is no staging copy,
/// flush, rename, or source comparison: a concurrent edit may be overwritten,
/// and recovery belongs to version-control rollback.
///
/// # Errors
///
/// Returns pending-work, validation, write, or cancellation failures.
pub(crate) async fn replace_validated(
	root: &Path,
	contents: &str,
	cancellation: &CancellationToken,
	validate: impl Fn(&str) -> bool,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	refuse_unfinished_operation_in_temp(&root.join("temp"), SettingsAccess::Mutation).await?;

	if !validate(contents) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	write(root.join(MANIFEST), contents)
		.await
		.context(ErrorMarker::environment_root_unsafe())
}

#[cfg(test)]
mod tests {
	use super::MANIFEST;
	use super::replace_validated;
	use application::ErrorCode;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;
	use std::fs;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	fn fixture() -> Result<TempDir> {
		let fixture = TempDir::new().into_report()?;
		fs::create_dir(fixture.path().join("temp")).into_report()?;
		fs::write(fixture.path().join(MANIFEST), "old").into_report()?;
		Ok(fixture)
	}

	#[tokio::test]
	async fn replacement_writes_the_validated_manifest() -> Result<()> {
		let fixture = fixture()?;

		replace_validated(fixture.path(), "new", &CancellationToken::new(), |candidate| {
			candidate == "new"
		})
		.await?;

		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "new");
		Ok(())
	}

	#[tokio::test]
	async fn existing_temp_entry_refuses_replacement_without_mutating_it() -> Result<()> {
		let fixture = fixture()?;
		let pending = fixture.path().join("temp/pending");
		fs::write(&pending, "keep").into_report()?;

		let result = replace_validated(fixture.path(), "new", &CancellationToken::new(), |_| true).await;

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::ManualCleanupRequired
		));
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		assert_eq!(fs::read_to_string(pending).into_report()?, "keep");
		Ok(())
	}

	#[tokio::test]
	async fn invalid_content_is_not_written() -> Result<()> {
		let fixture = fixture()?;

		let result = replace_validated(fixture.path(), "invalid", &CancellationToken::new(), |_| false).await;

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		assert_eq!(fs::read_to_string(fixture.path().join(MANIFEST)).into_report()?, "old");
		Ok(())
	}
}
