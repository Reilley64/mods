mod name;
use super::LocateEnvironmentRoot;
use super::LocateLauncher;
use super::PersistShortcut;
use super::ShortcutDefinition;
use crate::ports::LoadSettings;
use crate::ports::PrepareExecutionEnvironment;
use crate::ports::ResolveLaunchInputs;
use crate::ports::ResolvedLaunch;
use crate::settings::SettingKey;
use crate::settings::SettingValue;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use name::shortcut_name;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct CreateShortcutDependencies {
	pub locate_launcher: LocateLauncher,
	pub resolve_launch_inputs: ResolveLaunchInputs,
	pub prepare_execution_environment: PrepareExecutionEnvironment,
	pub load_settings: LoadSettings,
	pub locate_environment_root: LocateEnvironmentRoot,
	pub persist: PersistShortcut,
}

#[derive(Debug)]
pub struct CreateShortcutOutput;

#[derive(Debug)]
pub struct CreateShortcutError;
impl fmt::Display for CreateShortcutError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to create launch shortcut")
	}
}

#[expect(
	clippy::too_many_arguments,
	reason = "Keep independent launch and publication choices explicit at the application boundary"
)]
#[tracing::instrument(skip_all)]
pub async fn create_shortcut(
	dependencies: CreateShortcutDependencies,
	output_target: OutputTarget,
	working_directory: Option<WorkingDirectory>,
	program: Program,
	arguments: Vec<ProgramArgument>,
	name: Option<String>,
	destination: Option<PathBuf>,
	log_level: String,
	cancellation: CancellationToken,
) -> Result<CreateShortcutOutput, CreateShortcutError> {
	let explicit_name = name
		.as_deref()
		.map(|value| shortcut_name(value, true))
		.transpose()
		.context(CreateShortcutError)?;

	let launcher = dependencies
		.locate_launcher
		.call(())
		.await
		.context(CreateShortcutError)?;
	let ResolvedLaunch {
		program: resolved_program,
		working_directory: resolved_directory,
		..
	} = dependencies
		.resolve_launch_inputs
		.call((working_directory, program, arguments.clone(), cancellation.clone()))
		.await
		.context(CreateShortcutError)?;
	dependencies
		.prepare_execution_environment
		.call((output_target.clone(), cancellation))
		.await
		.context(CreateShortcutError)?;
	let settings = dependencies.load_settings.call(()).await.context(CreateShortcutError)?;
	let environment = dependencies
		.locate_environment_root
		.call(())
		.await
		.context(CreateShortcutError)?;

	let name = if let Some(name) = explicit_name {
		name
	} else {
		let environment_name = settings
			.settings
			.into_iter()
			.find_map(|record| {
				if record.key == SettingKey::Name
					&& let SettingValue::String(name) = record.manifest_value
				{
					return Some(name);
				}
				None
			})
			.unwrap_or_else(|| {
				environment
					.file_name()
					.unwrap_or_default()
					.to_string_lossy()
					.into_owned()
			});
		let executable_name = resolved_program.file_stem().unwrap_or_default().to_string_lossy();
		shortcut_name(&format!("{environment_name} — {executable_name}"), false).context(CreateShortcutError)?
	};
	let mut saved_arguments = vec![
		OsString::from("--environment"),
		environment.into_os_string(),
		OsString::from("--log-level"),
		log_level.into(),
		OsString::from("exec"),
		OsString::from("--hidden"),
		OsString::from("--cwd"),
		resolved_directory.as_os_str().to_owned(),
	];
	if let OutputTarget::DataMod(name) = output_target {
		// Binding the value with `=` preserves mod names beginning with a hyphen.
		saved_arguments.push(OsString::from(format!("--output-target={}", name.as_str())));
	}
	saved_arguments.extend([OsString::from("--"), resolved_program.as_os_str().to_owned()]);
	saved_arguments.extend(arguments.into_iter().map(|argument| argument.as_os_str().to_owned()));

	dependencies
		.persist
		.call((ShortcutDefinition {
			launcher,
			arguments: saved_arguments,
			working_directory: resolved_directory,
			icon: resolved_program,
			name,
			destination,
		},))
		.await
		.context(CreateShortcutError)?;

	Ok(CreateShortcutOutput)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::ErrorMarker;
	use crate::installation::InstallationState;
	use crate::ports::PortFuture;
	use crate::ports::PreparedExecution;
	use crate::settings::ResolvedSettings;
	use crate::settings::SettingRecord;
	use crate::settings::SettingSource;
	use crate::shortcut::ShortcutFailure;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ModName;
	use domain::SteamBuildId;
	use rootcause::report;
	use std::collections::HashMap;
	use std::env::temp_dir;
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;
	use std::sync::atomic::Ordering;

	fn game_binding() -> Result<GameBinding, CreateShortcutError> {
		Ok(GameBinding::new(
			GameInstallationPath::new(temp_dir().join("game")).context(CreateShortcutError)?,
			SteamBuildId::new(1).context(CreateShortcutError)?,
		))
	}

	fn prepared_execution(binding: GameBinding) -> PreparedExecution {
		PreparedExecution {
			game_binding: binding.clone(),
			providers: Vec::new(),
			winners: Vec::new(),
			visible_files: Vec::new(),
			profile_files: Vec::new(),
			profile_directory: temp_dir().join("profile"),
			data_directory: temp_dir().join("game").join("Data"),
			cache_directory: temp_dir().join("cache"),
			consumed_state: InstallationState {
				game_binding: binding,
				installed_mods: Vec::new(),
				current_winners: HashMap::new(),
				file_dependencies: HashMap::new(),
			},
			consumed_bytes: Vec::new(),
			file_lengths: Vec::new(),
		}
	}

	fn resolved_settings(binding: GameBinding, display: Option<String>) -> ResolvedSettings {
		ResolvedSettings {
			settings: display
				.into_iter()
				.map(|name| SettingRecord {
					key: SettingKey::Name,
					value: SettingValue::String(name.clone()),
					source: SettingSource::Manifest,
					manifest_value: SettingValue::String(name),
					manifest_path: "name",
					shadowed: false,
					writable: true,
				})
				.collect(),
			effective_binding: binding.clone(),
			manifest_binding: binding,
		}
	}

	fn dependencies(display: Option<String>) -> Result<CreateShortcutDependencies, CreateShortcutError> {
		let binding = game_binding()?;
		let prepared = prepared_execution(binding.clone());
		let settings = resolved_settings(binding, display);
		Ok(CreateShortcutDependencies {
			locate_launcher: Arc::new(|| Box::pin(async { Ok(PathBuf::from("/bin/mods.exe")) })),
			resolve_launch_inputs: Arc::new(|_, _, _, _| {
				Box::pin(async {
					Ok(ResolvedLaunch {
						program: "/tools/Tool.exe".into(),
						working_directory: "/caller".into(),
						command_line: "\"/tools/Tool.exe\"".into(),
						target_lease: Arc::new(()),
					})
				}) as PortFuture<_>
			}),
			prepare_execution_environment: Arc::new(move |_, _| {
				let prepared = prepared.clone();
				Box::pin(async move { Ok(prepared) }) as PortFuture<_>
			}),
			load_settings: Arc::new(move || {
				let settings = settings.clone();
				Box::pin(async move { Ok(settings) }) as PortFuture<_>
			}),
			locate_environment_root: Arc::new(|| Box::pin(async { Ok(PathBuf::from("/Environment")) })),
			persist: Arc::new(|_| Box::pin(async { Ok(()) })),
		})
	}

	#[tokio::test]
	async fn invalid_explicit_name_never_validates_or_publishes() -> Result<(), CreateShortcutError> {
		for name in [
			"../tool", "CON", "COM1.txt", "LPT¹", "trail.", "trail ", "", "a:b", "snow?",
		] {
			let called = Arc::new(AtomicBool::new(false));
			let located = called.clone();
			let resolved = called.clone();
			let published = called.clone();
			let mut dependencies = dependencies(None)?;
			dependencies.locate_launcher = Arc::new(move || {
				located.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ShortcutFailure::InvalidLaunch)) })
			});
			dependencies.resolve_launch_inputs = Arc::new(move |_, _, _, _| {
				resolved.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ErrorMarker::program_not_found())) }) as PortFuture<_>
			});
			dependencies.persist = Arc::new(move |_| {
				published.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ShortcutFailure::Publication)) })
			});
			let result = create_shortcut(
				dependencies,
				OutputTarget::Overwrite,
				None,
				Program::new("tool.exe".into()).map_err(|e| e.context(CreateShortcutError))?,
				vec![],
				Some(name.into()),
				None,
				"info".into(),
				CancellationToken::new(),
			)
			.await;
			let error = result.err().ok_or_else(|| report!(CreateShortcutError))?;
			assert!(error
				.iter_reports()
				.any(|cause| cause.downcast_current_context::<ShortcutFailure>()
					== Some(&ShortcutFailure::InvalidName)));
			assert!(!called.load(Ordering::SeqCst));
		}

		Ok(())
	}

	#[tokio::test]
	async fn shortcut_saves_exact_launch_choices_and_sanitizes_only_default_name() -> Result<(), CreateShortcutError>
	{
		let values = ["", " ", "a\"b", "tail\\", "雪", "--"];
		for (display, explicit, expected) in [
			(Some("Vanilla: Plus"), None, "Vanilla_ Plus — Tool"),
			(None, None, "Environment — Tool"),
			(Some("ignored"), Some("My tool"), "My tool"),
		] {
			let expected = expected.to_owned();
			let mut dependencies = dependencies(display.map(str::to_owned))?;
			dependencies.resolve_launch_inputs = Arc::new(move |cwd, program, arguments, _| {
				assert!(cwd.is_none());
				assert_eq!(program.as_os_str(), "Tool.exe");
				assert_eq!(arguments.iter().map(|arg| arg.as_os_str()).collect::<Vec<_>>(), values);
				Box::pin(async {
					Ok(ResolvedLaunch {
						program: "/tools/Tool.exe".into(),
						working_directory: "/caller".into(),
						command_line: "\"/tools/Tool.exe\"".into(),
						target_lease: Arc::new(()),
					})
				}) as PortFuture<_>
			});
			let prepare = dependencies.prepare_execution_environment.clone();
			dependencies.prepare_execution_environment = Arc::new(move |target, cancellation| {
				assert!(matches!(&target, OutputTarget::DataMod(name) if name.as_str() == "Generated"));
				prepare(target, cancellation)
			});
			dependencies.persist = Arc::new(move |definition| {
				assert_eq!(definition.launcher, PathBuf::from("/bin/mods.exe"));
				assert_eq!(definition.icon, PathBuf::from("/tools/Tool.exe"));
				assert_eq!(definition.working_directory, PathBuf::from("/caller"));
				assert_eq!(definition.name, expected);
				assert_eq!(definition.destination, Some(PathBuf::from("/links")));
				assert_eq!(
					definition.arguments,
					[
						"--environment",
						"/Environment",
						"--log-level",
						"debug",
						"exec",
						"--hidden",
						"--cwd",
						"/caller",
						"--output-target=Generated",
						"--",
						"/tools/Tool.exe",
						"",
						" ",
						"a\"b",
						"tail\\",
						"雪",
						"--"
					]
					.map(OsString::from)
				);
				Box::pin(async { Ok(()) })
			});
			create_shortcut(
				dependencies,
				OutputTarget::DataMod(ModName::new("Generated".into()).context(CreateShortcutError)?),
				None,
				Program::new("Tool.exe".into()).context(CreateShortcutError)?,
				values.into_iter()
					.map(|value| ProgramArgument::new(value.into()))
					.collect::<Result<Vec<_>, _>>()
					.context(CreateShortcutError)?,
				explicit.map(str::to_owned),
				Some("/links".into()),
				"debug".into(),
				CancellationToken::new(),
			)
			.await?;
		}
		Ok(())
	}

	#[tokio::test]
	async fn shortcut_validation_failure_is_preserved_and_never_publishes() -> Result<(), CreateShortcutError> {
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		let mut dependencies = dependencies(None)?;
		dependencies.prepare_execution_environment = Arc::new(|_, _| {
			Box::pin(async { Err(report!(ErrorMarker::output_target_disabled())) }) as PortFuture<_>
		});
		dependencies.persist = Arc::new(move |_| {
			observed.store(true, Ordering::SeqCst);
			Box::pin(async { Ok(()) })
		});
		let result = create_shortcut(
			dependencies,
			OutputTarget::Overwrite,
			None,
			Program::new("Tool.exe".into()).context(CreateShortcutError)?,
			vec![],
			None,
			None,
			"info".into(),
			CancellationToken::new(),
		)
		.await;
		let error = result.err().ok_or_else(|| report!(CreateShortcutError))?;
		assert!(error
			.iter_reports()
			.any(|cause| cause.downcast_current_context::<ErrorMarker>()
				== Some(&ErrorMarker::output_target_disabled())));
		assert!(!called.load(Ordering::SeqCst));
		Ok(())
	}

	#[tokio::test]
	async fn unavailable_launcher_stops_before_launch_validation() -> Result<(), CreateShortcutError> {
		let called = Arc::new(AtomicBool::new(false));
		let resolved = called.clone();
		let published = called.clone();
		let mut dependencies = dependencies(None)?;
		dependencies.locate_launcher =
			Arc::new(|| Box::pin(async { Err(report!(ShortcutFailure::Unsupported)) }));
		dependencies.resolve_launch_inputs = Arc::new(move |_, _, _, _| {
			resolved.store(true, Ordering::SeqCst);
			Box::pin(async { Err(report!(ErrorMarker::program_unsupported())) }) as PortFuture<_>
		});
		dependencies.persist = Arc::new(move |_| {
			published.store(true, Ordering::SeqCst);
			Box::pin(async { Ok(()) })
		});
		let result = create_shortcut(
			dependencies,
			OutputTarget::Overwrite,
			None,
			Program::new("Tool.exe".into()).context(CreateShortcutError)?,
			vec![],
			None,
			None,
			"info".into(),
			CancellationToken::new(),
		)
		.await;
		let error = result.err().ok_or_else(|| report!(CreateShortcutError))?;
		assert!(error
			.iter_reports()
			.any(|cause| cause.downcast_current_context::<ShortcutFailure>()
				== Some(&ShortcutFailure::Unsupported)));
		assert!(!error
			.iter_reports()
			.any(|cause| cause.downcast_current_context::<ErrorMarker>().is_some()));
		assert!(!called.load(Ordering::SeqCst));
		Ok(())
	}

	#[tokio::test]
	async fn caller_cancellation_reaches_validation_and_never_publishes() -> Result<(), CreateShortcutError> {
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		let mut dependencies = dependencies(None)?;
		let resolve = dependencies.resolve_launch_inputs.clone();
		dependencies.resolve_launch_inputs = Arc::new(move |cwd, program, arguments, token| {
			assert!(token.is_cancelled());
			resolve(cwd, program, arguments, token)
		});
		dependencies.prepare_execution_environment = Arc::new(|_, token| {
			Box::pin(async move {
				if token.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				Err(report!(ErrorMarker::environment_invalid(None)))
			}) as PortFuture<_>
		});
		dependencies.persist = Arc::new(move |_| {
			observed.store(true, Ordering::SeqCst);
			Box::pin(async { Ok(()) })
		});
		let result = create_shortcut(
			dependencies,
			OutputTarget::Overwrite,
			None,
			Program::new("Tool.exe".into()).context(CreateShortcutError)?,
			vec![],
			None,
			None,
			"info".into(),
			cancellation,
		)
		.await;
		let error = result.err().ok_or_else(|| report!(CreateShortcutError))?;
		assert!(error
			.iter_reports()
			.any(|cause| cause.downcast_current_context::<ErrorMarker>()
				== Some(&ErrorMarker::operation_cancelled())));
		assert!(!called.load(Ordering::SeqCst));
		Ok(())
	}
}
