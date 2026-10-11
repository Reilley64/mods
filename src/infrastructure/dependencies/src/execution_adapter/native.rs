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
use application::ports::ReportProgress;
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

const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_PATH_NOT_FOUND: u32 = 3;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_BAD_EXE_FORMAT: u32 = 193;
const ERROR_EXE_MACHINE_TYPE_MISMATCH: u32 = 216;
const ERROR_DIRECTORY: u32 = 267;
const ERROR_ELEVATION_REQUIRED: u32 = 740;
const LAUNCH_INPUT_ERRORS: [u32; 6] = [
	ERROR_FILE_NOT_FOUND,
	ERROR_PATH_NOT_FOUND,
	ERROR_ACCESS_DENIED,
	ERROR_BAD_EXE_FORMAT,
	ERROR_EXE_MACHINE_TYPE_MISMATCH,
	ERROR_DIRECTORY,
];

struct NativeProgram {
	process: HookedProcess,
	private_streams: Option<PrivateStreams>,
}

/// Reports a foreign adapter handle as a composition defect, not a user-facing condition.
///
/// These ports create every handle they consume, so no user input can supply another handle type.
fn foreign_handle() -> Report<ErrorMarker> {
	report!(ErrorMarker::execution_supervision_failed())
}

fn completed<T: Send + 'static>(result: Result<T, ErrorMarker>) -> PortFuture<T> {
	Box::pin(ready(result))
}

impl ExecutionAdapter {
	/// Builds the Windows exec ports. The caller runs the whole use case on one
	/// thread, so native session calls never overlap.
	pub(super) fn dependencies(&self, report_progress: Option<ReportProgress>) -> ExecuteProgramDependencies {
		let adapter = Arc::new(self.clone());
		let preparation = PreparationPorts::new(self.root.clone(), self.binding.clone());

		ExecuteProgramDependencies {
			report_progress,
			resolve_launch_target: self.resolve_launch_target_port(),
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
				Box::pin(async move {
					let spool = adapter.capture.as_ref().and_then(|capture| capture.directory());
					EnvironmentAdapter
						.check_launch_with_spool(
							&adapter.root,
							&adapter.binding,
							spool,
							&cancellation,
						)
						.await
				}) as PortFuture<_>
			}),
		}
	}

	pub(super) fn resolve_target(
		&self,
		program: Program,
		arguments: Vec<ProgramArgument>,
		working_directory: WorkingDirectory,
		cancellation: &CancellationToken,
	) -> Result<LaunchTarget, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

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

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		Ok(LaunchTarget {
			program: launch.application.clone(),
			working_directory: launch.directory.clone(),
			state: AdapterState::new(launch),
		})
	}

	fn start_program(
		&self,
		file_system: VirtualFileSystem,
		target: LaunchTarget,
	) -> Result<RunningProgram, ErrorMarker> {
		let view: VirtualGameView = file_system.0.downcast().ok_or_else(foreign_handle)?;
		let launch: ResolvedLaunch = target.state.downcast().ok_or_else(foreign_handle)?;

		let inherited_streams = if self.capture.is_none() {
			Some(InheritedStreams::capture().context(ErrorMarker::program_launch_failed())?)
		} else {
			None
		};

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
					} else if native.native_error == ERROR_ELEVATION_REQUIRED {
						ErrorMarker::elevation_required()
					} else if LAUNCH_INPUT_ERRORS.contains(&native.native_error) {
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
