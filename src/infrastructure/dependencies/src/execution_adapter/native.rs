use super::ExecutionAdapter;
use application::ErrorMarker;
use application::execution::ExecuteProgramOutput;
use application::execution::ExecutionWarning;
use domain::OutputTarget;
use domain::ProcessStatus;
use domain::Program;
use domain::ProgramArgument;
use domain::ProviderIdentity;
use domain::WorkingDirectory;
use environment::EnvironmentAdapter;
use execution::InheritedStreams;
use execution::LaunchInputError;
use execution::LaunchRequest;
use execution::NativeFailure;
use execution::ProfileConfigurationInput;
use execution::ProfileText;
use execution::ProfileWarning;
use execution::ProviderRoot;
use execution::ViewConfiguration;
use execution::VirtualGameView;
use execution::VisibleProfileFile;
use execution::build_profile_configuration;
use execution::supervise;
use game_platform::GamePlatformAdapter;
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
		let streams = InheritedStreams::capture().context(ErrorMarker::program_launch_failed())?;

		let effective_binding = self.settings.load_execution_binding(&cancellation)?;
		let platform = GamePlatformAdapter::system();
		let validate_game = platform.validate_effective_port(self.root.clone());
		let binding = validate_game.call((effective_binding, cancellation.clone())).await?;
		let environment = EnvironmentAdapter;
		let prepared = environment.prepare_execution(&self.root, &binding, &cancellation)?;
		let selected = if let OutputTarget::DataMod(name) = output_target {
			let provider = prepared
				.providers
				.iter()
				.find(
					|provider| matches!(&provider.identity, ProviderIdentity::DataMod { mod_name, .. } if *mod_name == name),
				)
				.ok_or_else(|| report!(ErrorMarker::output_target_not_found()))?;
			if !provider.enabled {
				return Err(report!(ErrorMarker::output_target_disabled()));
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
			tracing::info!(plugin = %plugin.path, order, activation_sources = ?plugin.activation_sources, "effective plugin configuration");
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
			selected,
			profile.profile_directories,
			mappings,
			profile.saves,
		)
		.context(ErrorMarker::vfs_failed())?;

		let effective_binding = self.settings.load_execution_binding(&cancellation)?;
		let current_binding = validate_game.call((effective_binding, cancellation.clone())).await?;
		if current_binding != binding {
			return Err(report!(ErrorMarker::environment_invalid(Some("execution"))));
		}

		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let view = VirtualGameView::configure(&configuration).context(ErrorMarker::vfs_failed())?;
		if let Err(mut failure) = environment.revalidate_execution(&self.root, &prepared, &cancellation) {
			if let Err(cleanup) = view.close() {
				failure.children_mut().push(cleanup.into_dynamic().into_cloneable());
			}
			return Err(failure);
		}

		if cancellation.is_cancelled() {
			view.close().context(ErrorMarker::vfs_failed())?;
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let mut process = view
			.launch(LaunchRequest {
				application: &launch.application,
				command_line: &launch.command_line,
				directory: &launch.directory,
				standard_streams: Some(streams.borrowed()),
			})
			.map_err(|error| {
				let native = error
					.iter_reports()
					.find_map(|report| report.downcast_current_context::<NativeFailure>());
				let marker = if let Some(native) = native {
					if native.cleanup_status != 0 {
						ErrorMarker::vfs_failed()
					} else if native.native_error == 740 {
						ErrorMarker::elevation_required()
					} else if matches!(native.native_error, 2 | 3 | 5 | 193 | 216 | 267) {
						ErrorMarker::program_launch_failed()
					} else {
						ErrorMarker::vfs_failed()
					}
				} else {
					ErrorMarker::execution_supervision_failed()
				};
				error.context(marker)
			})?;
		let outcome = supervise(&mut process, cancellation, self.force_cancellation.clone())
			.await
			.context(ErrorMarker::execution_supervision_failed())?;

		if environment
			.check_execution(&self.root, &binding, &CancellationToken::new())
			.is_err()
		{
			warnings.push(ExecutionWarning::ProfileStateInvalid);
		}
		if outcome.forced {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		Ok(ExecuteProgramOutput {
			status: ProcessStatus::new(outcome.status),
			warnings,
		})
	}
}
