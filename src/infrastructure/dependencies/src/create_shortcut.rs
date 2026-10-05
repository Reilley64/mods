use crate::Resources;
#[cfg(windows)]
use application::ErrorMarker;
#[cfg(windows)]
use application::settings::SettingKey;
#[cfg(windows)]
use application::settings::SettingValue;
use application::shortcut::CreateShortcutDependencies;
use application::shortcut::ShortcutFailure;
#[cfg(windows)]
use application::shortcut::ValidatedShortcutLaunch;
#[cfg(windows)]
use domain::OutputTarget;
#[cfg(windows)]
use domain::Program;
#[cfg(windows)]
use domain::ProgramArgument;
#[cfg(windows)]
use domain::ProviderIdentity;
#[cfg(windows)]
use domain::WorkingDirectory;
#[cfg(windows)]
use infrastructure_execution::CallerSnapshot;
#[cfg(windows)]
use infrastructure_execution::LaunchInputError;
#[cfg(windows)]
use infrastructure_execution::persist_shortcut;
#[cfg(windows)]
use rootcause::Result;
#[cfg(windows)]
use rootcause::prelude::ResultExt;
use rootcause::report;
#[cfg(windows)]
use std::env::current_exe;
#[cfg(windows)]
use std::fs::canonicalize;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(windows)]
use tokio::task::spawn_blocking;
#[cfg(windows)]
use tokio_util::sync::CancellationToken;

impl Resources {
	pub fn create_shortcut_dependencies(&self, startup_directory: PathBuf) -> CreateShortcutDependencies {
		#[cfg(not(windows))]
		{
			let _ = startup_directory;
			CreateShortcutDependencies {
				validate_launch: Arc::new(|_, _, _, _, _| {
					Box::pin(async { Err(report!(ShortcutFailure::Unsupported)) })
				}),
				persist: Arc::new(|_| Box::pin(async { Err(report!(ShortcutFailure::Unsupported)) })),
			}
		}
		#[cfg(windows)]
		{
			let resources = self.clone();
			let caller = CallerSnapshot::new(startup_directory);
			CreateShortcutDependencies {
				validate_launch: Arc::new(
					move |output_target, working_directory, program, arguments, cancellation| {
						let resources = resources.clone();
						let caller = caller.clone();
						Box::pin(async move {
							resources
								.validate_shortcut_launch(
									caller,
									output_target,
									working_directory,
									program,
									arguments,
									cancellation,
								)
								.await
						})
					},
				),
				persist: Arc::new(|definition| {
					Box::pin(async move {
						spawn_blocking(move || persist_shortcut(definition))
							.await
							.context(ShortcutFailure::Publication)?
					})
				}),
			}
		}
	}

	#[cfg(windows)]
	async fn validate_shortcut_launch(
		&self,
		caller: CallerSnapshot,
		output_target: OutputTarget,
		working_directory: Option<WorkingDirectory>,
		program: Program,
		arguments: Vec<ProgramArgument>,
		cancellation: CancellationToken,
	) -> Result<ValidatedShortcutLaunch, ShortcutFailure> {
		let arguments: Vec<_> = arguments
			.iter()
			.map(|argument| argument.as_os_str().to_owned())
			.collect();
		let launch = caller
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
				error.context(marker).context(ShortcutFailure::InvalidLaunch)
			})?;

		let effective_binding = self
			.settings
			.load_execution_binding(&cancellation)
			.context(ShortcutFailure::InvalidLaunch)?;
		let binding = self
			.game_platform
			.validate_effective_port(self.root.clone())
			.call((effective_binding, cancellation.clone()))
			.await
			.context(ShortcutFailure::InvalidLaunch)?;
		let prepared = self
			.environment
			.prepare_execution(&self.root, &binding, &cancellation)
			.context(ShortcutFailure::InvalidLaunch)?;
		if let OutputTarget::DataMod(name) = output_target {
			let provider = prepared
				.providers
				.iter()
				.find(
					|provider| matches!(&provider.identity, ProviderIdentity::DataMod { mod_name, .. } if *mod_name == name),
				)
				.ok_or_else(|| {
					report!(ErrorMarker::output_target_not_found().with_mod_name(name.clone()))
						.context(ShortcutFailure::InvalidLaunch)
				})?;
			if !provider.enabled {
				return Err(report!(ErrorMarker::output_target_disabled().with_mod_name(name))
					.context(ShortcutFailure::InvalidLaunch));
			}
		}

		let settings = self
			.settings
			.load_port()
			.call(())
			.await
			.context(ShortcutFailure::InvalidLaunch)?;
		let environment_name = settings.settings.into_iter().find_map(|record| {
			if record.key == SettingKey::Name
				&& let SettingValue::String(name) = record.manifest_value
			{
				return Some(name);
			}
			None
		});

		Ok(ValidatedShortcutLaunch {
			launcher: canonicalize(current_exe().context(ShortcutFailure::InvalidLaunch)?)
				.context(ShortcutFailure::InvalidLaunch)?,
			environment: canonicalize(self.root.as_path()).context(ShortcutFailure::InvalidLaunch)?,
			program: launch.application,
			working_directory: launch.directory,
			environment_name,
		})
	}
}
