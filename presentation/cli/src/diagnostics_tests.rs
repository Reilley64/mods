use crate::commands::LogLevel;
use crate::commands::parse_from;
use crate::diagnostics::DiagnosticSession;
use crate::diagnostics::SINK_WARNING;
use crate::diagnostics::SessionStart;
use crate::runner::Dependencies;
use crate::runner::execute;
use application::ErrorMarker;
use application::environment::InitializeEnvironmentDependencies;
use application::ports::GameInstallationSource;
use application::ports::InitializationProfileSources;
use application::ports::InitializationTargetAssessment;
use application::ports::PortFuture;
use application::ports::RecoveryOutcome;
use application::ports::ResolvedGameInstallation;
use application::ports::StoredAndEffectiveBinding;
use application::settings::GetSettingDependencies;
use application::settings::ListSettingsDependencies;
use application::settings::ResolvedSettings;
use application::settings::SetGameDirectoryDependencies;
use application::settings::SettingSource;
use domain::GameBinding;
use domain::GameInstallationPath;
use domain::SteamBuildId;
use serde_json::Value;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use uuid::Uuid;

fn dependencies_with_list(binding: GameBinding, list_settings: ListSettingsDependencies) -> Dependencies {
	let resolved = ResolvedSettings {
		settings: Vec::new(),
		effective_binding: binding.clone(),
		manifest_binding: binding.clone(),
	};
	Dependencies {
		initialize_environment: InitializeEnvironmentDependencies {
			recover_environment: Arc::new(|_, _| {
				Box::pin(async { Ok(RecoveryOutcome::NothingToRecover) }) as PortFuture<_>
			}),
			assess_target: Arc::new(|_, _| {
				Box::pin(async { Ok(InitializationTargetAssessment::Available) }) as PortFuture<_>
			}),
			read_game_override: Arc::new(|| Box::pin(async { Ok(None) }) as PortFuture<_>),
			resolve_game_installation: Arc::new({
				let binding = binding.clone();
				move |_, _, _, _| {
					let binding = binding.clone();
					Box::pin(async move {
						Ok(ResolvedGameInstallation {
							binding,
							source: GameInstallationSource::Explicit,
						})
					}) as PortFuture<_>
				}
			}),
			load_profile_sources: Arc::new(|_, _| {
				Box::pin(async {
					Ok(InitializationProfileSources {
						files: Vec::new(),
						fallout_default_ini: Vec::new(),
					})
				}) as PortFuture<_>
			}),
			publish_environment: Arc::new(|_, _, _| Box::pin(async { Ok(Vec::new()) }) as PortFuture<_>),
		},
		list_settings,
		get_setting: GetSettingDependencies {
			load_settings: Arc::new(move || {
				let resolved = resolved.clone();
				Box::pin(async move { Ok(resolved) }) as PortFuture<_>
			}),
		},
		set_game_directory: SetGameDirectoryDependencies {
			recover_environment: Arc::new(|_| Box::pin(async { Ok(()) }) as PortFuture<_>),
			validate_game_directory: Arc::new({
				let binding = binding.clone();
				move |_, _| {
					let binding = binding.clone();
					Box::pin(async move { Ok(binding) }) as PortFuture<_>
				}
			}),
			store_game_binding: Arc::new(move |_, _| {
				let stored = binding.clone();
				Box::pin(async move {
					Ok(StoredAndEffectiveBinding {
						effective: stored.clone(),
						stored,
						source: SettingSource::Manifest,
						shadowed: false,
					})
				}) as PortFuture<_>
			}),
			validate_effective_binding: Arc::new(|binding| {
				Box::pin(async move { Ok(binding) }) as PortFuture<_>
			}),
		},
	}
}

fn successful_dependencies(root: &Path) -> Result<Dependencies, Box<dyn Error>> {
	let binding = GameBinding::new(
		GameInstallationPath::new(root.join("game")).map_err(|_| "invalid test game path")?,
		SteamBuildId::new(1).map_err(|_| "invalid test build ID")?,
	);
	let listed_binding = binding.clone();
	Ok(dependencies_with_list(
		binding,
		ListSettingsDependencies {
			load_settings: Arc::new(move || {
				let binding = listed_binding.clone();
				Box::pin(async move {
					Ok(ResolvedSettings {
						settings: Vec::new(),
						effective_binding: binding.clone(),
						manifest_binding: binding,
					})
				}) as PortFuture<_>
			}),
		},
	))
}

fn records(root: &Path) -> Result<(Uuid, Vec<Value>), Box<dyn Error>> {
	let files = fs::read_dir(root.join("logs"))?.collect::<Result<Vec<_>, _>>()?;
	assert_eq!(files.len(), 1);
	let id = Uuid::parse_str(
		files[0].path()
			.file_stem()
			.and_then(|stem| stem.to_str())
			.ok_or("diagnostic file stem")?,
	)?;
	let records = fs::read_to_string(files[0].path())?
		.lines()
		.map(serde_json::from_str::<Value>)
		.collect::<Result<Vec<_>, _>>()?;
	Ok((id, records))
}

#[test]
fn off_creates_no_log_file_or_root() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let root = temp.path().join("environment");
	assert!(matches!(
		DiagnosticSession::start(&root, LogLevel::Off, "config.list"),
		SessionStart::Disabled
	));
	assert!(!root.exists());
	Ok(())
}

#[test]
fn normal_session_creates_one_uuid_file_with_boundary_events() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let session = match DiagnosticSession::start(temp.path(), LogLevel::Error, "config.list") {
		SessionStart::FileBacked(session) => session,
		SessionStart::Disabled | SessionStart::SetupFailed => return Err("expected file-backed session".into()),
	};
	let expected_id = session.id();
	session.finish("success");

	let (id, records) = records(temp.path())?;
	assert_eq!(id, expected_id);
	let events = records
		.iter()
		.filter_map(|record| record.pointer("/fields/event").and_then(Value::as_str))
		.collect::<Vec<_>>();
	assert_eq!(events, ["session.started", "session.completed"]);
	Ok(())
}

#[tokio::test]
async fn configured_log_level_filters_captured_project_events() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let session = match DiagnosticSession::start(temp.path(), LogLevel::Info, "config.list") {
		SessionStart::FileBacked(session) => session,
		SessionStart::Disabled | SessionStart::SetupFailed => return Err("expected file-backed session".into()),
	};
	session.capture(async {
		tracing::debug!(target: "application::diagnostic_test", event = "project.debug", "debug event");
		tracing::info!(target: "application::diagnostic_test", event = "project.info", "info event");
	})
	.await;
	session.finish("success");

	let (_, records) = records(temp.path())?;
	let events = records
		.iter()
		.filter_map(|record| record.pointer("/fields/event").and_then(Value::as_str))
		.collect::<Vec<_>>();
	assert_eq!(events, ["session.started", "project.info", "session.completed"]);
	Ok(())
}

#[tokio::test]
async fn appender_setup_failure_warns_without_changing_the_command_result() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let root = temp.path().join("not-a-directory");
	fs::write(&root, b"file")?;
	let root_argument = root.to_str().ok_or("UTF-8 test path")?;
	let with_diagnostics = parse_from(["mods", "--environment", root_argument, "config", "list"])?;
	let without_diagnostics = parse_from([
		"mods",
		"--environment",
		root_argument,
		"--log-level",
		"off",
		"config",
		"list",
	])?;
	let expected = execute(without_diagnostics, temp.path().to_path_buf(), None, |_| {
		successful_dependencies(&root).map_err(|_| ErrorMarker::environment_invalid(None))
	})
	.await;
	let result = execute(with_diagnostics, temp.path().to_path_buf(), None, |_| {
		successful_dependencies(&root).map_err(|_| ErrorMarker::environment_invalid(None))
	})
	.await;

	assert_eq!(result.status, expected.status);
	assert_eq!(result.stdout, expected.stdout);
	assert_eq!(expected.stderr, "");
	assert_eq!(result.stderr, format!("{SINK_WARNING}\n"));
	Ok(())
}

#[tokio::test]
async fn failed_commands_include_the_file_backed_session_id() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let root = temp.path().join("environment");
	let cli = parse_from([
		"mods",
		"--environment",
		root.to_str().ok_or("UTF-8 test path")?,
		"install",
		"archive.zip",
	])?;
	let result = execute(cli, temp.path().to_path_buf(), None, |_| {
		successful_dependencies(&root).map_err(|_| ErrorMarker::environment_invalid(None))
	})
	.await;

	let (id, records) = records(&root)?;
	assert_ne!(result.status, 0);
	assert!(result.stderr.contains(&format!("diagnostic session: {id}\n")));
	assert_eq!(
		records.last()
			.and_then(|record| record.pointer("/fields/event"))
			.and_then(Value::as_str),
		Some("session.failed")
	);
	Ok(())
}
