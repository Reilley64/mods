use application::ErrorMarker;
use application::ports::AdapterState;
use application::ports::EnvironmentPlan;
use application::ports::EnvironmentProvider;
use application::ports::PortFuture;
use application::ports::PrepareEnvironmentPlan;
use application::ports::ProfileProjection;
use application::ports::ProjectProfile;
use application::ports::StageProfile;
use application::ports::StagedProfile;
use domain::EnvironmentRoot;
use domain::GameBinding;
use infrastructure_environment::EnvironmentAdapter;
use infrastructure_environment::PreparedLaunch;
use infrastructure_execution::ProfileProjectionInput;
use infrastructure_execution::ProfileText;
use infrastructure_execution::VisibleProfileFile;
use infrastructure_execution::build_profile_projection;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::future::ready;
use std::sync::Arc;

/// Builds the platform-neutral preparation ports that exec and export share.
#[derive(Clone)]
pub(crate) struct PreparationAdapter {
	root: EnvironmentRoot,
	binding: GameBinding,
}

// The ports create every plan they consume, so another handle type is a
// composition defect rather than a user-facing condition.
fn foreign_handle() -> Report<ErrorMarker> {
	report!(ErrorMarker::execution_supervision_failed())
}

#[cfg_attr(
	not(windows),
	expect(dead_code, reason = "only Windows exec composes these ports until export uses them")
)]
impl PreparationAdapter {
	/// Uses a binding that the caller has already validated.
	pub fn new(root: EnvironmentRoot, binding: GameBinding) -> Self {
		Self { root, binding }
	}

	pub fn prepare_environment_plan_port(&self) -> PrepareEnvironmentPlan {
		let root = self.root.clone();
		let binding = self.binding.clone();

		Arc::new(move |cancellation| {
			let root = root.clone();
			let binding = binding.clone();

			Box::pin(async move {
				let prepared = EnvironmentAdapter
					.prepare_launch(&root, &binding, &cancellation)
					.await?;

				let providers = prepared
					.providers
					.iter()
					.map(|provider| EnvironmentProvider {
						identity: provider.identity.clone(),
						enabled: provider.enabled,
					})
					.collect();

				Ok(EnvironmentPlan {
					providers,
					state: AdapterState::new(prepared),
				})
			}) as PortFuture<_>
		})
	}

	pub fn project_profile_port(&self) -> ProjectProfile {
		Arc::new(|plan: &EnvironmentPlan| Box::pin(ready(project(plan))) as PortFuture<_>)
	}

	pub fn stage_profile_port(&self) -> StageProfile {
		let root = self.root.clone();

		Arc::new(move |plan: &EnvironmentPlan, purpose, cancellation| {
			let root = root.clone();
			// The port borrows the plan, so the future owns a copy of the prepared launch.
			let prepared = plan.state.downcast_ref::<PreparedLaunch>().cloned();

			Box::pin(async move {
				let prepared = prepared.ok_or_else(foreign_handle)?;

				let inis = EnvironmentAdapter
					.stage_profile_inis(&root, &prepared, purpose, &cancellation)
					.await?;

				Ok(StagedProfile {
					directory: inis.path().to_owned(),
					state: AdapterState::new(inis),
				})
			}) as PortFuture<_>
		})
	}
}

fn project(plan: &EnvironmentPlan) -> Result<ProfileProjection, ErrorMarker> {
	let prepared = plan.state.downcast_ref::<PreparedLaunch>().ok_or_else(foreign_handle)?;

	let files: Vec<_> = prepared
		.profile_files
		.iter()
		.map(|file| ProfileText {
			name: file.name,
			text: &file.text,
		})
		.collect();
	let visible_files: Vec<_> = prepared
		.visible_files
		.iter()
		.map(|file| VisibleProfileFile {
			path: file.path.clone(),
		})
		.collect();

	let projected = build_profile_projection(ProfileProjectionInput {
		files: &files,
		visible_files: &visible_files,
	})
	.context(ErrorMarker::environment_invalid(Some("execution")))?;

	for (order, plugin) in projected.plugins.iter().enumerate() {
		tracing::info!(plugin = %plugin.path, basis = "analytical_data", runtime_observed = false, projected_order = order, projected_activation_sources = ?plugin.activation_sources, "advisory plugin projection");
	}

	Ok(ProfileProjection {
		warnings: projected.warnings,
	})
}
