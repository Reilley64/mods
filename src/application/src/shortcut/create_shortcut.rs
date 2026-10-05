mod name;
use super::LocateEnvironmentRoot;
use super::LocateLauncher;
use super::PersistShortcut;
use super::ShortcutDefinition;
use crate::execution::child_working_directory;
use crate::execution::output_mod;
use crate::ports::LaunchTarget;
use crate::ports::PrepareEnvironmentPlan;
use crate::ports::ProjectProfile;
use crate::ports::ResolveLaunchTarget;
use crate::preparation::PreparedEnvironment;
use crate::preparation::prepare_environment;
use crate::settings::ResolvedSettings;
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
	pub resolve_launch_target: ResolveLaunchTarget,
	pub prepare_environment_plan: PrepareEnvironmentPlan,
	pub project_profile: ProjectProfile,
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
	settings: ResolvedSettings,
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

	let working_directory =
		child_working_directory(working_directory, &settings.effective_binding).context(CreateShortcutError)?;
	let LaunchTarget {
		program: resolved_program,
		working_directory: resolved_directory,
		..
	} = dependencies
		.resolve_launch_target
		.call((program, arguments.clone(), working_directory))
		.await
		.context(CreateShortcutError)?;

	let PreparedEnvironment { plan, .. } = prepare_environment(
		&dependencies.prepare_environment_plan,
		&dependencies.project_profile,
		cancellation,
	)
	.await
	.context(CreateShortcutError)?;
	output_mod(&plan, output_target.clone()).context(CreateShortcutError)?;

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
	use crate::ErrorCode;
	use crate::ErrorMarker;
	use crate::ports::AdapterState;
	use crate::ports::EnvironmentPlan;
	use crate::ports::EnvironmentProvider;
	use crate::ports::PortFuture;
	use crate::ports::ProfileProjection;
	use crate::settings::SettingRecord;
	use crate::settings::SettingSource;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ProviderIdentity;
	use rootcause::report;
	use std::env::temp_dir;
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;
	use std::sync::atomic::Ordering;

	fn game_binding() -> Result<GameBinding, CreateShortcutError> {
		Ok(GameBinding::new(
			GameInstallationPath::new(temp_dir().join("game")).context(CreateShortcutError)?,
		))
	}

	fn settings(display: Option<&str>) -> Result<ResolvedSettings, CreateShortcutError> {
		let binding = game_binding()?;
		Ok(ResolvedSettings {
			settings: display
				.into_iter()
				.map(|name| SettingRecord {
					key: SettingKey::Name,
					value: SettingValue::String(name.to_owned()),
					source: SettingSource::Manifest,
					manifest_value: SettingValue::String(name.to_owned()),
					manifest_path: "name",
					shadowed: false,
					writable: true,
				})
				.collect(),
			effective_binding: binding.clone(),
			manifest_binding: binding,
		})
	}

	fn launch_target(working_directory: PathBuf) -> LaunchTarget {
		LaunchTarget {
			program: "/tools/Tool.exe".into(),
			working_directory,
			state: AdapterState::new(()),
		}
	}

	fn dependencies() -> Result<CreateShortcutDependencies, CreateShortcutError> {
		let generated = ModName::new("Generated".into()).context(CreateShortcutError)?;
		Ok(CreateShortcutDependencies {
			locate_launcher: Arc::new(|| Box::pin(async { Ok(PathBuf::from("/bin/mods.exe")) })),
			resolve_launch_target: Arc::new(|_, _, _| {
				Box::pin(async { Ok(launch_target("/caller".into())) }) as PortFuture<_>
			}),
			prepare_environment_plan: Arc::new(move |_| {
				let generated = generated.clone();
				Box::pin(async move {
					Ok(EnvironmentPlan {
						providers: vec![EnvironmentProvider {
							identity: ProviderIdentity::DataMod {
								mod_name: generated,
								priority: ModPriority::new(0),
							},
							enabled: true,
						}],
						state: AdapterState::new(()),
					})
				}) as PortFuture<_>
			}),
			project_profile: Arc::new(|_: &EnvironmentPlan| {
				Box::pin(async { Ok(ProfileProjection { warnings: Vec::new() }) }) as PortFuture<_>
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
			let mut dependencies = dependencies()?;
			dependencies.locate_launcher = Arc::new(move || {
				located.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ErrorMarker::shortcut_launch_invalid())) })
			});
			dependencies.resolve_launch_target = Arc::new(move |_, _, _| {
				resolved.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ErrorMarker::program_not_found())) }) as PortFuture<_>
			});
			dependencies.persist = Arc::new(move |_| {
				published.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ErrorMarker::shortcut_failed())) })
			});
			let result = create_shortcut(
				dependencies,
				settings(None)?,
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
			assert!(error.iter_reports().any(|cause| cause
				.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::ShortcutNameInvalid)));
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
			let mut dependencies = dependencies()?;
			dependencies.resolve_launch_target = Arc::new(move |program, arguments, cwd| {
				assert_eq!(cwd.as_path(), temp_dir().join("game"));
				assert_eq!(program.as_os_str(), "Tool.exe");
				assert_eq!(arguments.iter().map(|arg| arg.as_os_str()).collect::<Vec<_>>(), values);
				Box::pin(async { Ok(launch_target("/game".into())) }) as PortFuture<_>
			});
			dependencies.persist = Arc::new(move |definition| {
				assert_eq!(definition.launcher, PathBuf::from("/bin/mods.exe"));
				assert_eq!(definition.icon, PathBuf::from("/tools/Tool.exe"));
				assert_eq!(definition.working_directory, PathBuf::from("/game"));
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
						"/game",
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
				settings(display)?,
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
		let disabled = ModName::new("Disabled".into()).context(CreateShortcutError)?;
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		let mut dependencies = dependencies()?;
		dependencies.prepare_environment_plan = Arc::new(|_| {
			Box::pin(async {
				Ok(EnvironmentPlan {
					providers: vec![EnvironmentProvider {
						identity: ProviderIdentity::DataMod {
							mod_name: ModName::new("Disabled".into())
								.context(ErrorMarker::invalid_mod_name())?,
							priority: ModPriority::new(0),
						},
						enabled: false,
					}],
					state: AdapterState::new(()),
				})
			}) as PortFuture<_>
		});
		dependencies.persist = Arc::new(move |_| {
			observed.store(true, Ordering::SeqCst);
			Box::pin(async { Ok(()) })
		});
		let result = create_shortcut(
			dependencies,
			settings(None)?,
			OutputTarget::DataMod(disabled.clone()),
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
				== Some(&ErrorMarker::output_target_disabled().with_mod_name(disabled.clone()))));
		assert!(!called.load(Ordering::SeqCst));
		Ok(())
	}

	#[tokio::test]
	async fn unavailable_launcher_stops_before_launch_validation() -> Result<(), CreateShortcutError> {
		let called = Arc::new(AtomicBool::new(false));
		let resolved = called.clone();
		let published = called.clone();
		let mut dependencies = dependencies()?;
		dependencies.locate_launcher =
			Arc::new(|| Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) }));
		dependencies.resolve_launch_target = Arc::new(move |_, _, _| {
			resolved.store(true, Ordering::SeqCst);
			Box::pin(async { Err(report!(ErrorMarker::program_unsupported())) }) as PortFuture<_>
		});
		dependencies.persist = Arc::new(move |_| {
			published.store(true, Ordering::SeqCst);
			Box::pin(async { Ok(()) })
		});
		let result = create_shortcut(
			dependencies,
			settings(None)?,
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
		assert!(error.iter_reports().any(|cause| cause
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::ShortcutUnsupported)));
		assert!(!called.load(Ordering::SeqCst));
		Ok(())
	}

	#[tokio::test]
	async fn caller_cancellation_reaches_validation_and_never_publishes() -> Result<(), CreateShortcutError> {
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		let mut dependencies = dependencies()?;
		dependencies.prepare_environment_plan = Arc::new(|token| {
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
			settings(None)?,
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
