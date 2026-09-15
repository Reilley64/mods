use crate::fs_access;
use application::ErrorMarker;
use domain::EnvironmentRoot;
use domain::GameInstallationPath;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub(super) fn prove_separation(root: &EnvironmentRoot, game: &GameInstallationPath) -> Result<(), ErrorMarker> {
	let (game, _) = fs_access::open_ambient_dir(game.as_path()).context(ErrorMarker::game_install_invalid())?;
	let (root_ancestor, root_exists) =
		fs_access::open_existing_ancestor(root.as_path()).context(ErrorMarker::environment_root_unsafe())?;

	if fs_access::is_ancestor_of(&game, &root_ancestor).context(ErrorMarker::environment_root_unsafe())? {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	if root_exists
		&& fs_access::is_ancestor_of(&root_ancestor, &game).context(ErrorMarker::environment_root_unsafe())?
	{
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	Ok(())
}
