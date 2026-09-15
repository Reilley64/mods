use super::support::game_fixture;
#[cfg(any(unix, windows))]
use super::support::symlink_dir;
#[cfg(any(unix, windows))]
use super::support::symlink_file;
use crate::steam::validate;
use rootcause::Result;
use std::fs;
use std::io::Error as IoError;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn validates_structured_manifest_executable_and_nonzero_build() -> Result<()> {
	let (_temp, game) = game_fixture(42)?;
	assert_eq!(validate(&game)?.observed_build_id().get(), 42);
	Ok(())
}

#[test]
fn rejects_reserved_base_archive_zero_build_and_stray_manifest_keys() -> Result<()> {
	let (_temp, game) = game_fixture(0)?;
	assert!(validate(&game).is_err());
	let manifest = game
		.parent()
		.and_then(Path::parent)
		.ok_or_else(|| IoError::other("missing steamapps"))?
		.join("appmanifest_22380.acf");
	fs::write(
		&manifest,
		"\"appid\" \"22380\"\n\"buildid\" \"42\"\n\"installdir\" \"Fallout New Vegas\"",
	)?;
	assert!(validate(&game).is_err());
	fs::write(
		&manifest,
		"\"AppState\" { \"appid\" \"22380\" \"buildid\" \"42\" \"installdir\" \"Fallout New Vegas\" }",
	)?;
	fs::write(game.join("Data/Fallout - Invalidation.bsa"), b"conflict")?;
	assert!(validate(&game).is_err());
	Ok(())
}

#[test]
fn rejects_manifest_outside_a_steamapps_parent() -> Result<()> {
	let (temp, game) = game_fixture(42)?;
	let arbitrary = temp.path().join("arbitrary");
	fs::rename(temp.path().join("steamapps"), &arbitrary)?;
	let relocated = arbitrary
		.join("common")
		.join(game.file_name().ok_or_else(|| IoError::other("missing game name"))?);
	assert!(validate(&relocated).is_err());
	Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn rejects_symlinked_game_files_directories_and_manifest() -> Result<()> {
	let (_temp, game) = game_fixture(42)?;
	let executable = game.join("FalloutNV.exe");
	let executable_target = game.join("FalloutNV.real.exe");
	fs::rename(&executable, &executable_target)?;
	symlink_file(&executable_target, &executable)?;
	assert!(validate(&game).is_err());

	let (_temp, game) = game_fixture(42)?;
	let data = game.join("Data");
	let data_target = game.join("RealData");
	fs::rename(&data, &data_target)?;
	symlink_dir(&data_target, &data)?;
	assert!(validate(&game).is_err());

	let (_temp, game) = game_fixture(42)?;
	let manifest = game
		.parent()
		.and_then(Path::parent)
		.ok_or_else(|| IoError::other("missing steamapps"))?
		.join("appmanifest_22380.acf");
	let manifest_target = manifest.with_extension("real.acf");
	fs::rename(&manifest, &manifest_target)?;
	symlink_file(&manifest_target, &manifest)?;
	assert!(validate(&game).is_err());
	Ok(())
}

#[cfg(any(unix, windows))]
#[test]
fn rejects_symlinked_game_ancestor_and_directory() -> Result<()> {
	let (fixture, game) = game_fixture(42)?;
	let holder = TempDir::new()?;
	let linked_ancestor = holder.path().join("linked-library");
	symlink_dir(fixture.path(), &linked_ancestor)?;
	assert!(validate(&linked_ancestor.join("steamapps/common/Fallout New Vegas")).is_err());
	drop(game);

	let (_temp, game) = game_fixture(42)?;
	let target = game.with_file_name("Real Fallout New Vegas");
	fs::rename(&game, &target)?;
	symlink_dir(&target, &game)?;
	assert!(validate(&game).is_err());
	Ok(())
}
