use crate::ErrorMarker;
use crate::ports::EnvironmentPlan;
use domain::GameBinding;
use domain::ModName;
use domain::OutputTarget;
use domain::ProviderIdentity;
use domain::WorkingDirectory;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub(crate) fn child_working_directory(
	working_directory: Option<WorkingDirectory>,
	game_binding: &GameBinding,
) -> Result<WorkingDirectory, ErrorMarker> {
	// The game resolves `Data\` and its script-extender loaders relative to its
	// working directory, so a child without `--cwd` starts in the game directory.
	working_directory.map_or_else(
		|| {
			WorkingDirectory::new(game_binding.game_directory().as_path().to_owned())
				.context(ErrorMarker::invalid_working_directory())
		},
		Ok,
	)
}

pub(crate) fn output_mod(plan: &EnvironmentPlan, output_target: OutputTarget) -> Result<Option<ModName>, ErrorMarker> {
	let OutputTarget::DataMod(name) = output_target else {
		return Ok(None);
	};

	let provider = plan
		.providers
		.iter()
		.find(
			|provider| matches!(&provider.identity, ProviderIdentity::DataMod { mod_name, .. } if *mod_name == name),
		)
		.ok_or_else(|| report!(ErrorMarker::output_target_not_found().with_mod_name(name.clone())))?;

	if !provider.enabled {
		return Err(report!(ErrorMarker::output_target_disabled().with_mod_name(name)));
	}

	Ok(Some(name))
}
