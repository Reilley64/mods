use super::ExecutionAdapter;
use application::ErrorMarker;
use application::execution::ExecuteProgramDependencies;
use application::ports::AdapterState;
use application::ports::ExecutionProfile;
use application::ports::ExecutionProfileProjection;
use application::ports::LaunchPlan;
use application::ports::LaunchProvider;
use application::ports::LaunchTarget;
use application::ports::PortFuture;
use application::ports::ProgramExit;
use application::ports::ProgramOutput;
use application::ports::ProgramSupervision;
use application::ports::RunningProgram;
use application::ports::StagedExecutionProfile;
use application::ports::VirtualFileSystem;
use domain::ModName;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use infrastructure_environment::EnvironmentAdapter;
use infrastructure_environment::ExecutionInis;
use infrastructure_environment::PreparedLaunch;
use infrastructure_execution::HookedProcess;
use infrastructure_execution::InheritedStreams;
use infrastructure_execution::LaunchInputError;
use infrastructure_execution::LaunchRequest;
use infrastructure_execution::NativeFailure;
use infrastructure_execution::PrivateStreams;
use infrastructure_execution::ProfileConfiguration;
use infrastructure_execution::ProfileConfigurationInput;
use infrastructure_execution::ProfileText;
use infrastructure_execution::ProviderRoot;
use infrastructure_execution::ResolvedLaunch;
use infrastructure_execution::ViewConfiguration;
use infrastructure_execution::VirtualGameView;
use infrastructure_execution::VisibleProfileFile;
use infrastructure_execution::build_profile_configuration;
use infrastructure_execution::supervise;
use infrastructure_game_platform::GamePlatformAdapter;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::future::ready;
use std::mem::take;
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

		ExecuteProgramDependencies {
			report_progress: None,
			resolve_launch_target: Arc::new({
				let adapter = adapter.clone();
				move |program, arguments, working_directory| {
					completed(adapter.resolve_launch_target(program, arguments, working_directory))
				}
			}),
			prepare_launch_plan: Arc::new({
				let adapter = adapter.clone();
				move |cancellation| {
					let adapter = adapter.clone();
					Box::pin(async move { adapter.prepare_launch_plan(cancellation).await })
						as PortFuture<_>
				}
			}),
			project_execution_profile: Arc::new(|plan: &LaunchPlan| {
				completed(project_execution_profile(plan))
			}),
			stage_execution_profile: Arc::new({
				let adapter = adapter.clone();
				move |plan: &LaunchPlan, cancellation| {
					let adapter = adapter.clone();
					// The port borrows the plan, so the future owns a copy of the prepared launch.
					let prepared = plan.state.downcast_ref::<PreparedLaunch>().cloned();
					Box::pin(async move {
						let prepared = prepared.ok_or_else(foreign_handle)?;
						adapter.stage_execution_profile(&prepared, cancellation).await
					}) as PortFuture<_>
				}
			}),
			create_virtual_file_system: Arc::new(
				|plan: LaunchPlan,
				 profile: ExecutionProfile,
				 staged: &StagedExecutionProfile,
				 output_mod: Option<ModName>,
				 cancellation: CancellationToken| {
					completed(create_virtual_file_system(
						plan,
						profile,
						staged,
						output_mod,
						cancellation,
					))
				},
			),
			launch_program: Arc::new({
				let adapter = adapter.clone();
				move |file_system, target| completed(adapter.launch_program(file_system, target))
			}),
			supervise_program: Arc::new({
				let adapter = adapter.clone();
				move |program, cancellation| {
					let force_cancellation = adapter.force_cancellation.clone();
					Box::pin(supervise_program(program, cancellation, force_cancellation))
						as PortFuture<_>
				}
			}),
			preserve_execution_profile: Arc::new(|staged| {
				Box::pin(preserve_execution_profile(staged)) as PortFuture<_>
			}),
			finish_program_output: Arc::new(|output| completed(finish_program_output(output))),
			check_profile_state: Arc::new(move |cancellation| {
				let adapter = adapter.clone();
				Box::pin(async move { adapter.check_profile_state(cancellation).await })
					as PortFuture<_>
			}),
		}
	}

	fn resolve_launch_target(
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

	async fn prepare_launch_plan(&self, cancellation: CancellationToken) -> Result<LaunchPlan, ErrorMarker> {
		let prepared = EnvironmentAdapter
			.prepare_launch(&self.root, &self.binding, &cancellation)
			.await?;

		let providers = prepared
			.providers
			.iter()
			.map(|provider| LaunchProvider {
				identity: provider.identity.clone(),
				enabled: provider.enabled,
			})
			.collect();

		Ok(LaunchPlan {
			providers,
			state: AdapterState::new(prepared),
		})
	}

	async fn stage_execution_profile(
		&self,
		prepared: &PreparedLaunch,
		cancellation: CancellationToken,
	) -> Result<StagedExecutionProfile, ErrorMarker> {
		let inis = EnvironmentAdapter
			.derive_execution_inis(&self.root, prepared, &cancellation)
			.await?;

		Ok(StagedExecutionProfile {
			directory: inis.path().to_owned(),
			state: AdapterState::new(inis),
		})
	}

	fn launch_program(
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

	async fn check_profile_state(&self, cancellation: CancellationToken) -> Result<(), ErrorMarker> {
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

fn project_execution_profile(plan: &LaunchPlan) -> Result<ExecutionProfileProjection, ErrorMarker> {
	let prepared = plan.state.downcast_ref::<PreparedLaunch>().ok_or_else(foreign_handle)?;

	let (documents, local) = GamePlatformAdapter::system().execution_profile_directories()?;

	let profile_files: Vec<_> = prepared
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
	let mut profile = build_profile_configuration(ProfileConfigurationInput {
		files: &profile_files,
		visible_files: &visible_files,
		profile_directory: &prepared.profile_directory,
		documents_directory: &documents,
		local_app_data_directory: &local,
		data_directory: &prepared.data_directory,
		cache_directory: &prepared.cache_directory,
	})
	.context(ErrorMarker::environment_invalid(Some("execution")))?;

	for (order, plugin) in profile.plugins.iter().enumerate() {
		tracing::info!(plugin = %plugin.path, basis = "analytical_data", runtime_observed = false, projected_order = order, projected_activation_sources = ?plugin.activation_sources, "advisory plugin projection");
	}

	let warnings = take(&mut profile.warnings);

	Ok(ExecutionProfileProjection {
		profile: ExecutionProfile(AdapterState::new(profile)),
		warnings,
	})
}

fn create_virtual_file_system(
	plan: LaunchPlan,
	profile: ExecutionProfile,
	staged: &StagedExecutionProfile,
	output_mod: Option<ModName>,
	cancellation: CancellationToken,
) -> Result<VirtualFileSystem, ErrorMarker> {
	let prepared: PreparedLaunch = plan.state.downcast().ok_or_else(foreign_handle)?;
	let profile: ProfileConfiguration = profile.0.downcast().ok_or_else(foreign_handle)?;

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

async fn supervise_program(
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

async fn preserve_execution_profile(staged: StagedExecutionProfile) -> Result<(), ErrorMarker> {
	let inis: ExecutionInis = staged.state.downcast().ok_or_else(foreign_handle)?;

	inis.preserve().await
}

fn finish_program_output(output: ProgramOutput) -> Result<(), ErrorMarker> {
	let private_streams: Option<PrivateStreams> = output.0.downcast().ok_or_else(foreign_handle)?;

	if let Some(private_streams) = private_streams {
		private_streams.finish()?;
	}

	Ok(())
}
