use super::support::adapter_without_sources;
use super::support::fixture;
#[cfg(any(unix, windows))]
use super::support::symlink_dir;
use domain::EnvironmentRoot;
use domain::GameInstallationPath;
use rootcause::Result;
#[cfg(windows)]
use std::ffi::OsString;
use std::fs;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt as _;
#[cfg(windows)]
use std::os::windows::ffi::OsStringExt as _;
#[cfg(windows)]
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

#[test]
fn rejects_missing_environment_root_inside_game() -> Result<()> {
	let (_temp, game) = fixture()?;
	let root = EnvironmentRoot::new(game.join("mods-environment"))?;
	let path = GameInstallationPath::new(game)?;
	assert!(adapter_without_sources()
		.resolve(Some(path), None, &root, &CancellationToken::new())
		.is_err());
	Ok(())
}

#[test]
fn rejects_existing_environment_root_inside_game() -> Result<()> {
	let (_temp, game) = fixture()?;
	let environment = game.join("mods-environment");
	fs::create_dir(&environment)?;
	let root = EnvironmentRoot::new(environment)?;
	let path = GameInstallationPath::new(game)?;
	assert!(adapter_without_sources()
		.resolve(Some(path), None, &root, &CancellationToken::new())
		.is_err());
	Ok(())
}

#[test]
fn rejects_game_inside_environment_root() -> Result<()> {
	let (temp, game) = fixture()?;
	let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?)?;
	let path = GameInstallationPath::new(game)?;
	assert!(adapter_without_sources()
		.resolve(Some(path), None, &root, &CancellationToken::new())
		.is_err());
	Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn rejects_symlinked_environment_root_during_containment_proof() -> Result<()> {
	let (_fixture, game) = fixture()?;
	let real_root = game.join("real-environment");
	fs::create_dir(&real_root)?;
	let holder = TempDir::new()?;
	let linked_root = holder.path().join("linked-environment");
	symlink_dir(&real_root, &linked_root)?;
	let root = EnvironmentRoot::new(linked_root)?;
	let path = GameInstallationPath::new(game)?;
	assert!(adapter_without_sources()
		.resolve(Some(path), None, &root, &CancellationToken::new())
		.is_err());
	Ok(())
}

#[cfg(windows)]
#[test]
fn rejects_case_alias_of_environment_ancestor() -> Result<()> {
	let (temp, game) = fixture()?;
	let alias = alternate_ascii_case(&fs::canonicalize(temp.path())?);
	assert!(alias.exists());
	let root = EnvironmentRoot::new(alias)?;
	let path = GameInstallationPath::new(game)?;
	assert!(adapter_without_sources()
		.resolve(Some(path), None, &root, &CancellationToken::new())
		.is_err());
	Ok(())
}

#[cfg(windows)]
fn alternate_ascii_case(path: &Path) -> PathBuf {
	let units = path.as_os_str().encode_wide().map(|unit| match unit {
		97..=122 => unit - 32,
		65..=90 => unit + 32,
		_ => unit,
	});
	PathBuf::from(OsString::from_wide(&units.collect::<Vec<_>>()))
}
