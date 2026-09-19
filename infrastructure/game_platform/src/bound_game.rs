use crate::steam;
use application::ErrorMarker;
use cap_std::fs::Dir;
use domain::GameBinding;
use rootcause::Result;
use rootcause::report;
use tokio_util::sync::CancellationToken;

pub(super) fn reopen_bound_game(expected: &GameBinding, cancellation: &CancellationToken) -> Result<Dir, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let game = steam::reopen(expected)?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	Ok(game)
}

#[cfg(test)]
mod tests {
	use super::reopen_bound_game;
	use crate::steam;
	use application::ErrorCode;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use rootcause::Result;
	use rootcause::report;
	use std::fs;
	use std::io::Error as IoError;
	use std::path::Path;
	use std::path::PathBuf;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn reopening_rejects_a_changed_steam_build() -> Result<()> {
		let (_temp, game) = fixture()?;
		let binding = steam::validate(&game)?;
		let steamapps = game
			.parent()
			.and_then(Path::parent)
			.ok_or_else(|| report!(IoError::other("fixture has no steamapps directory")))?;
		fs::write(
			steamapps.join("appmanifest_22380.acf"),
			concat!(
				"\"AppState\"\n{\n",
				"\"appid\" \"22380\"\n",
				"\"buildid\" \"89\"\n",
				"\"installdir\" \"Fallout New Vegas\"\n}"
			),
		)?;

		let result = reopen_bound_game(&binding, &CancellationToken::new());

		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::GameBuildMismatch),
		);
		Ok(())
	}

	#[test]
	fn cancellation_stops_before_reopening_the_game() -> Result<()> {
		let missing = if cfg!(windows) {
			PathBuf::from(r"C:\game-that-must-not-be-opened")
		} else {
			PathBuf::from("/game-that-must-not-be-opened")
		};
		let path = GameInstallationPath::new(missing)?;
		let binding = GameBinding::new(path, SteamBuildId::new(1)?);
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = reopen_bound_game(&binding, &cancellation);

		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::OperationCancelled),
		);
		Ok(())
	}

	fn fixture() -> Result<(TempDir, PathBuf)> {
		let temp = TempDir::new()?;
		let game = fs::canonicalize(temp.path())?.join("steam/steamapps/common/Fallout New Vegas");
		fs::create_dir_all(game.join("Data"))?;
		fs::write(game.join("FalloutNV.exe"), b"exe")?;
		fs::write(game.join("Fallout_default.ini"), b"[Archive]\n")?;
		fs::write(
			game.parent()
				.and_then(Path::parent)
				.ok_or_else(|| report!(IoError::other("missing steamapps")))?
				.join("appmanifest_22380.acf"),
			concat!(
				"\"AppState\"\n{\n",
				"\"appid\" \"22380\"\n",
				"\"buildid\" \"88\"\n",
				"\"installdir\" \"Fallout New Vegas\"\n}"
			),
		)?;
		Ok((temp, game))
	}
}
