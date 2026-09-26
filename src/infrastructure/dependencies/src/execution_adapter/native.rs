use super::ExecutionAdapter;
use application::ErrorMarker;
use application::execution::ExecuteProgramOutput;
use application::execution::ExecutionWarning;
use application::ports::ProgressEvent;
use application::ports::ReportProgress;
use domain::OutputTarget;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::ProviderIdentity;
use domain::WorkingDirectory;
use infrastructure_environment::EnvironmentAdapter;
use infrastructure_execution::InheritedStreams;
use infrastructure_execution::LaunchInputError;
use infrastructure_execution::LaunchRequest;
use infrastructure_execution::NativeFailure;
use infrastructure_execution::ProfileConfigurationInput;
use infrastructure_execution::ProfileText;
use infrastructure_execution::ProfileWarning;
use infrastructure_execution::ProviderRoot;
use infrastructure_execution::ViewConfiguration;
use infrastructure_execution::VirtualGameView;
use infrastructure_execution::VisibleProfileFile;
use infrastructure_execution::build_profile_configuration;
use infrastructure_execution::supervise;
use infrastructure_game_platform::GamePlatformAdapter;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use tokio_util::sync::CancellationToken;

impl ExecutionAdapter {
	pub(super) async fn execute(
		&self,
		output_target: OutputTarget,
		working_directory: Option<WorkingDirectory>,
		program: Program,
		arguments: Vec<ProgramArgument>,
		progress: Option<ReportProgress>,
		cancellation: CancellationToken,
	) -> Result<ExecuteProgramOutput, ErrorMarker> {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let arguments: Vec<_> = arguments
			.iter()
			.map(|argument| argument.as_os_str().to_owned())
			.collect();
		let launch = self
			.caller
			.resolve(
				program.as_os_str(),
				&arguments,
				working_directory.as_ref().map(WorkingDirectory::as_path),
			)
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

		let effective_binding = self.settings.load_execution_binding(&cancellation)?;
		let platform = GamePlatformAdapter::system();
		let validate_game = platform.validate_effective_port(self.root.clone());
		let binding = validate_game.call((effective_binding, cancellation.clone())).await?;
		let environment = EnvironmentAdapter;
		let prepared = environment.prepare_execution(&self.root, &binding, &cancellation)?;
		let selected_output_mod = if let OutputTarget::DataMod(name) = output_target {
			let provider = prepared
				.providers
				.iter()
				.find(
					|provider| matches!(&provider.identity, ProviderIdentity::DataMod { mod_name, .. } if *mod_name == name),
				)
				.ok_or_else(|| {
					report!(ErrorMarker::output_target_not_found().with_mod_name(name.clone()))
				})?;
			if !provider.enabled {
				return Err(report!(
					ErrorMarker::output_target_disabled().with_mod_name(name.clone())
				));
			}
			Some(name)
		} else {
			None
		};

		let (documents, local) = platform.execution_profile_directories()?;
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
				modified: file.modified,
			})
			.collect();
		let profile = build_profile_configuration(ProfileConfigurationInput {
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

		let mut warnings: Vec<_> = profile
			.warnings
			.into_iter()
			.map(|warning| match warning {
				ProfileWarning::LoadOrderNotEnforced => ExecutionWarning::LoadOrderNotEnforced,
				ProfileWarning::Unavailable { file, plugin }
					if file.eq_ignore_ascii_case("plugins.txt") =>
				{
					ExecutionWarning::StalePluginEntry { name: plugin }
				}
				ProfileWarning::Unavailable { plugin, .. } => {
					ExecutionWarning::StaleLoadOrderEntry { name: plugin }
				}
				ProfileWarning::Duplicate { file, plugin } => {
					ExecutionWarning::DuplicatePluginEntry { file, name: plugin }
				}
				ProfileWarning::Unlisted { plugin } => {
					ExecutionWarning::UnlistedPlugin { name: plugin }
				}
			})
			.collect();
		let mut mappings = profile.profile_files;
		mappings.push(profile.invalidation_mapping);
		let configuration = ViewConfiguration::new(
			prepared.data_directory.clone(),
			prepared.providers
				.iter()
				.map(|provider| ProviderRoot {
					identity: provider.identity.clone(),
					root: provider.root.clone(),
					enabled: provider.enabled,
				})
				.collect(),
			prepared.winners.clone(),
			selected_output_mod,
			profile.profile_directories,
			mappings,
			profile.saves,
		)
		.context(ErrorMarker::vfs_failed().with_phase("vfs_setup"))?;

		let effective_binding = self.settings.load_execution_binding(&cancellation)?;
		let current_binding = validate_game.call((effective_binding, cancellation.clone())).await?;
		if current_binding != binding {
			return Err(report!(ErrorMarker::environment_invalid(Some("execution"))));
		}

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let view = VirtualGameView::configure(&configuration)
			.context(ErrorMarker::vfs_failed().with_phase("vfs_setup"))?;
		if let Err(mut failure) = environment.revalidate_execution(&self.root, &prepared, &cancellation) {
			if let Err(cleanup) = view.close() {
				failure.children_mut().push(cleanup.into_dynamic().into_cloneable());
			}
			return Err(failure);
		}

		if cancellation.is_cancelled() {
			view.close().context(ErrorMarker::vfs_failed().with_phase("cleanup"))?;
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

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

		let mut process = launched?;

		if let Some(progress) = &progress {
			progress.call((ProgressEvent::ExecutionPrepared,)).await;
		}

		let outcome = supervise(&mut process, cancellation, self.force_cancellation.clone())
			.await
			.context(ErrorMarker::execution_supervision_failed().with_phase("running"));
		drop(process);
		if let Some(streams) = private_streams {
			streams.finish()?;
		}

		let outcome = outcome?;

		if environment
			.check_execution_with_spool(
				&self.root,
				&binding,
				self.capture.as_ref().and_then(|capture| capture.directory()),
				&CancellationToken::new(),
			)
			.is_err()
		{
			warnings.push(ExecutionWarning::ProfileStateInvalid);
		}
		if outcome.forced {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("cleanup")));
		}

		Ok(ExecuteProgramOutput {
			status: ProcessStatus::new(outcome.status),
			warnings,
		})
	}
}
