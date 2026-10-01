use crate::ErrorMarker;
use crate::ports::EnvironmentPlan;
use crate::ports::PrepareEnvironmentPlan;
use crate::ports::ProfileWarning;
use crate::ports::ProjectProfile;
use rootcause::Result;
use tokio_util::sync::CancellationToken;

/// A user-facing diagnostic from the advisory analytical plugin projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginWarning {
	StalePluginEntry { name: String },
	DuplicatePluginEntry { file: String, name: String },
}

pub(crate) struct PreparedEnvironment {
	pub(crate) plan: EnvironmentPlan,
	pub(crate) warnings: Vec<PluginWarning>,
}

/// Prepares the environment plan and projects its profile for any command that
/// reproduces the game's view of the environment.
///
/// # Errors
///
/// Returns the failing port's marker.
pub(crate) async fn prepare_environment(
	prepare_environment_plan: &PrepareEnvironmentPlan,
	project_profile: &ProjectProfile,
	cancellation: CancellationToken,
) -> Result<PreparedEnvironment, ErrorMarker> {
	let plan = prepare_environment_plan.call((cancellation,)).await?;

	let projection = project_profile.call((&plan,)).await?;

	let warnings = projection
		.warnings
		.into_iter()
		.map(|warning| match warning {
			ProfileWarning::Unavailable { plugin } => PluginWarning::StalePluginEntry { name: plugin },
			ProfileWarning::Duplicate { file, plugin } => {
				PluginWarning::DuplicatePluginEntry { file, name: plugin }
			}
		})
		.collect();

	Ok(PreparedEnvironment { plan, warnings })
}
