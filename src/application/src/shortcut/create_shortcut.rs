mod name;
use super::PersistShortcut;
use super::ShortcutDefinition;
use super::ValidateShortcutLaunch;
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
	pub validate_launch: ValidateShortcutLaunch,
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

	let launch = dependencies
		.validate_launch
		.call((
			output_target.clone(),
			working_directory,
			program,
			arguments.clone(),
			cancellation,
		))
		.await
		.context(CreateShortcutError)?;

	let name = if let Some(name) = explicit_name {
		name
	} else {
		let environment_name = launch.environment_name.unwrap_or_else(|| {
			launch.environment
				.file_name()
				.unwrap_or_default()
				.to_string_lossy()
				.into_owned()
		});
		let executable_name = launch.program.file_stem().unwrap_or_default().to_string_lossy();
		shortcut_name(&format!("{environment_name} — {executable_name}"), false).context(CreateShortcutError)?
	};
	let mut saved_arguments = vec![
		OsString::from("--environment"),
		launch.environment.into_os_string(),
		OsString::from("--log-level"),
		log_level.into(),
		OsString::from("exec"),
		OsString::from("--hidden"),
		OsString::from("--cwd"),
		launch.working_directory.as_os_str().to_owned(),
	];
	if let OutputTarget::DataMod(name) = output_target {
		// Binding the value with `=` preserves mod names beginning with a hyphen.
		saved_arguments.push(OsString::from(format!("--output-target={}", name.as_str())));
	}
	saved_arguments.extend([OsString::from("--"), launch.program.as_os_str().to_owned()]);
	saved_arguments.extend(arguments.into_iter().map(|argument| argument.as_os_str().to_owned()));

	dependencies
		.persist
		.call((ShortcutDefinition {
			launcher: launch.launcher,
			arguments: saved_arguments,
			working_directory: launch.working_directory,
			icon: launch.program,
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
	use crate::shortcut::ShortcutFailure;
	use crate::shortcut::ValidatedShortcutLaunch;
	use domain::ModName;
	use rootcause::report;
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;
	use std::sync::atomic::Ordering;
	#[tokio::test]
	async fn invalid_explicit_name_never_validates_or_publishes() -> Result<(), CreateShortcutError> {
		for name in [
			"../tool", "CON", "COM1.txt", "LPT¹", "trail.", "trail ", "", "a:b", "snow?",
		] {
			let dependencies = CreateShortcutDependencies {
				validate_launch: Arc::new(|_, _, _, _, _| {
					Box::pin(async { Err(report!(ShortcutFailure::InvalidLaunch)) })
				}),
				persist: Arc::new(|_| Box::pin(async { Err(report!(ShortcutFailure::Publication)) })),
			};
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
			let display = display.map(str::to_owned);
			let expected = expected.to_owned();
			let dependencies = CreateShortcutDependencies {
				validate_launch: Arc::new(move |target, cwd, program, arguments, _| {
					assert!(
						matches!(target, OutputTarget::DataMod(name) if name.as_str() == "Generated")
					);
					assert!(cwd.is_none());
					assert_eq!(program.as_os_str(), "Tool.exe");
					assert_eq!(
						arguments.iter().map(|arg| arg.as_os_str()).collect::<Vec<_>>(),
						values
					);
					let display = display.clone();
					Box::pin(async move {
						Ok(ValidatedShortcutLaunch {
							launcher: "/bin/mods.exe".into(),
							environment: "/Environment".into(),
							program: "/tools/Tool.exe".into(),
							working_directory: "/caller".into(),
							environment_name: display,
						})
					})
				}),
				persist: Arc::new(move |definition| {
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
				}),
			};
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
		let dependencies = CreateShortcutDependencies {
			validate_launch: Arc::new(|_, _, _, _, _| {
				Box::pin(async {
					Err(report!(ErrorMarker::output_target_disabled())
						.context(ShortcutFailure::InvalidLaunch))
				})
			}),
			persist: Arc::new(move |_| {
				observed.store(true, Ordering::SeqCst);
				Box::pin(async { Ok(()) })
			}),
		};
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
	async fn caller_cancellation_reaches_validation_and_never_publishes() -> Result<(), CreateShortcutError> {
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		let dependencies = CreateShortcutDependencies {
			validate_launch: Arc::new(|_, _, _, _, token| {
				Box::pin(async move {
					if token.is_cancelled() {
						return Err(report!(ErrorMarker::operation_cancelled())
							.context(ShortcutFailure::InvalidLaunch));
					}
					Err(report!(ShortcutFailure::InvalidLaunch))
				})
			}),
			persist: Arc::new(move |_| {
				observed.store(true, Ordering::SeqCst);
				Box::pin(async { Ok(()) })
			}),
		};
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
