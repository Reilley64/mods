use super::support::adapter_with_discovery;
use super::support::adapter_without_sources;
use super::support::fixture;
use application::ports::GameInstallationSource;
use domain::EnvironmentRoot;
use domain::GameInstallationPath;
use rootcause::Result;
use std::env;
use std::fs;
use std::io::Error as IoError;
use std::path::Path;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

#[test]
fn explicit_precedes_environment_and_discovery() -> Result<()> {
	let (temp, game) = fixture()?;
	let root = EnvironmentRoot::new(fs::canonicalize(temp.path())?.join("environment"))?;
	let path = GameInstallationPath::new(game)?;
	let resolved =
		adapter_without_sources().resolve(Some(path.clone()), Some(path), &root, &CancellationToken::new())?;
	assert_eq!(resolved.source, GameInstallationSource::Explicit);
	Ok(())
}

#[test]
fn automatic_discovery_precedes_bethesda_hint_and_fallback_is_typed() -> Result<()> {
	let (steam_fixture, steam_game) = fixture()?;
	let (bethesda_fixture, bethesda_game) = fixture()?;
	let root =
		EnvironmentRoot::new(fs::canonicalize(env::temp_dir())?.join("game-platform-discovery-environment"))?;
	let steam_root = steam_game
		.parent()
		.and_then(Path::parent)
		.and_then(Path::parent)
		.ok_or_else(|| IoError::other("missing steam root"))?
		.to_path_buf();
	let resolved = adapter_with_discovery(vec![steam_root], vec![bethesda_game.clone()]).resolve(
		None,
		None,
		&root,
		&CancellationToken::new(),
	)?;
	assert_eq!(resolved.source, GameInstallationSource::Steam);
	assert_eq!(resolved.binding.observed_build_id().get(), 88);
	drop(steam_fixture);

	let fallback = adapter_with_discovery(Vec::new(), vec![bethesda_game]).resolve(
		None,
		None,
		&root,
		&CancellationToken::new(),
	)?;
	assert_eq!(fallback.source, GameInstallationSource::BethesdaRegistryFallback);
	drop(bethesda_fixture);
	Ok(())
}

#[test]
fn libraryfolders_path_participates_in_discovery() -> Result<()> {
	let (library, game) = fixture()?;
	let primary = TempDir::new()?;
	let primary_root = fs::canonicalize(primary.path())?;
	fs::create_dir_all(primary_root.join("steamapps"))?;
	let library_root = game
		.parent()
		.and_then(Path::parent)
		.and_then(Path::parent)
		.ok_or_else(|| IoError::other("missing library"))?;
	fs::write(
		primary_root.join("steamapps/libraryfolders.vdf"),
		format!(
			"\"libraryfolders\"\n{{\n\"1\"\n{{\n\"path\" \"{}\"\n}}\n}}",
			library_root.display()
		),
	)?;
	let root = EnvironmentRoot::new(primary_root.join("environment"))?;
	let resolved = adapter_with_discovery(vec![primary_root], Vec::new()).resolve(
		None,
		None,
		&root,
		&CancellationToken::new(),
	)?;
	assert_eq!(resolved.source, GameInstallationSource::Steam);
	assert_eq!(resolved.binding.game_directory().as_path(), fs::canonicalize(game)?);
	drop(library);
	Ok(())
}
