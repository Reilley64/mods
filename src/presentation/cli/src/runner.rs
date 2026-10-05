use crate::commands::Cli;
use crate::commands::Command;
use crate::commands::ConfigCommand;
use crate::commands::ConflictsCommand;
use crate::commands::SetCommand;
use crate::commands::parse_from;
use crate::conflict_output;
use crate::diagnostics::DiagnosticSession;
use crate::diagnostics::SINK_WARNING;
use crate::diagnostics::SessionStart;
use crate::error;
use crate::json_conflicts;
use crate::json_install;
use crate::json_output;
use crate::operation;
use crate::output;
use crate::path_resolution::resolve_path;
use application::ErrorMarker;
use application::conflicts::ExplainPathDependencies;
use application::conflicts::InspectModConflictsDependencies;
use application::conflicts::ListEffectiveConflictsDependencies;
use application::conflicts::explain_path;
use application::conflicts::inspect_mod_conflicts;
use application::conflicts::list_effective_conflicts;
use application::environment::InitializeEnvironmentDependencies;
use application::environment::InitializeEnvironmentWarning;
use application::environment::initialize_environment;
use application::execution::ExecuteProgramDependencies;
use application::execution::ExecutionWarning;
use application::execution::execute_program;
use application::installation::InstallArchiveOutput;
use application::installation::InstallModDependencies;
use application::installation::InstallModOutput;
use application::installation::ModSource;
use application::installation::RemoteModSource;
use application::installation::install_mod;
use application::ports::ProgressEvent;
use application::settings::GetSettingDependencies;
use application::settings::ListSettingsDependencies;
use application::settings::SetGameDirectoryDependencies;
use application::settings::get_setting;
use application::settings::list_settings;
use application::settings::set_game_directory;
use application::shortcut::CreateShortcutDependencies;
use application::shortcut::create_shortcut;
use clap::Error as ClapError;
use clap::error::ErrorKind;
use domain::ArchivePath;
use domain::DataRelativePath;
use domain::EnvironmentRoot;
use domain::FomodChoice;
use domain::GameInstallationPath;
use domain::InvalidEnvironmentRoot;
use domain::ModName;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use rootcause::Report;
use rootcause::Result as RootResult;
use rootcause::report;
use serde_json::Value;
use serde_json::json;
use std::env::current_dir;
use std::env::var_os;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct Dependencies {
	pub(crate) create_shortcut: CreateShortcutDependencies,
	pub(crate) execution_force_cancellation: CancellationToken,
	pub(crate) execute_program: ExecuteProgramDependencies,
	pub(crate) initialize_environment: InitializeEnvironmentDependencies,
	pub(crate) list_settings: ListSettingsDependencies,
	pub(crate) get_setting: GetSettingDependencies,
	pub(crate) set_game_directory: SetGameDirectoryDependencies,
	pub(crate) install_mod: InstallModDependencies,
	pub(crate) list_effective_conflicts: ListEffectiveConflictsDependencies,
	pub(crate) inspect_mod_conflicts: InspectModConflictsDependencies,
	pub(crate) explain_path: ExplainPathDependencies,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunOutcome {
	pub(crate) status: u32,
	pub(crate) stdout: String,
	pub(crate) stderr: String,
	pub(crate) execution_failed: bool,
	pub(crate) diagnostic_log: Option<PathBuf>,
	pub(crate) presentation: Option<JsonPresentation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonPresentation {
	Success(Value),
	Problem(Value),
}

impl RunOutcome {
	fn publish_json(mut self) -> Self {
		if let Some(presentation) = self.presentation.take() {
			match presentation {
				JsonPresentation::Success(value) => {
					self.stdout = json_output::document(&value);
					self.stderr.clear();
				}
				JsonPresentation::Problem(value) => {
					self.stdout.clear();
					self.stderr = json_output::document(&value);
				}
			}
		}

		self
	}
}

fn success(stdout: String, stderr: String, json_mode: bool, mut value: Value, warnings: Vec<Value>) -> RunOutcome {
	let mut outcome = RunOutcome {
		presentation: None,
		execution_failed: false,
		diagnostic_log: None,
		status: 0,
		stdout,
		stderr,
	};
	if json_mode {
		value["warnings"] = Value::Array(warnings);
		outcome.presentation = Some(JsonPresentation::Success(value));
	}
	outcome
}

#[derive(Debug)]
enum RootSelectionError {
	LocalAppDataUnavailable,
	InvalidRoot(InvalidEnvironmentRoot),
}

impl RootSelectionError {
	fn message(&self) -> &'static str {
		match self {
			Self::LocalAppDataUnavailable => "LOCALAPPDATA is unavailable",
			Self::InvalidRoot(_) => "environment root must be an absolute path",
		}
	}
}

impl fmt::Display for RootSelectionError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(self.message())
	}
}

impl Error for RootSelectionError {
	fn source(&self) -> Option<&(dyn Error + 'static)> {
		match self {
			Self::LocalAppDataUnavailable => None,
			Self::InvalidRoot(cause) => Some(cause),
		}
	}
}

fn select_environment_root(
	cli: &Cli,
	startup_directory: &Path,
	local_app_data: Option<&Path>,
) -> Result<EnvironmentRoot, RootSelectionError> {
	let path = match &cli.environment {
		Some(path) => resolve_path(path, startup_directory),
		None => local_app_data
			.ok_or(RootSelectionError::LocalAppDataUnavailable)?
			.join("mods/environments/default"),
	};

	EnvironmentRoot::new(path).map_err(|report| RootSelectionError::InvalidRoot(report.current_context().clone()))
}

pub(crate) async fn execute(
	cli: Cli,
	startup_directory: PathBuf,
	local_app_data: Option<PathBuf>,
	dependency_factory: impl FnOnce(&EnvironmentRoot) -> Result<Dependencies, ErrorMarker>,
) -> RunOutcome {
	let root = match select_environment_root(&cli, &startup_directory, local_app_data.as_deref()) {
		Ok(root) => root,
		Err(selection_error) => {
			let message = selection_error.message();
			let presentation = cli.json.then(|| {
				JsonPresentation::Problem(json_output::problem(
					"environment_root_selection_failed",
					"Environment Root selection failed",
					message,
					2,
					json!({}),
				))
			});
			return RunOutcome {
				presentation,
				execution_failed: true,
				diagnostic_log: None,
				status: 2,
				stdout: String::new(),
				stderr: format!("error: {message}\n"),
			}
			.publish_json();
		}
	};

	let operation_name = match &cli.command {
		Command::Init { .. } => "initialize",
		Command::Config {
			command: ConfigCommand::List,
		} => "config.list",
		Command::Config {
			command: ConfigCommand::Get { .. },
		} => "config.get",
		Command::Config {
			command: ConfigCommand::Set { .. },
		} => "config.set.game_dir",
		Command::Install(_) => "install",
		Command::Conflicts {
			command: ConflictsCommand::List { .. },
		} => "conflicts.list",
		Command::Conflicts {
			command: ConflictsCommand::Inspect { .. },
		} => "conflicts.inspect",
		Command::Conflicts {
			command: ConflictsCommand::Explain { .. },
		} => "conflicts.explain",
		Command::Exec(_) => "exec",
		Command::Shortcut(_) => "shortcut",
	};
	let (mut session, diagnostic_warning) =
		match DiagnosticSession::start(root.as_path(), cli.log_level, operation_name) {
			SessionStart::FileBacked(session) => (Some(session), String::new()),
			SessionStart::Disabled => (None, String::new()),
			SessionStart::SetupFailed => (None, format!("{SINK_WARNING}\n")),
		};

	let session_id = session.as_ref().map(DiagnosticSession::id);
	let diagnostic_log = session.as_ref().map(DiagnosticSession::path);
	let dependencies = match dependency_factory(&root) {
		Ok(dependencies) => dependencies,
		Err(marker) => {
			let mut stderr = error::marker(&marker);
			stderr.push_str(&diagnostic_warning);
			if let Some(id) = session_id {
				stderr.push_str(&format!("diagnostic session: {id}\n"));
			}

			if let Some(session) = session.take() {
				session.finish("failure");
			}

			let mut presentation =
				cli.json.then(|| JsonPresentation::Problem(json_output::marker_problem(&marker, 1)));
			if let Some(JsonPresentation::Problem(problem)) = &mut presentation {
				if !diagnostic_warning.is_empty() {
					problem["warnings"] = json!([json_output::diagnostic_warning()]);
				}
				if let Some(id) = session_id {
					problem["instance"] = json!(id.to_string());
				}
			}
			return RunOutcome {
				presentation,
				execution_failed: true,
				diagnostic_log,
				status: 1,
				stdout: String::new(),
				stderr,
			}
			.publish_json();
		}
	};

	let json_mode = cli.json;
	let quiet_success = matches!(&cli.command, Command::Shortcut(_));
	let work = dispatch(
		cli.command,
		dependencies,
		root,
		startup_directory,
		cli.log_level.to_string(),
		json_mode,
	);
	let mut result = match session.as_ref() {
		Some(session) => session.capture(work).await,
		None => work.await,
	};

	result.diagnostic_log = diagnostic_log;
	let terminal = if result.status == 0 {
		"success"
	} else if result.status == 0xC000_013A {
		"cancelled"
	} else {
		"failure"
	};

	if let Some(session) = session.take() {
		session.finish(terminal);
	}

	if let Some(presentation) = &mut result.presentation {
		match presentation {
			JsonPresentation::Success(value) => {
				if !diagnostic_warning.is_empty()
					&& let Some(warnings) = value["warnings"].as_array_mut()
				{
					warnings.push(json_output::diagnostic_warning());
				}
			}
			JsonPresentation::Problem(problem) => {
				if !diagnostic_warning.is_empty() {
					problem["warnings"] = json!([json_output::diagnostic_warning()]);
				}
				if let Some(id) = session_id {
					problem["instance"] = json!(id.to_string());
				}
			}
		}
	} else {
		if !quiet_success || result.status != 0 {
			result.stderr.push_str(&diagnostic_warning);
		}
		if result.status != 0
			&& let Some(id) = session_id
		{
			result.stderr.push_str(&format!("diagnostic session: {id}\n"));
		}
	}

	result.publish_json()
}

async fn dispatch(
	command: Command,
	dependencies: Dependencies,
	root: EnvironmentRoot,
	startup: PathBuf,
	log_level: String,
	json_mode: bool,
) -> RunOutcome {
	match command {
		Command::Init { game_install } => {
			let Ok(game_install) = game_install
				.map(|path| resolve_path(&path, &startup))
				.map(GameInstallationPath::new)
				.transpose()
			else {
				return marker_outcome(ErrorMarker::game_install_invalid(), json_mode);
			};
			match initialize_environment(
				dependencies.initialize_environment,
				root,
				game_install,
				operation::ctrl_c_token(),
			)
			.await
			{
				Ok(output) => {
					let (stdout, stderr) = output::initialization(&output);
					let warnings = output
						.warnings
						.iter()
						.map(|warning| match warning {
							InitializeEnvironmentWarning::BethesdaRegistryFallbackUsed => {
								json_output::warning(
									"bethesda_registry_fallback_used",
									"Bethesda registry fallback was used",
									json!({}),
								)
							}
						})
						.collect();
					success(stdout, stderr, json_mode, json!({}), warnings)
				}
				Err(report) => report_outcome(&report, json_mode),
			}
		}
		Command::Config { command } => match command {
			ConfigCommand::List => match list_settings(dependencies.list_settings).await {
				Ok(output) => success(
					output::settings(&output.settings),
					String::new(),
					json_mode,
					json!({"settings": output.settings.iter().map(json_output::setting).collect::<Vec<_>>() }),
					vec![],
				),
				Err(report) => report_outcome(&report, json_mode),
			},
			ConfigCommand::Get { key } => match get_setting(dependencies.get_setting, key.into()).await {
				Ok(output) => success(
					output::setting(&output.setting),
					String::new(),
					json_mode,
					json!({"setting": json_output::setting(&output.setting)}),
					vec![],
				),
				Err(report) => report_outcome(&report, json_mode),
			},
			ConfigCommand::Set {
				command: SetCommand::GameDir { value },
			} => {
				let Ok(path) = GameInstallationPath::new(resolve_path(&value, &startup)) else {
					return marker_outcome(ErrorMarker::setting_value_invalid(), json_mode);
				};
				match set_game_directory(
					dependencies.set_game_directory,
					path,
					operation::ctrl_c_token(),
				)
				.await
				{
					Ok(output) => {
						let (stdout, stderr) = output::set_game_directory(&output);
						let warnings = json_output::game_directory_warnings(&output.warnings);
						success(stdout, stderr, json_mode, json!({}), warnings)
					}
					Err(report) => report_outcome(&report, json_mode),
				}
			}
		},
		Command::Install(arguments) => {
			let cancellation = operation::ctrl_c_token();

			let source = if let Some(url) =
				arguments.archive.to_str().filter(|source| source.contains("://"))
			{
				ModSource::Remote(RemoteModSource {
					url: url.to_owned(),
					file_id: arguments.file,
				})
			} else {
				if arguments.file.is_some() {
					return marker_outcome(ErrorMarker::nexus_source_invalid(), json_mode);
				}
				let Ok(archive) = ArchivePath::new(resolve_path(&arguments.archive, &startup)) else {
					return marker_outcome(ErrorMarker::unsafe_archive(), json_mode);
				};
				ModSource::Local(archive)
			};

			let mod_name = if let Some(value) = arguments.name {
				let Ok(value) = ModName::new(value) else {
					return marker_outcome(ErrorMarker::invalid_mod_name(), json_mode);
				};
				Some(value)
			} else {
				None
			};
			let choices = match parse_choices(arguments.choice) {
				Ok(choices) => choices,
				Err(report) => return report_outcome(&report, json_mode),
			};

			match install_mod(
				dependencies.install_mod,
				source,
				mod_name,
				arguments.replace,
				choices,
				arguments.dry_run,
				cancellation,
			)
			.await
			{
				Ok(InstallModOutput::SelectionRequired(files)) => {
					let mut stderr = "error [nexus_file_selection_required]: Select a file with --file <id>.\n".to_owned();
					for file in &files {
						stderr.push_str(&format!(
							"file_id = {}, name = {}, version = {}, category = {}\n",
							file.file_id,
							output::quote(&file.name),
							output::quote(&file.version),
							output::quote(&file.category)
						));
					}
					RunOutcome {
						presentation: json_mode.then(|| {
							JsonPresentation::Problem(
								json_install::file_selection_required(&files),
							)
						}),
						execution_failed: false,
						diagnostic_log: None,
						status: 2,
						stdout: String::new(),
						stderr,
					}
				}
				Ok(InstallModOutput::Archive(InstallArchiveOutput::AdditionalSelectionsRequired(
					output_value,
				))) => {
					let (stdout, stderr) = output::additional_selections(&output_value);
					let value = json_install::additional(&output_value);
					let warnings = json_install::warnings(&output_value.warnings);
					success(stdout, stderr, json_mode, value, warnings)
				}
				Ok(InstallModOutput::Archive(InstallArchiveOutput::Preview(output_value))) => {
					let (stdout, stderr) = output::install_preview(&output_value);
					let value = json_install::preview(&output_value);
					let warnings = json_install::warnings(&output_value.plan.warnings);
					success(stdout, stderr, json_mode, value, warnings)
				}
				Ok(InstallModOutput::Archive(InstallArchiveOutput::Installed(output_value))) => {
					let stderr = output::install_warnings(&output_value.warnings);
					let warnings = json_install::warnings(&output_value.warnings);
					success(String::new(), stderr, json_mode, json_install::installed(), warnings)
				}
				Err(report) => report_outcome(&report, json_mode),
			}
		}
		Command::Conflicts { command } => match command {
			ConflictsCommand::List { compare_content } => match list_effective_conflicts(
				dependencies.list_effective_conflicts,
				compare_content,
				operation::ctrl_c_token(),
			)
			.await
			{
				Ok(output) => success(
					conflict_output::list(&output),
					String::new(),
					json_mode,
					json_conflicts::list(&output),
					vec![],
				),
				Err(report) => report_outcome(&report, json_mode),
			},
			ConflictsCommand::Inspect {
				mod_name,
				compare_content,
			} => {
				let mod_name = match ModName::new(mod_name) {
					Ok(mod_name) => mod_name,
					Err(report) => {
						return report_outcome(
							&report.context(ErrorMarker::invalid_mod_name()),
							json_mode,
						);
					}
				};

				match inspect_mod_conflicts(
					dependencies.inspect_mod_conflicts,
					mod_name,
					compare_content,
					operation::ctrl_c_token(),
				)
				.await
				{
					Ok(output) => success(
						conflict_output::inspection(&output),
						String::new(),
						json_mode,
						json_conflicts::inspection(&output),
						vec![],
					),
					Err(report) => report_outcome(&report, json_mode),
				}
			}
			ConflictsCommand::Explain { path, compare_content } => {
				let path = match DataRelativePath::new(path) {
					Ok(path) => path,
					Err(report) => {
						return report_outcome(
							&report.context(ErrorMarker::invalid_data_path()),
							json_mode,
						);
					}
				};

				match explain_path(
					dependencies.explain_path,
					path,
					compare_content,
					operation::ctrl_c_token(),
				)
				.await
				{
					Ok(output) => success(
						conflict_output::explanation(&output),
						String::new(),
						json_mode,
						json_conflicts::explanation(&output),
						vec![],
					),
					Err(report) => report_outcome(&report, json_mode),
				}
			}
		},
		Command::Shortcut(arguments) => {
			let output_target = if let Some(name) = arguments.output_target {
				let Ok(name) = ModName::new(name) else {
					return marker_outcome(ErrorMarker::invalid_output_target(), json_mode);
				};
				OutputTarget::DataMod(name)
			} else {
				OutputTarget::Overwrite
			};
			let Ok(working_directory) = WorkingDirectory::new(resolve_path(
				arguments.cwd.as_deref().unwrap_or(&startup),
				&startup,
			)) else {
				return marker_outcome(ErrorMarker::invalid_working_directory(), json_mode);
			};
			let mut command = arguments.command.into_iter();
			let Some(program) = command.next() else {
				return marker_outcome(ErrorMarker::program_not_found(), json_mode);
			};
			let Ok(program) = Program::new(program) else {
				return marker_outcome(ErrorMarker::program_not_found(), json_mode);
			};
			let Ok(program_arguments) = command.map(ProgramArgument::new).collect::<Result<Vec<_>, _>>()
			else {
				return marker_outcome(ErrorMarker::program_launch_failed(), json_mode);
			};

			match create_shortcut(
				dependencies.create_shortcut,
				output_target,
				Some(working_directory),
				program,
				program_arguments,
				arguments.name,
				arguments.destination.map(|path| resolve_path(&path, &startup)),
				log_level,
				operation::ctrl_c_token(),
			)
			.await
			{
				Ok(_) => success(String::new(), String::new(), json_mode, json!({}), vec![]),
				Err(report) => report_outcome(&report, json_mode),
			}
		}
		Command::Exec(arguments) => {
			if arguments.hidden && !cfg!(windows) {
				let marker = ErrorMarker::program_unsupported();
				return RunOutcome {
					execution_failed: true,
					..problem_outcome(
						error::execution_exit_status(marker.code()),
						"error [program_unsupported]: hidden managed execution is unsupported on this platform\n"
							.to_owned(),
						json_mode,
						Some(&marker),
					)
				};
			}

			let output_target = if let Some(name) = arguments.output_target {
				let Ok(name) = ModName::new(name) else {
					return execution_report_outcome(
						&report!(ErrorMarker::invalid_output_target()),
						json_mode,
						false,
					);
				};
				OutputTarget::DataMod(name)
			} else {
				OutputTarget::Overwrite
			};
			let Ok(working_directory) = arguments
				.cwd
				.map(|path| {
					WorkingDirectory::new(path).and_then(|path| {
						WorkingDirectory::new(resolve_path(path.as_path(), &startup))
					})
				})
				.transpose()
			else {
				return execution_report_outcome(
					&report!(ErrorMarker::invalid_working_directory()),
					json_mode,
					false,
				);
			};
			let mut command = arguments.command.into_iter();
			let Some(program) = command.next() else {
				return execution_report_outcome(
					&report!(ErrorMarker::program_not_found()),
					json_mode,
					false,
				);
			};
			let Ok(program) = Program::new(program) else {
				return execution_report_outcome(
					&report!(ErrorMarker::program_not_found()),
					json_mode,
					false,
				);
			};
			let Ok(program_arguments) = command.map(ProgramArgument::new).collect::<Result<Vec<_>, _>>()
			else {
				return execution_report_outcome(
					&report!(ErrorMarker::program_launch_failed()),
					json_mode,
					false,
				);
			};

			let launched = Arc::new(AtomicBool::new(false));
			let launch_state = launched.clone();
			let mut execution = dependencies.execute_program;
			let previous_reporter = execution.report_progress.take();
			execution.report_progress = Some(Arc::new(move |event| {
				let launch_state = launch_state.clone();
				let previous_reporter = previous_reporter.clone();
				Box::pin(async move {
					if event == ProgressEvent::ExecutionPrepared {
						launch_state.store(true, Ordering::Release);
					}
					if let Some(reporter) = previous_reporter {
						reporter.call((event,)).await;
					}
				})
			}));

			let signals = if arguments.hidden {
				None
			} else {
				Some(operation::ExecutionSignals::new(
					dependencies.execution_force_cancellation,
				))
			};
			let cancellation = signals
				.as_ref()
				.map_or_else(CancellationToken::new, |signals| signals.cancellation.clone());

			match execute_program(
				execution,
				output_target,
				working_directory,
				program,
				program_arguments,
				cancellation,
			)
			.await
			{
				Ok(output) => {
					let mut stderr = String::new();
					for warning in output.warnings {
						let message = match warning {
                            ExecutionWarning::LoadOrderNotEnforced => "warning [load_order_not_enforced]: Plugin diagnostics use the analytical Data projection, not an observed runtime view. Mappings use canonical Profile State; this computed list does not change those files. Projected plugin order is advisory and is not enforced through virtual timestamps.\n".to_owned(),
                            ExecutionWarning::StalePluginEntry { name } => format!("warning [stale_plugin_entry]: analysis projection: plugins.txt entry {} is absent from the analytical Data view; runtime availability is not established.\n", output::quote(&name)),
                            ExecutionWarning::StaleLoadOrderEntry { name } => format!("warning [stale_load_order_entry]: analysis projection: loadorder.txt entry {} is absent from the analytical Data view; runtime availability is not established.\n", output::quote(&name)),
                            ExecutionWarning::DuplicatePluginEntry { file, name } => format!("warning [duplicate_plugin_entry]: duplicate entry {} in {}; analysis projection uses the first occurrence; canonical file is unchanged.\n", output::quote(&name), output::quote(&file)),
                            ExecutionWarning::UnlistedPlugin { name } => format!("warning [unlisted_plugin]: analysis projection: {} is absent from loadorder.txt; projected order uses backing-file modification time.\n", output::quote(&name)),
                            ExecutionWarning::ProfileStateInvalid => "warning [profile_state_invalid]: retained Profile State is invalid; correct it before the next execution\n".to_owned(),
                        };
						stderr.push_str(&message);
					}
					RunOutcome {
						presentation: None,
						execution_failed: false,
						diagnostic_log: None,
						status: output.status.value(),
						stdout: String::new(),
						stderr,
					}
				}
				Err(report) => {
					execution_report_outcome(&report, json_mode, launched.load(Ordering::Acquire))
				}
			}
		}
	}
}

#[cfg(any(windows, test))]
pub(crate) fn hidden_failure_dialog(outcome: &RunOutcome) -> Option<String> {
	if !outcome.execution_failed {
		return None;
	}

	let mut message = outcome.stderr.trim_end().to_owned();
	if let Some(path) = &outcome.diagnostic_log {
		message.push_str(&format!("\nDiagnostic log: {}", path.display()));
	}
	Some(message)
}

fn execution_report_outcome<E>(report: &Report<E>, json_mode: bool, launched: bool) -> RunOutcome {
	let marker = error::application_marker(report);
	let status = marker.map_or(125, |marker| error::execution_exit_status(marker.code()));
	RunOutcome {
		execution_failed: true,
		..problem_outcome(status, error::application_error(report), json_mode && !launched, marker)
	}
}

fn parse_choices(values: Vec<String>) -> RootResult<Vec<FomodChoice>, ErrorMarker> {
	values.into_iter()
		.enumerate()
		.map(|(sequence, value)| {
			let Some((group_id, option_id)) = value.split_once('=') else {
				return Err(report!(ErrorMarker::invalid_selection(
					"choices",
					None,
					None,
					Some(sequence as u64),
				)));
			};
			Ok(FomodChoice {
				group_id: group_id.to_owned(),
				option_id: option_id.to_owned(),
			})
		})
		.collect()
}

fn marker_outcome(marker: ErrorMarker, json_mode: bool) -> RunOutcome {
	let status = error::exit_status(marker.code());
	problem_outcome(status, error::marker(&marker), json_mode, Some(&marker))
}

fn report_outcome<E>(report: &Report<E>, json_mode: bool) -> RunOutcome {
	let marker = error::application_marker(report);
	let status = marker.map_or(1, |marker| error::exit_status(marker.code()));
	problem_outcome(status, error::application_error(report), json_mode, marker)
}

fn problem_outcome(status: u32, stderr: String, json_mode: bool, marker: Option<&ErrorMarker>) -> RunOutcome {
	let presentation = json_mode.then(|| {
		JsonPresentation::Problem(marker.map_or_else(
			|| {
				json_output::problem(
					"operation_failed",
					"Operation failed",
					"operation failed",
					status,
					json!({}),
				)
			},
			|marker| json_output::marker_problem(marker, status),
		))
	});
	RunOutcome {
		presentation,
		execution_failed: false,
		diagnostic_log: None,
		status,
		stdout: String::new(),
		stderr,
	}
}

pub(crate) async fn run(
	arguments: impl IntoIterator<Item = OsString>,
	startup_directory: PathBuf,
	local_app_data: Option<PathBuf>,
	dependency_factory: impl FnOnce(&EnvironmentRoot) -> Result<Dependencies, ErrorMarker>,
) -> Result<RunOutcome, ClapError> {
	let cli = parse_from(arguments)?;
	Ok(execute(cli, startup_directory, local_app_data, dependency_factory).await)
}

pub(crate) async fn run_current_process(
	arguments: impl IntoIterator<Item = OsString>,
	dependency_factory: impl FnOnce(&EnvironmentRoot, &Path) -> Result<Dependencies, ErrorMarker>,
) -> Result<RunOutcome, ClapError> {
	let startup_directory = current_dir().map_err(|error| ClapError::raw(ErrorKind::Io, error.to_string()))?;
	let local_app_data = var_os("LOCALAPPDATA").map(PathBuf::from);
	let factory_startup = startup_directory.clone();
	run(arguments, startup_directory, local_app_data, |root| {
		dependency_factory(root, &factory_startup)
	})
	.await
}

#[cfg(test)]
mod tests {
	use super::Cli;
	use super::Dependencies;
	use super::RunOutcome;
	use super::hidden_failure_dialog;
	use super::parse_choices;
	use super::run;
	use super::select_environment_root;
	use crate::diagnostics::SINK_WARNING;
	use application::ErrorCode;
	use application::ErrorMarker;
	use application::conflicts::ConflictContentRead;
	use application::conflicts::EnvironmentConflictScan;
	use application::conflicts::ExplainPathDependencies;
	use application::conflicts::IndexedConflictFile;
	use application::conflicts::IndexedConflictFileId;
	use application::conflicts::InspectModConflictsDependencies;
	use application::conflicts::ListEffectiveConflictsDependencies;
	use application::conflicts::ScannedConflictProvider;
	use application::environment::InitializeEnvironmentDependencies;
	use application::execution::ExecuteProgramDependencies;
	use application::execution::ExecuteProgramOutput;
	use application::execution::ExecutionWarning;
	use application::installation::ArchiveIndex;
	use application::installation::DownloadModFile;
	use application::installation::DownloadModOutput;
	use application::installation::DownloadedMod;
	use application::installation::FomodGroup;
	use application::installation::FomodInstaller;
	use application::installation::FomodOption;
	use application::installation::IndexedInstaller;
	use application::installation::InstallArchiveDependencies;
	use application::installation::InstallModDependencies;
	use application::installation::InstallationAssessment;
	use application::installation::InstallationState;
	use application::installation::NexusProvenance;
	use application::ports::GameInstallationSource;
	use application::ports::InitializationProfileSources;
	use application::ports::InitializationTargetAssessment;
	use application::ports::InstallationChange;
	use application::ports::PortFuture;
	use application::ports::PrepareExecutionEnvironment;
	use application::ports::PreparedExecution;
	use application::ports::ProgressEvent;
	use application::ports::ResolveLaunchInputs;
	use application::ports::ResolvedGameInstallation;
	use application::ports::ResolvedLaunch;
	use application::ports::StoredAndEffectiveBinding;
	use application::settings::GetSettingDependencies;
	use application::settings::ListSettingsDependencies;
	use application::settings::ResolvedSettings;
	use application::settings::SetGameDirectoryDependencies;
	use application::settings::SettingKey;
	use application::settings::SettingRecord;
	use application::settings::SettingSource;
	use application::settings::SettingValue;
	use application::shortcut::CreateShortcutDependencies;
	use clap::Parser;
	use domain::ArchiveIdentity;
	use domain::ArchivePath;
	use domain::DataRelativePath;
	use domain::FomodCardinality;
	use domain::FomodCondition;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::InstallCandidate;
	use domain::InstallCandidateOrigin;
	use domain::InstallationPhase;
	use domain::ModName;
	use domain::ModPriority;
	use domain::OutputTarget;
	use domain::ParticipationReason;
	use domain::ProcessStatus;
	use domain::ProviderIdentity;
	use domain::ProviderReference;
	use domain::ResolvedOptionType;
	use domain::Sha256Digest;
	use domain::SteamBuildId;
	#[cfg(windows)]
	use domain::WorkingDirectory;
	use infrastructure_dependencies::Resources;
	use rootcause::compat::boxed_error::IntoBoxedError;
	use rootcause::report;
	use serde_json::Value;
	use serde_json::from_str;
	use serde_json::json;
	use std::collections::HashMap;
	use std::error::Error;
	use std::ffi::OsString;
	use std::fs::create_dir_all;
	use std::fs::read;
	use std::fs::read_dir;
	use std::fs::read_to_string;
	use std::fs::write;
	use std::path::Path;
	use std::path::PathBuf;
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;
	use std::sync::atomic::Ordering;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn hidden_dialog_reports_failures_but_not_child_exit_codes() {
		let failure = RunOutcome {
			presentation: None,
			status: 126,
			stdout: String::new(),
			stderr: "error [program_launch_failed]: launch failed\n".into(),
			execution_failed: true,
			diagnostic_log: Some("logs/session.jsonl".into()),
		};
		assert_eq!(
			hidden_failure_dialog(&failure).as_deref(),
			Some("error [program_launch_failed]: launch failed\nDiagnostic log: logs/session.jsonl")
		);
		let child_exit = RunOutcome {
			status: 126,
			execution_failed: false,
			..failure
		};
		assert!(hidden_failure_dialog(&child_exit).is_none());
	}

	macro_rules! arguments {
		($($value:expr),* $(,)?) => {
			vec![$(OsString::from($value)),*]
		};
	}

	fn successful_conflict_dependencies(
		scan: EnvironmentConflictScan,
	) -> (
		ListEffectiveConflictsDependencies,
		InspectModConflictsDependencies,
		ExplainPathDependencies,
	) {
		let list_scan = scan.clone();
		let inspect_scan = scan.clone();
		let explain_scan = scan;
		let list = ListEffectiveConflictsDependencies {
			report_progress: None,
			scan_environment: Arc::new(move |_| {
				let scan = list_scan.clone();
				Box::pin(async move { Ok(scan) }) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Ok(ConflictContentRead::Unavailable) }) as PortFuture<_>
			}),
		};
		let inspect = InspectModConflictsDependencies {
			report_progress: None,
			scan_environment: Arc::new(move |_| {
				let scan = inspect_scan.clone();
				Box::pin(async move { Ok(scan) }) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Ok(ConflictContentRead::Unavailable) }) as PortFuture<_>
			}),
		};
		let explain = ExplainPathDependencies {
			report_progress: None,
			scan_environment: Arc::new(move |_| {
				let scan = explain_scan.clone();
				Box::pin(async move { Ok(scan) }) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Ok(ConflictContentRead::Unavailable) }) as PortFuture<_>
			}),
		};
		(list, inspect, explain)
	}

	fn unavailable_list_effective_conflicts_dependencies() -> ListEffectiveConflictsDependencies {
		ListEffectiveConflictsDependencies {
			report_progress: None,
			scan_environment: Arc::new(|_| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
		}
	}

	fn unavailable_inspect_mod_conflicts_dependencies() -> InspectModConflictsDependencies {
		InspectModConflictsDependencies {
			report_progress: None,
			scan_environment: Arc::new(|_| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
		}
	}

	fn unavailable_explain_path_dependencies() -> ExplainPathDependencies {
		ExplainPathDependencies {
			report_progress: None,
			scan_environment: Arc::new(|_| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
		}
	}

	fn unavailable_install_archive_dependencies() -> InstallArchiveDependencies {
		InstallArchiveDependencies {
			report_progress: None,
			scan_environment_conflicts: Arc::new(|_| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			read_conflict_content: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			load_installation_state: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			assess_installation: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			index_archive: Arc::new(|_, _, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			read_game_version: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			read_xnvse_version: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			begin_installation: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
			extract_approved_files: Arc::new(|_, _, _, _, _, _| {
				Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
			}),
		}
	}

	fn resolved_launch(working_directory: PathBuf, program: PathBuf) -> ResolvedLaunch {
		ResolvedLaunch {
			command_line: program.as_os_str().to_owned(),
			program,
			working_directory,
			target_lease: Arc::new(()),
		}
	}

	fn prepared_execution(binding: GameBinding) -> PreparedExecution {
		PreparedExecution {
			game_binding: binding.clone(),
			providers: Vec::new(),
			winners: Vec::new(),
			visible_files: Vec::new(),
			profile_files: Vec::new(),
			profile_directory: PathBuf::new(),
			data_directory: binding.game_directory().as_path().join("Data"),
			cache_directory: PathBuf::new(),
			revalidation_basis: Arc::new(()),
		}
	}

	fn dependencies_with_list(binding: GameBinding, list_settings: ListSettingsDependencies) -> Dependencies {
		let resolved = ResolvedSettings {
			settings: Vec::new(),
			effective_binding: binding.clone(),
			manifest_binding: binding.clone(),
		};
		let install_archive = unavailable_install_archive_dependencies();
		let prepared = prepared_execution(binding.clone());
		let prepare_execution_environment: PrepareExecutionEnvironment = Arc::new(move |_, _| {
			let prepared = prepared.clone();
			Box::pin(async move { Ok(prepared) }) as PortFuture<_>
		});
		let resolve_launch_inputs: ResolveLaunchInputs = Arc::new(|cwd, program, _, _| {
			let launch = resolved_launch(
				cwd.map(|cwd| cwd.as_path().to_owned()).unwrap_or_default(),
				program.as_os_str().into(),
			);
			Box::pin(async move { Ok(launch) }) as PortFuture<_>
		});
		Dependencies {
			create_shortcut: CreateShortcutDependencies {
				locate_launcher: Arc::new(|| {
					Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
				}),
				resolve_launch_inputs: resolve_launch_inputs.clone(),
				prepare_execution_environment: prepare_execution_environment.clone(),
				load_settings: list_settings.load_settings.clone(),
				locate_environment_root: Arc::new(|| {
					Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
				}),
				persist: Arc::new(|_| {
					Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
				}),
			},
			execution_force_cancellation: CancellationToken::new(),
			execute_program: ExecuteProgramDependencies {
				report_progress: None,
				resolve_launch_inputs,
				prepare_execution_environment,
				run_managed_program: Arc::new(|_, _, _, _, _| {
					Box::pin(async { Err(report!(ErrorMarker::vfs_failed())) }) as PortFuture<_>
				}),
			},
			initialize_environment: InitializeEnvironmentDependencies {
				assess_target: Arc::new(|_, _| {
					Box::pin(async { Ok(InitializationTargetAssessment::Available) })
						as PortFuture<_>
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
				publish_environment: Arc::new(|_, _, _| {
					Box::pin(async { Ok(Vec::new()) }) as PortFuture<_>
				}),
			},
			list_settings,
			get_setting: GetSettingDependencies {
				report_progress: None,
				load_settings: Arc::new(move || {
					let resolved = resolved.clone();
					Box::pin(async move { Ok(resolved) }) as PortFuture<_>
				}),
			},
			set_game_directory: SetGameDirectoryDependencies {
				report_progress: None,
				check_settings_readiness: Arc::new(|_| Box::pin(async { Ok(()) }) as PortFuture<_>),
				validate_game_directory: Arc::new({
					let binding = binding.clone();
					move |_, _| {
						let binding = binding.clone();
						Box::pin(async move { Ok(binding) }) as PortFuture<_>
					}
				}),
				preview_game_binding: Arc::new({
					let binding = binding.clone();
					move |_, _| {
						let stored = binding.clone();
						Box::pin(async move {
							Ok(StoredAndEffectiveBinding {
								effective: stored.clone(),
								stored,
								source: SettingSource::Manifest,
								shadowed: false,
							})
						}) as PortFuture<_>
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
				validate_effective_binding: Arc::new(|binding, _| {
					Box::pin(async move { Ok(binding) }) as PortFuture<_>
				}),
			},
			install_mod: InstallModDependencies {
				install_archive,
				download_mod: Arc::new(|_, _| {
					Box::pin(async { Err(report!(ErrorMarker::nexus_network_failure())) })
						as PortFuture<_>
				}),
			},
			list_effective_conflicts: unavailable_list_effective_conflicts_dependencies(),
			inspect_mod_conflicts: unavailable_inspect_mod_conflicts_dependencies(),
			explain_path: unavailable_explain_path_dependencies(),
		}
	}

	fn test_binding(root: &Path) -> Result<GameBinding, ErrorMarker> {
		let game = GameInstallationPath::new(root.join("game"))
			.map_err(|_| ErrorMarker::environment_invalid(None))?;
		let build = SteamBuildId::new(1).map_err(|_| ErrorMarker::environment_invalid(None))?;
		Ok(GameBinding::new(game, build))
	}

	fn named_settings(binding: GameBinding, name: &str) -> ResolvedSettings {
		ResolvedSettings {
			settings: vec![SettingRecord {
				key: SettingKey::Name,
				value: SettingValue::String(name.to_owned()),
				source: SettingSource::Manifest,
				manifest_value: SettingValue::String(name.to_owned()),
				manifest_path: "name",
				shadowed: false,
				writable: true,
			}],
			effective_binding: binding.clone(),
			manifest_binding: binding,
		}
	}

	fn successful_dependencies(root: &Path) -> Result<Dependencies, ErrorMarker> {
		let binding = test_binding(root)?;
		let listed_binding = binding.clone();
		Ok(dependencies_with_list(
			binding,
			ListSettingsDependencies {
				report_progress: None,
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

	#[tokio::test]
	#[expect(
		clippy::expect_used,
		reason = "synthetic fixture values must retain report details on setup failure"
	)]
	async fn remote_install_replays_full_choices_previews_and_retains_cache_after_installation_failure()
	-> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let cache = temp.path().join("cache/downloads/newvegas-42-7");
		create_dir_all(&cache)?;
		let archive_path = cache.join("archive");
		write(&archive_path, b"completed archive fixture")?;
		let archive = ArchivePath::new(archive_path.clone()).expect("archive fixture");
		let provenance = NexusProvenance {
			game_domain: "newvegas".into(),
			mod_id: 42,
			file_id: 7,
			file_version: "01-beta".into(),
			mod_version: "2.0".into(),
			mod_name: "Page".into(),
			file_name: "Selected file".into(),
		};
		let game_binding = GameBinding::new(
			GameInstallationPath::new(temp.path().join("game")).expect("game fixture"),
			SteamBuildId::new(1).expect("build fixture"),
		);
		let candidate = InstallCandidate {
			candidate_id: 1,
			origin: InstallCandidateOrigin::Required,
			phase: InstallationPhase::Required,
			declared_priority: 0,
			descriptor_order: 0,
			source_member: "Data/file.txt".into(),
			destination: DataRelativePath::new("file.txt".into()).expect("path fixture"),
		};
		let groups = [("first", "a"), ("second", "b")]
			.into_iter()
			.map(|(group, option)| FomodGroup {
				id: group.into(),
				label: group.into(),
				description: String::new(),
				cardinality: FomodCardinality::SelectExactlyOne,
				condition: FomodCondition::Constant(true),
				options: vec![FomodOption {
					id: option.into(),
					label: option.into(),
					description: String::new(),
					condition: FomodCondition::Constant(true),
					default_type: ResolvedOptionType::Optional,
					type_patterns: Vec::new(),
					flag_writes: Vec::new(),
					file_candidates: Vec::new(),
					file_effects: Vec::new(),
				}],
			})
			.collect();
		let index = ArchiveIndex {
			identity: ArchiveIdentity::Fomod {
				archive_sha256: Sha256Digest::new("a".repeat(64)).expect("hash fixture"),
				package_root: String::new(),
				config_member: "fomod/ModuleConfig.xml".into(),
				config_sha256: Sha256Digest::new("b".repeat(64)).expect("hash fixture"),
			},
			installer: IndexedInstaller::Fomod(FomodInstaller {
				schema_version: "5.0".into(),
				module_condition: FomodCondition::Constant(true),
				groups,
				required_candidates: vec![candidate],
				conditional_candidates: Vec::new(),
				warnings: Vec::new(),
			}),
		};

		for (choices, dry_run, expected) in [
			(vec![], true, "additional_selections_required"),
			(vec!["first=a"], true, "additional_selections_required"),
			(vec!["first=a", "second=b"], true, "preview"),
			(vec!["first=a", "second=b"], false, "failed"),
		] {
			let mut dependencies = successful_dependencies(temp.path()).expect("dependency fixture");
			dependencies.install_mod.download_mod = Arc::new({
				let downloaded = DownloadedMod {
					suggested_name: "Selected file".into(),
					archive: archive.clone(),
					provenance: Some(provenance.clone()),
				};
				move |source, _| {
					assert_eq!(source.url, "https://www.nexusmods.com/newvegas/mods/42");
					assert_eq!(source.file_id, Some(7));
					let downloaded = downloaded.clone();
					Box::pin(async move { Ok(DownloadModOutput::Downloaded(downloaded)) })
						as PortFuture<_>
				}
			});
			dependencies.install_mod.install_archive.load_installation_state = Arc::new({
				let game_binding = game_binding.clone();
				move |_, _| {
					let game_binding = game_binding.clone();
					Box::pin(async move {
						Ok(InstallationState {
							game_binding,
							installed_mods: Vec::new(),
							current_winners: HashMap::new(),
							file_dependencies: HashMap::new(),
						})
					}) as PortFuture<_>
				}
			});
			dependencies.install_mod.install_archive.index_archive = Arc::new({
				let archive = archive.clone();
				let index = index.clone();
				move |received, _, _| {
					assert_eq!(received, archive);
					let index = index.clone();
					Box::pin(async move { Ok(index) }) as PortFuture<_>
				}
			});
			dependencies.install_mod.install_archive.assess_installation = Arc::new(|_, _| {
				Box::pin(async { Ok(InstallationAssessment { overlaps: Vec::new() }) }) as PortFuture<_>
			});
			dependencies.install_mod.install_archive.scan_environment_conflicts = Arc::new(|_| {
				Box::pin(async {
					Ok(EnvironmentConflictScan {
						providers: Vec::new(),
						problems: Vec::new(),
					})
				}) as PortFuture<_>
			});
			let began = Arc::new(AtomicBool::new(false));
			let published = Arc::new(AtomicBool::new(false));
			dependencies.install_mod.install_archive.begin_installation = Arc::new({
				let began = began.clone();
				let published = published.clone();
				let provenance = provenance.clone();
				move |approved, _| {
					began.store(true, Ordering::SeqCst);
					assert_eq!(approved.nexus, Some(provenance.clone()));
					assert_eq!(
						approved.plan
							.accepted_choices
							.iter()
							.map(|choice| (
								choice.group_id.as_str(),
								choice.option_id.as_str()
							))
							.collect::<Vec<_>>(),
						[("first", "a"), ("second", "b")]
					);
					let published = published.clone();
					Box::pin(async move {
						Ok(InstallationChange {
							begin_file: Arc::new(|_, _| {
								Box::pin(async {
									Err(report!(ErrorMarker::io_failure()))
								}) as PortFuture<_>
							}),
							finish: Arc::new(move |_| {
								published.store(true, Ordering::SeqCst);
								Box::pin(async { Ok(()) }) as PortFuture<_>
							}),
						})
					}) as PortFuture<_>
				}
			});
			dependencies.install_mod.install_archive.extract_approved_files =
				Arc::new(|_, _, _, _, _, _| {
					Box::pin(async { Err(report!(ErrorMarker::unsafe_archive())) }) as PortFuture<_>
				});
			let mut arguments = arguments![
				"mods",
				"--log-level",
				"off",
				"install",
				"https://www.nexusmods.com/newvegas/mods/42",
				"--file",
				"7"
			];
			for choice in choices {
				arguments.extend(arguments!["--choice", choice]);
			}
			if dry_run {
				arguments.push(OsString::from("--dry-run"));
			}

			let result = run(arguments, temp.path().to_owned(), Some(temp.path().to_owned()), |_| {
				Ok(dependencies)
			})
			.await?;

			if expected == "failed" {
				assert_ne!(result.status, 0);
				assert!(result.stderr.contains("unsafe_archive"));
				assert!(began.load(Ordering::SeqCst));
			} else {
				assert_eq!(result.status, 0, "{}", result.stderr);
				assert!(
					result.stdout.contains(&format!("outcome = \"{expected}\"")),
					"{}",
					result.stdout
				);
				assert!(!began.load(Ordering::SeqCst));
			}
			assert!(!published.load(Ordering::SeqCst));
			assert_eq!(read(&archive_path)?, b"completed archive fixture");
			assert!(!temp.path().join("mods/Selected file/meta.toml").exists());
		}
		Ok(())
	}

	#[tokio::test]
	async fn exec_dispatch_preserves_child_status_and_does_not_capture_streams() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for status in [0, 1, 125, 126, 127, 256, 259, 0xC000_0005] {
			let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
			dependencies.execute_program.resolve_launch_inputs = Arc::new(|cwd, program, arguments, _| {
				assert!(cwd.is_none());
				assert_eq!(program.as_os_str(), "tool.exe");
				assert_eq!(
					arguments.iter().map(|value| value.as_os_str()).collect::<Vec<_>>(),
					["", "--", "雪"]
				);
				let launch = resolved_launch(PathBuf::new(), program.as_os_str().into());
				Box::pin(async move { Ok(launch) }) as PortFuture<_>
			});
			dependencies.execute_program.run_managed_program = Arc::new(move |target, _, _, _, _| {
				assert_eq!(target, OutputTarget::Overwrite);
				Box::pin(async move {
					Ok(ExecuteProgramOutput {
						status: ProcessStatus::new(status),
						warnings: Vec::new(),
					})
				}) as PortFuture<_>
			});
			let outcome = run(
				arguments!["mods", "--log-level", "off", "exec", "--", "tool.exe", "", "--", "雪"],
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				|_| Ok(dependencies),
			)
			.await?;
			assert_eq!(outcome.status, status);
			assert!(outcome.stdout.is_empty());
			assert!(outcome.stderr.is_empty());
			assert!(!outcome.execution_failed);
		}
		Ok(())
	}

	#[tokio::test]
	async fn execution_failure_dialog_uses_the_created_diagnostic_path() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		let outcome = run(
			arguments!["mods", "exec", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(dependencies),
		)
		.await?;
		let path = outcome.diagnostic_log.as_ref().ok_or("expected diagnostic log")?;
		assert!(path.exists());
		assert!(hidden_failure_dialog(&outcome)
			.is_some_and(|message| message.contains(&path.display().to_string())));
		Ok(())
	}

	#[cfg(windows)]
	#[tokio::test]
	async fn hidden_exec_runs_managed_execution_and_classifies_its_result() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for failed in [false, true] {
			let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
			let called = Arc::new(AtomicBool::new(false));
			let observed = called.clone();
			let expected_cwd = temp.path().join("tools");
			dependencies.execute_program.resolve_launch_inputs =
				Arc::new(move |cwd, program, arguments, cancellation| {
					assert_eq!(
						cwd.as_ref().map(WorkingDirectory::as_path),
						Some(expected_cwd.as_path())
					);
					assert_eq!(program.as_os_str(), "tool.exe");
					assert_eq!(
						arguments.iter().map(|value| value.as_os_str()).collect::<Vec<_>>(),
						["", "--hidden", "雪"]
					);
					assert!(!cancellation.is_cancelled());
					let launch = resolved_launch(expected_cwd.clone(), program.as_os_str().into());
					Box::pin(async move { Ok(launch) }) as PortFuture<_>
				});
			dependencies.execute_program.run_managed_program =
				Arc::new(move |target, _, _, _, cancellation| {
					observed.store(true, Ordering::SeqCst);
					assert!(
						matches!(target, OutputTarget::DataMod(name) if name.as_str() == "Tool Output")
					);
					assert!(!cancellation.is_cancelled());
					Box::pin(async move {
						if failed {
							return Err(report!(
								ErrorMarker::execution_supervision_failed()
							));
						}

						Ok(ExecuteProgramOutput {
							status: ProcessStatus::new(125),
							warnings: Vec::new(),
						})
					}) as PortFuture<_>
				});

			let outcome = run(
				arguments![
					"mods",
					"--log-level",
					"off",
					"exec",
					"--hidden",
					"--cwd",
					"tools",
					"--output-target",
					"Tool Output",
					"--",
					"tool.exe",
					"",
					"--hidden",
					"雪"
				],
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				|_| Ok(dependencies),
			)
			.await?;

			assert!(called.load(Ordering::SeqCst));
			assert_eq!(outcome.status, 125);
			assert_eq!(outcome.execution_failed, failed);
			assert_eq!(hidden_failure_dialog(&outcome).is_some(), failed);
			assert!(outcome.diagnostic_log.is_none());
		}
		Ok(())
	}

	#[cfg(not(windows))]
	#[tokio::test]
	async fn hidden_exec_is_explicitly_unsupported_on_non_windows() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		let outcome = run(
			arguments!["mods", "--log-level", "off", "exec", "--hidden", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(dependencies),
		)
		.await?;
		assert_eq!(outcome.status, 126);
		assert_eq!(
			outcome.stderr,
			"error [program_unsupported]: hidden managed execution is unsupported on this platform\n"
		);
		assert!(outcome.execution_failed);
		Ok(())
	}

	#[tokio::test]
	async fn exec_qualifies_projection_warnings_without_changing_child_output() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		dependencies.execute_program.run_managed_program = Arc::new(|_, _, _, _, _| {
			Box::pin(async {
				Ok(ExecuteProgramOutput {
					status: ProcessStatus::new(259),
					warnings: vec![
						ExecutionWarning::LoadOrderNotEnforced,
						ExecutionWarning::StalePluginEntry {
							name: "Missing.esp".into(),
						},
						ExecutionWarning::StaleLoadOrderEntry {
							name: "Ordered.esp".into(),
						},
						ExecutionWarning::DuplicatePluginEntry {
							file: "plugins.txt".into(),
							name: "Duplicate.esp".into(),
						},
						ExecutionWarning::UnlistedPlugin {
							name: "Unlisted.esp".into(),
						},
						ExecutionWarning::ProfileStateInvalid,
					],
				})
			}) as PortFuture<_>
		});

		let outcome = run(
			arguments!["mods", "--log-level", "off", "exec", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(dependencies),
		)
		.await?;

		assert_eq!(outcome.status, 259);
		assert!(outcome.stdout.is_empty());
		assert_eq!(
			outcome.stderr,
			"warning [load_order_not_enforced]: Plugin diagnostics use the analytical Data projection, not an observed runtime view. Mappings use canonical Profile State; this computed list does not change those files. Projected plugin order is advisory and is not enforced through virtual timestamps.\nwarning [stale_plugin_entry]: analysis projection: plugins.txt entry \"Missing.esp\" is absent from the analytical Data view; runtime availability is not established.\nwarning [stale_load_order_entry]: analysis projection: loadorder.txt entry \"Ordered.esp\" is absent from the analytical Data view; runtime availability is not established.\nwarning [duplicate_plugin_entry]: duplicate entry \"Duplicate.esp\" in \"plugins.txt\"; analysis projection uses the first occurrence; canonical file is unchanged.\nwarning [unlisted_plugin]: analysis projection: \"Unlisted.esp\" is absent from loadorder.txt; projected order uses backing-file modification time.\nwarning [profile_state_invalid]: retained Profile State is invalid; correct it before the next execution\n"
		);
		Ok(())
	}

	#[tokio::test]
	async fn exec_rejects_reserved_output_target_before_calling_port() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		let resolved = called.clone();
		dependencies.execute_program.resolve_launch_inputs = Arc::new(move |_, _, _, _| {
			resolved.store(true, Ordering::SeqCst);
			Box::pin(async { Err(report!(ErrorMarker::program_not_found())) }) as PortFuture<_>
		});
		dependencies.execute_program.run_managed_program = Arc::new(move |_, _, _, _, _| {
			observed.store(true, Ordering::SeqCst);
			Box::pin(async { Err(report!(ErrorMarker::vfs_failed())) }) as PortFuture<_>
		});
		let outcome = run(
			arguments![
				"mods",
				"--log-level",
				"off",
				"exec",
				"--output-target",
				"overwrite",
				"--",
				"tool.exe"
			],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(dependencies),
		)
		.await?;
		assert_eq!(outcome.status, 125);
		assert!(outcome.stderr.contains("invalid_output_target"));
		assert!(!called.load(Ordering::SeqCst));
		Ok(())
	}

	#[test]
	fn direct_choices_preserve_occurrence_order_and_whitespace() -> Result<(), Box<dyn Error>> {
		let Ok(choices) = parse_choices(vec![" first = one ".to_owned(), "second=two=parts".to_owned()]) else {
			return Err("valid direct choices must parse".into());
		};

		assert_eq!(choices[0].group_id, " first ");
		assert_eq!(choices[0].option_id, " one ");
		assert_eq!(choices[1].group_id, "second");
		assert_eq!(choices[1].option_id, "two=parts");
		Ok(())
	}

	#[test]
	fn malformed_direct_choice_reports_its_total_sequence() -> Result<(), Box<dyn Error>> {
		let result = parse_choices(vec!["valid=choice".to_owned(), "malformed".to_owned()]);
		let Err(report) = result else {
			return Err("malformed direct choice must fail".into());
		};
		let marker = report.current_context();

		assert_eq!(marker.code(), ErrorCode::InvalidSelection);
		assert_eq!(marker.field(), Some("choices"));
		assert_eq!(marker.supplied_sequence(), Some(1));
		Ok(())
	}

	#[test]
	fn default_and_relative_explicit_roots_resolve_against_fixed_bases() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let default = Cli::try_parse_from(["mods", "--log-level", "off", "config", "list"])?;
		assert_eq!(
			select_environment_root(&default, temp.path(), Some(temp.path()))?.as_path(),
			temp.path().join("mods/environments/default")
		);
		let explicit = Cli::try_parse_from([
			"mods",
			"--environment",
			"portable",
			"--log-level",
			"off",
			"config",
			"list",
		])?;
		assert_eq!(
			select_environment_root(&explicit, temp.path(), Some(Path::new("/ignored")))?.as_path(),
			temp.path().join("portable")
		);
		let parent_relative = Cli::try_parse_from([
			"mods",
			"--environment",
			"../portable",
			"--log-level",
			"off",
			"config",
			"list",
		])?;
		assert_eq!(
			select_environment_root(&parent_relative, temp.path(), None)?.as_path(),
			temp.path()
				.parent()
				.ok_or("temporary directory must have a parent")?
				.join("portable")
		);
		Ok(())
	}

	fn dependencies_with_conflict_scan(
		root: &Path,
		scan: EnvironmentConflictScan,
	) -> Result<Dependencies, ErrorMarker> {
		let mut dependencies = successful_dependencies(root)?;
		let (list, inspect, explain) = successful_conflict_dependencies(scan);
		dependencies.list_effective_conflicts = list;
		dependencies.inspect_mod_conflicts = inspect;
		dependencies.explain_path = explain;
		Ok(dependencies)
	}

	#[tokio::test]
	async fn dispatches_all_conflict_commands_to_transport_free_use_cases() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let root = temp.path().join("environment");
		let mod_name = ModName::new("Visuals".to_owned()).map_err(|_| "valid test mod name")?;
		let lower_path =
			DataRelativePath::new("Textures/Shared.dds".to_owned()).map_err(|_| "valid test path")?;
		let mod_path =
			DataRelativePath::new("textures/shared.dds".to_owned()).map_err(|_| "valid test path")?;
		let scan = EnvironmentConflictScan {
			providers: vec![
				ScannedConflictProvider {
					identity: ProviderIdentity::DataMod {
						mod_name: ModName::new("Base Visuals".to_owned())
							.map_err(|_| "valid test mod name")?,
						priority: ModPriority::new(1),
					},
					enabled: true,
					files: vec![IndexedConflictFile {
						id: IndexedConflictFileId::new(
							ProviderIdentity::DataMod {
								mod_name: ModName::new("Base Visuals".to_owned())
									.map_err(|_| "valid test mod name")?,
								priority: ModPriority::new(1),
							},
							lower_path.clone(),
						),
						provider: ProviderReference::DataMod {
							mod_name: ModName::new("Base Visuals".to_owned())
								.map_err(|_| "valid test mod name")?,
							priority: ModPriority::new(1),
							original_path: lower_path,
							participation_reason: ParticipationReason::EnabledMod,
						},
					}],
					directories: Vec::new(),
					tombstones: Vec::new(),
					problems: Vec::new(),
				},
				ScannedConflictProvider {
					identity: ProviderIdentity::DataMod {
						mod_name: mod_name.clone(),
						priority: ModPriority::new(3),
					},
					enabled: true,
					files: vec![IndexedConflictFile {
						id: IndexedConflictFileId::new(
							ProviderIdentity::DataMod {
								mod_name: mod_name.clone(),
								priority: ModPriority::new(3),
							},
							mod_path.clone(),
						),
						provider: ProviderReference::DataMod {
							mod_name,
							priority: ModPriority::new(3),
							original_path: mod_path,
							participation_reason: ParticipationReason::EnabledMod,
						},
					}],
					directories: Vec::new(),
					tombstones: Vec::new(),
					problems: Vec::new(),
				},
			],
			problems: Vec::new(),
		};

		let listed = run(
			arguments![
				"mods",
				"--environment",
				root.as_os_str(),
				"--log-level",
				"off",
				"conflicts",
				"list",
			],
			temp.path().to_path_buf(),
			None,
			|_| dependencies_with_conflict_scan(&root, scan.clone()),
		)
		.await?;
		assert_eq!(listed.status, 0);
		assert_eq!(listed.stderr, "");
		assert!(listed.stdout.contains("resolution_status = \"exact\""));
		assert!(listed.stdout.contains("rows.count = 1"));

		let inspected = run(
			arguments![
				"mods",
				"--environment",
				root.as_os_str(),
				"--log-level",
				"off",
				"conflicts",
				"inspect",
				"visuals",
				"--compare-content",
			],
			temp.path().to_path_buf(),
			None,
			|_| dependencies_with_conflict_scan(&root, scan.clone()),
		)
		.await?;
		assert_eq!(inspected.status, 0);
		assert_eq!(inspected.stderr, "");
		assert!(inspected.stdout.contains("mod_name = \"Visuals\""));
		assert!(inspected.stdout.contains("participation = \"active\""));
		assert!(
			inspected
				.stdout
				.contains("content_comparisons[0].state = \"unavailable\""),
			"{}",
			inspected.stdout
		);

		let explained = run(
			arguments![
				"mods",
				"--environment",
				root.as_os_str(),
				"--log-level",
				"off",
				"conflicts",
				"explain",
				r"Textures\Missing.dds",
			],
			temp.path().to_path_buf(),
			None,
			|_| dependencies_with_conflict_scan(&root, scan),
		)
		.await?;
		assert_eq!(explained.status, 0);
		assert_eq!(explained.stderr, "");
		assert!(explained.stdout.contains("normalized_key = \"textures/missing.dds\""));
		assert!(explained.stdout.contains("display_path = \"Textures\\\\Missing.dds\""));
		assert!(explained.stdout.contains("effective_result.kind = \"absent\""));
		Ok(())
	}

	#[tokio::test]
	async fn conflict_dispatch_rejects_invalid_typed_inputs_before_scanning() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let root = temp.path().join("environment");
		let invalid_mod = run(
			arguments![
				"mods",
				"--environment",
				root.as_os_str(),
				"--log-level",
				"off",
				"conflicts",
				"inspect",
				"Overwrite",
			],
			temp.path().to_path_buf(),
			None,
			|_| successful_dependencies(&root),
		)
		.await?;
		assert_eq!(invalid_mod.status, 1);
		assert_eq!(invalid_mod.stderr, "error [invalid_mod_name]: mod name is invalid\n");

		let invalid_path = run(
			arguments![
				"mods",
				"--environment",
				root.as_os_str(),
				"--log-level",
				"off",
				"conflicts",
				"explain",
				"../outside.dds",
			],
			temp.path().to_path_buf(),
			None,
			|_| successful_dependencies(&root),
		)
		.await?;
		assert_eq!(invalid_path.status, 1);
		assert_eq!(
			invalid_path.stderr,
			"error [invalid_data_path]: Data-relative path is invalid\n"
		);
		Ok(())
	}

	#[tokio::test]
	async fn appender_setup_failure_warns_without_changing_the_command_result() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let root = temp.path().join("not-a-directory");
		write(&root, b"file")?;
		let expected = run(
			arguments![
				"mods",
				"--environment",
				root.as_os_str(),
				"--log-level",
				"off",
				"config",
				"list",
			],
			temp.path().to_path_buf(),
			None,
			|_| successful_dependencies(&root),
		)
		.await?;
		let result = run(
			arguments!["mods", "--environment", root.as_os_str(), "config", "list"],
			temp.path().to_path_buf(),
			None,
			|_| successful_dependencies(&root),
		)
		.await?;
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
		let result = run(
			arguments!["mods", "--environment", root.as_os_str(), "install", "archive.zip",],
			temp.path().to_path_buf(),
			None,
			|_| successful_dependencies(&root),
		)
		.await?;
		let files = read_dir(root.join("logs"))?.collect::<Result<Vec<_>, _>>()?;
		assert_eq!(files.len(), 1);
		let id = files[0]
			.path()
			.file_stem()
			.and_then(|stem| stem.to_str())
			.ok_or("diagnostic file stem")?
			.to_owned();
		let records = read_to_string(files[0].path())?;
		assert_ne!(result.status, 0);
		assert!(result.stderr.contains(&format!("diagnostic session: {id}\n")));
		assert!(records.contains("session.failed"));
		Ok(())
	}
	#[tokio::test]
	async fn json_settings_root_failures_and_mutations_have_distinct_documents() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let root_failure = run(
			arguments!["mods", "--json", "--log-level", "off", "config", "list"],
			temp.path().to_owned(),
			None,
			|_| Err(ErrorMarker::io_failure()),
		)
		.await?;
		let error: Value = from_str(&root_failure.stderr)?;
		assert_eq!(root_failure.status, 2);
		assert_eq!(error["code"], "environment_root_selection_failed");
		assert_eq!(error["exit_code"], 2);
		assert!(root_failure.stdout.is_empty());

		let settings = run(
			arguments!["mods", "--json", "--log-level", "off", "config", "list"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| successful_dependencies(temp.path()),
		)
		.await?;
		let document: Value = from_str(&settings.stdout)?;
		assert_eq!(document, json!({"settings": [], "warnings": []}));
		assert!(settings.stderr.is_empty());

		let mutation = run(
			arguments!["mods", "--json", "--log-level", "off", "init", "--game-install", "game"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| successful_dependencies(temp.path()),
		)
		.await?;
		assert_eq!(from_str::<Value>(&mutation.stdout)?, json!({"warnings": []}));
		assert!(mutation.stderr.is_empty());
		Ok(())
	}

	#[tokio::test]
	async fn json_exec_distinguishes_prelaunch_and_postlaunch_failures() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for launched in [false, true] {
			let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
			dependencies.execute_program.run_managed_program = Arc::new(move |_, _, _, reporter, _| {
				Box::pin(async move {
					if launched && let Some(reporter) = reporter {
						reporter.call((ProgressEvent::ExecutionPrepared,)).await;
					}
					Err(report!(ErrorMarker::execution_supervision_failed()))
				}) as PortFuture<_>
			});
			let outcome = run(
				arguments!["mods", "--json", "--log-level", "off", "exec", "--", "tool.exe"],
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				|_| Ok(dependencies),
			)
			.await?;
			assert_eq!(outcome.status, 125);
			if launched {
				assert!(outcome.stderr.starts_with("error [execution_supervision_failed]:"));
			} else {
				let problem: Value = from_str(&outcome.stderr)?;
				assert_eq!(problem["code"], "execution_supervision_failed");
			}
			assert!(outcome.stdout.is_empty());
		}
		Ok(())
	}
	#[tokio::test]
	async fn json_diagnostic_warning_and_failure_reference_are_structured() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let root = temp.path().join("blocked");
		write(&root, b"file")?;
		let warned = run(
			arguments!["mods", "--json", "--environment", root.as_os_str(), "config", "list"],
			temp.path().to_owned(),
			None,
			|_| successful_dependencies(&root),
		)
		.await?;
		let document: Value = from_str(&warned.stdout)?;
		assert_eq!(
			document["warnings"][0],
			json!({"code": "diagnostic_logging_unavailable", "message": "diagnostic session logging is unavailable", "details": {}})
		);
		assert!(warned.stderr.is_empty());

		let valid_root = temp.path().join("environment");
		let failed = run(
			arguments![
				"mods",
				"--json",
				"--environment",
				valid_root.as_os_str(),
				"install",
				"missing.zip"
			],
			temp.path().to_owned(),
			None,
			|_| successful_dependencies(&valid_root),
		)
		.await?;
		let problem: Value = from_str(&failed.stderr)?;
		assert_eq!(problem["code"], "io_failure");
		assert!(problem["instance"].as_str().is_some());
		assert!(!problem["detail"]
			.as_str()
			.unwrap_or_default()
			.contains("diagnostic session"));
		Ok(())
	}
	#[tokio::test]
	async fn invalid_environment_root_has_safe_json_and_unchanged_text() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let json = run(
			arguments!["mods", "--json", "--log-level", "off", "config", "list"],
			temp.path().to_owned(),
			Some(Path::new("relative-root").to_owned()),
			|_| Err(ErrorMarker::io_failure()),
		)
		.await?;
		assert_eq!(json.status, 2);
		let problem: Value = from_str(&json.stderr)?;
		assert_eq!(problem["detail"], "environment root must be an absolute path");

		let text = run(
			arguments!["mods", "--log-level", "off", "config", "list"],
			temp.path().to_owned(),
			Some(Path::new("relative-root").to_owned()),
			|_| Err(ErrorMarker::io_failure()),
		)
		.await?;
		assert_eq!(text.stderr, "error: environment root must be an absolute path\n");
		Ok(())
	}

	#[tokio::test]
	async fn nexus_file_selection_is_a_problem_with_files_in_published_order() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let files = vec![
			DownloadModFile {
				file_id: 9,
				name: "Optional".into(),
				version: "2.0".into(),
				category: "OPTIONAL".into(),
			},
			DownloadModFile {
				file_id: 7,
				name: "Main".into(),
				version: "1.0".into(),
				category: "MAIN".into(),
			},
		];
		let game_binding = GameBinding::new(
			GameInstallationPath::new(temp.path().join("game")).map_err(|_| "game fixture")?,
			SteamBuildId::new(1).map_err(|_| "build fixture")?,
		);
		let mut outcomes = Vec::new();
		for json_flag in [true, false] {
			let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
			dependencies.install_mod.install_archive.load_installation_state = Arc::new({
				let game_binding = game_binding.clone();
				move |_, _| {
					let game_binding = game_binding.clone();
					Box::pin(async move {
						Ok(InstallationState {
							game_binding,
							installed_mods: Vec::new(),
							current_winners: HashMap::new(),
							file_dependencies: HashMap::new(),
						})
					}) as PortFuture<_>
				}
			});
			dependencies.install_mod.download_mod = Arc::new({
				let files = files.clone();
				move |_, _| {
					let files = files.clone();
					Box::pin(async move { Ok(DownloadModOutput::SelectionRequired(files)) })
						as PortFuture<_>
				}
			});
			let mut arguments = arguments!["mods", "--log-level", "off"];
			if json_flag {
				arguments.push(OsString::from("--json"));
			}
			arguments.extend(arguments!["install", "https://www.nexusmods.com/newvegas/mods/42"]);
			outcomes.push(
				run(arguments, temp.path().to_owned(), Some(temp.path().to_owned()), |_| {
					Ok(dependencies)
				})
				.await?,
			);
		}

		let problem: Value = from_str(&outcomes[0].stderr)?;
		assert_eq!(outcomes[0].status, 2);
		assert!(outcomes[0].stdout.is_empty());
		assert_eq!(problem["code"], "nexus_file_selection_required");
		assert_eq!(problem["exit_code"], 2);
		assert_eq!(
			problem["details"]["files"],
			json!([
				{"file_id": 9, "name": "Optional", "version": "2.0", "category": "OPTIONAL"},
				{"file_id": 7, "name": "Main", "version": "1.0", "category": "MAIN"},
			])
		);
		assert_eq!(outcomes[1].status, 2);
		assert_eq!(
			outcomes[1].stderr,
			concat!(
				"error [nexus_file_selection_required]: Select a file with --file <id>.\n",
				"file_id = 9, name = \"Optional\", version = \"2.0\", category = \"OPTIONAL\"\n",
				"file_id = 7, name = \"Main\", version = \"1.0\", category = \"MAIN\"\n",
			)
		);
		Ok(())
	}

	#[tokio::test]
	async fn shortcut_forwards_startup_relative_paths_and_stays_quiet_without_execution()
	-> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for custom in [false, true] {
			let mut dependencies = successful_dependencies(temp.path())
				.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
			let expected_root = temp.path().join("environment");
			let expected_cwd = if custom {
				temp.path().join("work")
			} else {
				temp.path().to_owned()
			};
			let expected_destination = custom.then(|| temp.path().join("links"));
			let launcher = temp.path().join("mods.exe");
			let program = temp.path().join("tool.exe");
			let published = Arc::new(AtomicBool::new(false));
			let observed = published.clone();
			let executed = Arc::new(AtomicBool::new(false));
			let observed_execution = executed.clone();
			dependencies.execute_program.run_managed_program = Arc::new(move |_, _, _, _, _| {
				observed_execution.store(true, Ordering::SeqCst);
				Box::pin(async { Err(report!(ErrorMarker::vfs_failed())) })
			});
			let resolved_cwd = expected_cwd.clone();
			let resolved_program = program.clone();
			let binding = test_binding(temp.path())
				.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
			let prepared = prepared_execution(binding.clone());
			let settings = named_settings(binding, "Vanilla Plus");
			dependencies.create_shortcut = CreateShortcutDependencies {
				locate_launcher: Arc::new(move || {
					let launcher = launcher.clone();
					Box::pin(async move { Ok(launcher) })
				}),
				resolve_launch_inputs: Arc::new(move |cwd, input, arguments, _| {
					assert_eq!(input.as_os_str(), "tool.exe");
					assert_eq!(
						cwd.as_ref().map(|value| value.as_path()),
						Some(resolved_cwd.as_path())
					);
					assert_eq!(
						arguments.iter().map(|value| value.as_os_str()).collect::<Vec<_>>(),
						["", "a\"b", "雪", "--"]
					);
					let launch = resolved_launch(resolved_cwd.clone(), resolved_program.clone());
					Box::pin(async move { Ok(launch) }) as PortFuture<_>
				}),
				prepare_execution_environment: Arc::new(move |target, _| {
					if custom {
						assert!(
							matches!(&target, OutputTarget::DataMod(name) if name.as_str() == "--Generated")
						);
					} else {
						assert_eq!(target, OutputTarget::Overwrite);
					}
					let prepared = prepared.clone();
					Box::pin(async move { Ok(prepared) }) as PortFuture<_>
				}),
				load_settings: Arc::new(move || {
					let settings = settings.clone();
					Box::pin(async move { Ok(settings) }) as PortFuture<_>
				}),
				locate_environment_root: Arc::new(move || {
					let root = expected_root.clone();
					Box::pin(async move { Ok(root) })
				}),
				persist: Arc::new(move |definition| {
					observed.store(true, Ordering::SeqCst);
					assert_eq!(definition.destination, expected_destination);
					assert_eq!(
						definition.name,
						if custom { "My tool" } else { "Vanilla Plus — tool" }
					);
					assert_eq!(
						&definition.arguments[2..6],
						["--log-level", "off", "exec", "--hidden"].map(OsString::from)
					);
					let saved = Cli::try_parse_from(
						[OsString::from("mods")].into_iter().chain(definition.arguments),
					);
					assert!(saved.is_ok());
					Box::pin(async { Ok(()) })
				}),
			};
			let mut values =
				arguments!["mods", "--environment", "environment", "--log-level", "off", "shortcut"];
			if custom {
				values.extend(arguments![
					"--cwd",
					"work",
					"--output-target=--Generated",
					"--name",
					"My tool",
					"--destination",
					"links"
				]);
			}
			values.extend(arguments!["--", "tool.exe", "", "a\"b", "雪", "--"]);
			let outcome = run(values, temp.path().to_owned(), None, |_| Ok(dependencies)).await?;

			assert_eq!(outcome.status, 0);
			assert!(outcome.stdout.is_empty());
			assert!(outcome.stderr.is_empty());
			assert!(published.load(Ordering::SeqCst));
			assert!(!executed.load(Ordering::SeqCst));
		}
		Ok(())
	}
	#[tokio::test]
	async fn shortcut_logging_setup_failure_is_quiet_only_after_success() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		write(temp.path().join("logs"), b"block diagnostic directory creation")?;
		for succeeds in [true, false] {
			let mut dependencies = successful_dependencies(temp.path())
				.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
			let root = temp.path().to_owned();
			let launcher = root.join("mods.exe");
			let environment = root.clone();
			dependencies.create_shortcut.locate_launcher = Arc::new(move || {
				let launcher = launcher.clone();
				Box::pin(async move { Ok(launcher) })
			});
			dependencies.create_shortcut.locate_environment_root = Arc::new(move || {
				let environment = environment.clone();
				Box::pin(async move { Ok(environment) })
			});
			dependencies.create_shortcut.persist = Arc::new(move |_| {
				Box::pin(async move {
					if !succeeds {
						return Err(report!(ErrorMarker::shortcut_failed()));
					}
					Ok(())
				})
			});
			let outcome = run(
				arguments![
					"mods",
					"--environment",
					temp.path().as_os_str(),
					"--log-level",
					"debug",
					"shortcut",
					"--",
					"tool.exe"
				],
				temp.path().to_owned(),
				None,
				|_| Ok(dependencies),
			)
			.await?;

			assert!(outcome.stdout.is_empty());
			assert!(outcome.diagnostic_log.is_none());
			if succeeds {
				assert_eq!(outcome.status, 0);
				assert!(outcome.stderr.is_empty(), "{}", outcome.stderr);
			} else {
				assert_eq!(outcome.status, 1);
				assert!(outcome.stderr.contains("error [shortcut_failed]"));
				assert!(outcome.stderr.contains(SINK_WARNING));
			}
		}
		Ok(())
	}

	#[cfg(not(windows))]
	#[tokio::test]
	async fn shortcut_reports_unsupported_platform_through_normal_diagnostics() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let mut dependencies = successful_dependencies(temp.path())
			.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
		let outcome = run(
			arguments!["mods", "--log-level", "off", "shortcut", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|root| {
				dependencies.create_shortcut = Resources::system(root.clone())
					.create_shortcut_dependencies(temp.path().to_owned());
				Ok(dependencies)
			},
		)
		.await?;

		assert_eq!(outcome.status, 1);
		assert!(outcome.stdout.is_empty());
		assert_eq!(
			outcome.stderr,
			"error [shortcut_unsupported]: Launch Shortcuts are supported only on Windows\n"
		);
		Ok(())
	}

	#[tokio::test]
	async fn json_shortcut_reports_warnings_and_marker_problems() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		write(temp.path().join("logs"), b"block diagnostic directory creation")?;
		let cases = [
			(arguments!["--log-level", "off", "shortcut", "--", "tool.exe"], 0),
			(arguments!["--log-level", "debug", "shortcut", "--", "tool.exe"], 0),
			(
				arguments!["--log-level", "off", "shortcut", "--name", "a/b", "--", "tool.exe"],
				1,
			),
			(
				arguments![
					"--log-level",
					"off",
					"shortcut",
					"--output-target",
					"",
					"--",
					"tool.exe"
				],
				1,
			),
		];
		let mut documents = Vec::new();
		for (command, status) in cases {
			let mut dependencies = successful_dependencies(temp.path())
				.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
			let launcher = temp.path().join("mods.exe");
			let environment = temp.path().to_owned();
			dependencies.create_shortcut.locate_launcher = Arc::new(move || {
				let launcher = launcher.clone();
				Box::pin(async move { Ok(launcher) })
			});
			dependencies.create_shortcut.locate_environment_root = Arc::new(move || {
				let environment = environment.clone();
				Box::pin(async move { Ok(environment) })
			});
			dependencies.create_shortcut.persist = Arc::new(|_| Box::pin(async { Ok(()) }));
			let mut values = arguments!["mods", "--json", "--environment", temp.path().as_os_str()];
			values.extend(command);
			let outcome = run(values, temp.path().to_owned(), None, |_| Ok(dependencies)).await?;

			assert_eq!(outcome.status, status, "{}", outcome.stderr);
			let (document, other) = if status == 0 {
				(&outcome.stdout, &outcome.stderr)
			} else {
				(&outcome.stderr, &outcome.stdout)
			};
			assert!(other.is_empty());
			documents.push(from_str::<Value>(document)?);
		}

		assert_eq!(documents[0], json!({"warnings": []}));
		assert_eq!(documents[1]["warnings"][0]["code"], "diagnostic_logging_unavailable");
		assert_eq!(documents[2]["code"], "shortcut_name_invalid");
		assert_eq!(documents[2]["exit_code"], 1);
		assert_eq!(
			documents[2]["detail"],
			"shortcut name must be a valid Windows filename without a path"
		);
		assert_eq!(documents[3]["code"], "invalid_output_target");
		assert_eq!(documents[3]["exit_code"], 1);
		Ok(())
	}
}
