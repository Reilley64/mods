use super::ExecutionAdapter;
use crate::environment_preparation::PreparationPorts;
use application::ErrorMarker;
use application::execution::ExecuteProgramDependencies;
use application::ports::AdapterState;
use application::ports::EnvironmentPlan;
use application::ports::LaunchTarget;
use application::ports::PortFuture;
use application::ports::ProgramExit;
use application::ports::ProgramOutput;
use application::ports::ProgramSupervision;
use application::ports::RunningProgram;
use application::ports::StagedProfile;
use application::ports::VirtualFileSystem;
use domain::ModName;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use infrastructure_environment::EnvironmentAdapter;
use infrastructure_environment::PreparedLaunch;
use infrastructure_environment::StagedProfileInis;
use infrastructure_execution::HookedProcess;
use infrastructure_execution::InheritedStreams;
use infrastructure_execution::LaunchInputError;
use infrastructure_execution::LaunchRequest;
use infrastructure_execution::NativeFailure;
use infrastructure_execution::PrivateStreams;
use infrastructure_execution::ProfileMappingInput;
use infrastructure_execution::ProviderRoot;
use infrastructure_execution::ResolvedLaunch;
use infrastructure_execution::ViewConfiguration;
use infrastructure_execution::VirtualGameView;
use infrastructure_execution::profile_mappings;
use infrastructure_execution::supervise;
use infrastructure_game_platform::GamePlatformAdapter;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::future::ready;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Caller-resolved launch inputs and the inherited streams chosen before preparation.
struct NativeLaunchTarget {
	launch: ResolvedLaunch,
	inherited_streams: Option<InheritedStreams>,
}

struct NativeProgram {
	process: HookedProcess,
	private_streams: Option<PrivateStreams>,
}

// These ports create every handle they consume, so another handle type is a
// composition defect rather than a user-facing condition.
fn foreign_handle() -> Report<ErrorMarker> {
	report!(ErrorMarker::execution_supervision_failed())
}

fn completed<T: Send + 'static>(result: Result<T, ErrorMarker>) -> PortFuture<T> {
	Box::pin(ready(result))
}

impl ExecutionAdapter {
	/// Builds the Windows exec ports. The caller runs the whole use case on one
	/// thread, so native session calls never overlap.
	pub(super) fn dependencies(&self) -> ExecuteProgramDependencies {
		let adapter = Arc::new(self.clone());
		let preparation = PreparationPorts::new(self.root.clone(), self.binding.clone());

		ExecuteProgramDependencies {
			report_progress: None,
			resolve_launch_target: Arc::new({
				let adapter = adapter.clone();
				move |program, arguments, working_directory| {
					completed(adapter.resolve_target(program, arguments, working_directory))
				}
			}),
			prepare_environment_plan: preparation.prepare_environment_plan,
			project_profile: preparation.project_profile,
			set_load_order_times: preparation.set_load_order_times,
			stage_profile: preparation.stage_profile,
			create_virtual_file_system: Arc::new(
				|plan: EnvironmentPlan,
				 staged: &StagedProfile,
				 output_mod: Option<ModName>,
				 cancellation: CancellationToken| {
					completed(build_virtual_file_system(plan, staged, output_mod, cancellation))
				},
			),
			launch_program: Arc::new({
				let adapter = adapter.clone();
				move |file_system, target| completed(adapter.start_program(file_system, target))
			}),
			supervise_program: Arc::new({
				let adapter = adapter.clone();
				move |program, cancellation| {
					let force_cancellation = adapter.force_cancellation.clone();
					Box::pin(supervise_child(program, cancellation, force_cancellation))
						as PortFuture<_>
				}
			}),
			preserve_execution_profile: Arc::new(|staged| Box::pin(preserve_inis(staged)) as PortFuture<_>),
			finish_program_output: Arc::new(|output| completed(finish_streams(output))),
			check_profile_state: Arc::new(move |cancellation| {
				let adapter = adapter.clone();
				Box::pin(async move { adapter.check_retained_state(cancellation).await })
					as PortFuture<_>
			}),
		}
	}

	fn resolve_target(
		&self,
		program: Program,
		arguments: Vec<ProgramArgument>,
		working_directory: WorkingDirectory,
	) -> Result<LaunchTarget, ErrorMarker> {
		let arguments: Vec<_> = arguments
			.iter()
			.map(|argument| argument.as_os_str().to_owned())
			.collect();
		let launch = self
			.caller
			.resolve(program.as_os_str(), &arguments, Some(working_directory.as_path()))
			.map_err(|error| {
				let marker = match error.current_context() {
					LaunchInputError::NotFound => ErrorMarker::program_not_found(),
					LaunchInputError::InvalidDirectory => ErrorMarker::invalid_working_directory(),
					LaunchInputError::InvalidTarget => ErrorMarker::program_unsupported(),
					_ => ErrorMarker::program_launch_failed(),
				};
				error.context(marker)
			})?;

		let inherited_streams = if self.capture.is_none() {
			Some(InheritedStreams::capture().context(ErrorMarker::program_launch_failed())?)
		} else {
			None
		};

		Ok(LaunchTarget(AdapterState::new(NativeLaunchTarget {
			launch,
			inherited_streams,
		})))
	}

	fn start_program(
		&self,
		file_system: VirtualFileSystem,
		target: LaunchTarget,
	) -> Result<RunningProgram, ErrorMarker> {
		let view: VirtualGameView = file_system.0.downcast().ok_or_else(foreign_handle)?;
		let NativeLaunchTarget {
			launch,
			inherited_streams,
		} = target.0.downcast().ok_or_else(foreign_handle)?;

		let mut private_streams = self
			.capture
			.as_ref()
			.map(|capture| capture.prepare(self.force_cancellation.clone()))
			.transpose()?;

		let launched = view
			.launch(LaunchRequest {
				new_process_group: self.capture.is_some(),
				application: &launch.application,
				command_line: &launch.command_line,
				directory: &launch.directory,
				standard_streams: private_streams
					.as_ref()
					.and_then(|streams| streams.borrowed())
					.or_else(|| inherited_streams.as_ref().map(InheritedStreams::borrowed)),
			})
			.map_err(|error| {
				let native = error
					.iter_reports()
					.find_map(|report| report.downcast_current_context::<NativeFailure>());
				let marker = if let Some(native) = native {
					if native.cleanup_status != 0 {
						ErrorMarker::vfs_failed().with_phase("cleanup")
					} else if native.native_error == 740 {
						ErrorMarker::elevation_required()
					} else if matches!(native.native_error, 2 | 3 | 5 | 193 | 216 | 267) {
						ErrorMarker::program_launch_failed()
					} else {
						ErrorMarker::vfs_failed().with_phase("vfs_setup")
					}
				} else {
					ErrorMarker::execution_supervision_failed().with_phase("launch")
				};
				error.context(marker)
			});
		if let Some(streams) = &mut private_streams {
			streams.close_child_ends();
		}

		// Process creation gave the child its own copies of the inherited streams,
		// so this function's duplicates close when it returns.
		let process = launched?;

		Ok(RunningProgram(AdapterState::new(NativeProgram {
			process,
			private_streams,
		})))
	}

	async fn check_retained_state(&self, cancellation: CancellationToken) -> Result<(), ErrorMarker> {
		EnvironmentAdapter
			.check_launch_with_spool(
				&self.root,
				&self.binding,
				self.capture.as_ref().and_then(|capture| capture.directory()),
				&cancellation,
			)
			.await
	}
}

fn build_virtual_file_system(
	plan: EnvironmentPlan,
	staged: &StagedProfile,
	output_mod: Option<ModName>,
	cancellation: CancellationToken,
) -> Result<VirtualFileSystem, ErrorMarker> {
	let prepared: PreparedLaunch = plan.state.downcast().ok_or_else(foreign_handle)?;

	let (documents, local) = GamePlatformAdapter::system().execution_profile_directories()?;

	let profile = profile_mappings(ProfileMappingInput {
		profile_directory: &prepared.profile_directory,
		documents_directory: &documents,
		local_app_data_directory: &local,
		data_directory: &prepared.data_directory,
		cache_directory: &prepared.cache_directory,
	});

	let mut mappings = profile.profile_files;
	for mapping in &mut mappings {
		if let Some(name) = mapping.source.file_name()
			&& name.to_str().is_some_and(|name| name.ends_with(".ini"))
		{
			mapping.source = staged.directory.join(name);
		}
	}
	mappings.push(profile.invalidation_mapping);

	let configuration = ViewConfiguration::new(
		prepared.data_directory,
		prepared.providers
			.into_iter()
			.map(|provider| ProviderRoot {
				identity: provider.identity,
				root: provider.root,
				enabled: provider.enabled,
			})
			.collect(),
		prepared.winners,
		output_mod,
		profile.profile_directories,
		mappings,
		profile.saves,
	)
	.context(ErrorMarker::vfs_failed().with_phase("vfs_setup"))?;

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let view = VirtualGameView::configure(&configuration)
		.context(ErrorMarker::vfs_failed().with_phase("vfs_setup"))?;

	Ok(VirtualFileSystem(AdapterState::new(view)))
}

async fn supervise_child(
	program: RunningProgram,
	cancellation: CancellationToken,
	force_cancellation: CancellationToken,
) -> Result<ProgramSupervision, ErrorMarker> {
	let NativeProgram {
		mut process,
		private_streams,
	} = program.0.downcast().ok_or_else(foreign_handle)?;

	let exit = supervise(&mut process, cancellation, force_cancellation)
		.await
		.context(ErrorMarker::execution_supervision_failed().with_phase("running"))
		.map(|exit| ProgramExit {
			status: ProcessStatus::new(exit.status),
			forced: exit.forced,
		});

	let job_drained = process.job_is_empty().map_err(|error| error.into_dynamic());
	drop(process);

	Ok(ProgramSupervision {
		exit,
		job_drained,
		output: ProgramOutput(AdapterState::new(private_streams)),
	})
}

async fn preserve_inis(staged: StagedProfile) -> Result<(), ErrorMarker> {
	let inis: StagedProfileInis = staged.state.downcast().ok_or_else(foreign_handle)?;

	inis.preserve().await
}

fn finish_streams(output: ProgramOutput) -> Result<(), ErrorMarker> {
	let private_streams: Option<PrivateStreams> = output.0.downcast().ok_or_else(foreign_handle)?;

	if let Some(private_streams) = private_streams {
		private_streams.finish()?;
	}

	Ok(())
}
