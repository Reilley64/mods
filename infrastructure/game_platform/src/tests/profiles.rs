use super::support::adapter_with_profiles;
use super::support::fixture;
#[cfg(any(unix, windows))]
use super::support::symlink_file;
#[cfg(not(windows))]
use crate::GamePlatformAdapter;
use crate::steam;
use rootcause::Result;
use std::fs;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

#[cfg(not(windows))]
#[test]
fn empty_non_windows_system_known_folders_mean_no_profile_sources() -> Result<()> {
	let (_fixture, game) = fixture()?;
	let binding = steam::validate(&game)?;
	let sources = GamePlatformAdapter::system().load_profile_sources(&binding, &CancellationToken::new())?;
	assert!(sources.files.iter().all(|source| source.contents.is_none()));
	assert_eq!(sources.fallout_default_ini, b"[Archive]\n");
	Ok(())
}

#[test]
fn profile_sources_are_read_from_known_folder_capabilities() -> Result<()> {
	let (game_fixture, game) = fixture()?;
	let profile_fixture = TempDir::new()?;
	let profile_root = fs::canonicalize(profile_fixture.path())?;
	let documents = profile_root.join("Documents");
	let local_app_data = profile_root.join("LocalAppData");
	fs::create_dir_all(documents.join("My Games/FalloutNV"))?;
	fs::create_dir_all(local_app_data.join("FalloutNV"))?;
	fs::write(documents.join("My Games/FalloutNV/Fallout.ini"), b"[Archive]\n")?;
	fs::write(local_app_data.join("FalloutNV/plugins.txt"), b"Example.esp\r\n")?;
	let binding = steam::validate(&game)?;
	let sources = adapter_with_profiles(documents, local_app_data)
		.load_profile_sources(&binding, &CancellationToken::new())?;
	assert_eq!(sources.files[0].contents, Some(b"[Archive]\n".to_vec()));
	assert_eq!(sources.files[5].contents, Some(b"Example.esp\r\n".to_vec()));
	assert_eq!(sources.fallout_default_ini, b"[Archive]\n");
	drop(game_fixture);
	Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn symlinked_profile_source_is_rejected() -> Result<()> {
	let (_game_fixture, game) = fixture()?;
	let profile_fixture = TempDir::new()?;
	let profile_root = fs::canonicalize(profile_fixture.path())?;
	let documents = profile_root.join("Documents");
	let local_app_data = profile_root.join("LocalAppData");
	let fallout = documents.join("My Games/FalloutNV");
	fs::create_dir_all(&fallout)?;
	fs::create_dir_all(local_app_data.join("FalloutNV"))?;
	let target = profile_root.join("outside.ini");
	fs::write(&target, b"outside")?;
	symlink_file(&target, &fallout.join("Fallout.ini"))?;
	let binding = steam::validate(&game)?;
	assert!(adapter_with_profiles(documents, local_app_data)
		.load_profile_sources(&binding, &CancellationToken::new())
		.is_err());
	Ok(())
}
