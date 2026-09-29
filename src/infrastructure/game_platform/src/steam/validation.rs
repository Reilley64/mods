use super::io::read_required_text;
use super::manifest;
use application::ErrorMarker;
use domain::GameBinding;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::ffi::OsStr;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs::canonicalize;
use tokio::fs::metadata;
use tokio::fs::read_dir;

pub(crate) async fn validate(path: &Path) -> Result<GameBinding, ErrorMarker> {
	Ok(open_validated(path).await?.binding)
}

/// Validates the bound installation again and returns its canonical directory.
pub(crate) async fn reopen(expected: &GameBinding) -> Result<PathBuf, ErrorMarker> {
	let validated = open_validated(expected.game_directory().as_path()).await?;

	Ok(validated.directory)
}

struct ValidatedGameInstallation {
	binding: GameBinding,
	directory: PathBuf,
}

async fn open_validated(path: &Path) -> Result<ValidatedGameInstallation, ErrorMarker> {
	let game_name = path
		.file_name()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	let common_path = path
		.parent()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	if !has_name(common_path, "common") {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	let steamapps_path = common_path
		.parent()
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))?;
	if !has_name(steamapps_path, "steamapps") {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}

	require_directory(steamapps_path).await?;
	require_directory(common_path).await?;
	let game = metadata(path).await.map_err(|error| {
		let marker = if error.kind() == ErrorKind::NotFound {
			ErrorMarker::game_install_not_found()
		} else {
			ErrorMarker::game_install_invalid()
		};
		report!(error).context(marker)
	})?;
	if !game.is_dir() {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	let canonical_game = canonicalize(path).await.context(ErrorMarker::game_install_invalid())?;

	require_file(&path.join("FalloutNV.exe")).await?;
	require_file(&path.join("Fallout_default.ini")).await?;
	let mut data = read_dir(path.join("Data"))
		.await
		.context(ErrorMarker::game_install_invalid())?;
	while let Some(entry) = data.next_entry().await.context(ErrorMarker::game_install_invalid())? {
		if os_eq_ignore_ascii_case(&entry.file_name(), "Fallout - Invalidation.bsa") {
			return Err(report!(ErrorMarker::game_install_invalid()));
		}
	}

	let text = read_required_text(&steamapps_path.join("appmanifest_22380.acf")).await?;
	let (app_id, install_dir) = manifest::fields(&text).context(ErrorMarker::game_install_invalid())?;
	if app_id != 22_380
		|| !manifest::is_install_directory_name(&install_dir)
		|| !os_eq_ignore_ascii_case(game_name, &install_dir)
	{
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	let game_path =
		GameInstallationPath::new(canonical_game.clone()).context(ErrorMarker::game_install_invalid())?;
	Ok(ValidatedGameInstallation {
		binding: GameBinding::new(game_path),
		directory: canonical_game,
	})
}

async fn require_directory(path: &Path) -> Result<(), ErrorMarker> {
	let metadata = metadata(path).await.context(ErrorMarker::game_install_invalid())?;
	if !metadata.is_dir() {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	Ok(())
}

async fn require_file(path: &Path) -> Result<(), ErrorMarker> {
	let metadata = metadata(path).await.context(ErrorMarker::game_install_invalid())?;
	if !metadata.is_file() {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	Ok(())
}

fn has_name(path: &Path, expected: &str) -> bool {
	path.file_name()
		.is_some_and(|name| os_eq_ignore_ascii_case(name, expected))
}

fn os_eq_ignore_ascii_case(value: &OsStr, expected: &str) -> bool {
	value.to_str().is_some_and(|value| value.eq_ignore_ascii_case(expected))
}

#[cfg(test)]
mod tests {
	use super::validate;
	use rootcause::Result;
	use std::fs;
	use std::io::Error as IoError;
	use std::path::Path;
	use std::path::PathBuf;
	use tempfile::TempDir;

	#[tokio::test]
	async fn validates_structured_manifest_and_executable() -> Result<()> {
		let (_temp, game) = game_fixture(42)?;
		assert_eq!(validate(&game).await?.game_directory().as_path(), game);
		Ok(())
	}

	#[tokio::test]
	async fn ignores_build_id_and_rejects_reserved_base_archive_and_stray_manifest_keys() -> Result<()> {
		let (_temp, game) = game_fixture(0)?;
		assert!(validate(&game).await.is_ok());
		let manifest = game
			.parent()
			.and_then(Path::parent)
			.ok_or_else(|| IoError::other("missing steamapps"))?
			.join("appmanifest_22380.acf");
		fs::write(
			&manifest,
			"\"appid\" \"22380\"\n\"buildid\" \"42\"\n\"installdir\" \"Fallout New Vegas\"",
		)?;
		assert!(validate(&game).await.is_err());
		fs::write(
			&manifest,
			"\"AppState\" { \"appid\" \"22380\" \"buildid\" \"42\" \"installdir\" \"Fallout New Vegas\" }",
		)?;
		fs::write(game.join("Data/Fallout - Invalidation.bsa"), b"conflict")?;
		assert!(validate(&game).await.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn rejects_manifest_outside_a_steamapps_parent() -> Result<()> {
		let (temp, game) = game_fixture(42)?;
		let arbitrary = temp.path().join("arbitrary");
		fs::rename(temp.path().join("steamapps"), &arbitrary)?;
		let relocated = arbitrary
			.join("common")
			.join(game.file_name().ok_or_else(|| IoError::other("missing game name"))?);
		assert!(validate(&relocated).await.is_err());
		Ok(())
	}

	fn game_fixture(build: u64) -> Result<(TempDir, PathBuf)> {
		let temp = TempDir::new()?;
		let steamapps = fs::canonicalize(temp.path())?.join("steamapps");
		let game = steamapps.join("common/Fallout New Vegas");
		fs::create_dir_all(game.join("Data"))?;
		fs::write(game.join("FalloutNV.exe"), b"fixture")?;
		fs::write(game.join("Fallout_default.ini"), b"[Archive]\n")?;
		fs::write(
			steamapps.join("appmanifest_22380.acf"),
			format!(
				concat!(
					"\"AppState\"\n{{\n\"appid\" \"22380\"\n",
					"\"buildid\" \"{}\"\n",
					"\"installdir\" \"Fallout New Vegas\"\n}}",
				),
				build,
			),
		)?;
		Ok((temp, game))
	}
}
