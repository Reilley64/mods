use super::support::adapter_without_sources;
use super::support::fixture;
use crate::steam;
use application::ErrorCode;
use domain::EnvironmentRoot;
use domain::GameInstallationPath;
use rootcause::Result;
use std::fs;
use tokio_util::sync::CancellationToken;

#[test]
fn cancelled_resolution_stops_before_validation() -> Result<()> {
	let (temp, game) = fixture()?;
	let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?.join("environment"))?;
	let path = GameInstallationPath::new(game)?;
	let cancellation = CancellationToken::new();
	cancellation.cancel();

	let result = adapter_without_sources().resolve(Some(path), None, &root, &cancellation);
	assert_eq!(
		result.as_ref().err().map(|error| error.current_context().code()),
		Some(ErrorCode::OperationCancelled),
	);
	Ok(())
}

#[test]
fn cancelled_profile_load_stops_before_reads() -> Result<()> {
	let (_temp, game) = fixture()?;
	let binding = steam::validate(&game)?;
	let adapter = adapter_without_sources();
	let cancellation = CancellationToken::new();
	cancellation.cancel();

	let result = adapter.load_profile_sources(&binding, &cancellation);
	assert_eq!(
		result.as_ref().err().map(|error| error.current_context().code()),
		Some(ErrorCode::OperationCancelled),
	);
	Ok(())
}
