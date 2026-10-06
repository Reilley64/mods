use crate::commands::Cli;
use crate::commands::Command;
use crate::commands::ConfigCommand;
use crate::commands::ConflictsCommand;
use crate::commands::SetCommand;
use crate::conflict_output;
use crate::diagnostics::DiagnosticSession;
use crate::diagnostics::SINK_WARNING;
use crate::diagnostics::SessionStart;
use crate::error;
use crate::export_output;
use crate::invocation::CommandDependencies;
use crate::invocation::Composition;
use crate::invocation::ExitStatusFamily;
use crate::invocation::Invocation;
use crate::invocation::invocation;
use crate::json_conflicts;
use crate::json_export;
use crate::json_install;
use crate::json_output;
use crate::operation;
use crate::output;
use crate::path_resolution::resolve_path;
use application::ErrorMarker;
use application::conflicts::explain_path;
use application::conflicts::inspect_mod_conflicts;
use application::conflicts::list_effective_conflicts;
use application::environment::InitializeEnvironmentWarning;
use application::environment::initialize_environment;
use application::execution::ExecutionWarning;
use application::export::export_environment;
use application::installation::InstallArchiveOutput;
use application::installation::InstallModOutput;
use application::installation::ModSource;
use application::installation::RemoteModSource;
use application::installation::install_mod;
use application::ports::ProgressEvent;
use application::ports::ReportProgress;
use application::settings::get_setting;
use application::settings::list_settings;
use application::settings::set_game_directory;
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
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use std::env::current_dir;
use std::env::var_os;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use tokio_util::sync::CancellationToken;

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
	dependency_factory: impl AsyncFnOnce(
		&EnvironmentRoot,
		&Path,
		Composition,
	) -> RootResult<CommandDependencies, ErrorMarker>,
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

	let Invocation {
		operation_name,
		quiet_success,
		exit_status_family,
		composition,
	} = invocation(&cli.command);
	let (mut session, diagnostic_warning) =
		match DiagnosticSession::start(root.as_path(), cli.log_level, operation_name) {
			SessionStart::FileBacked(session) => (Some(session), String::new()),
			SessionStart::Disabled => (None, String::new()),
			SessionStart::SetupFailed => (None, format!("{SINK_WARNING}\n")),
		};

	let session_id = session.as_ref().map(DiagnosticSession::id);
	let diagnostic_log = session.as_ref().map(DiagnosticSession::path);
	let dependencies = match dependency_factory(&root, &startup_directory, composition).await {
		Ok(dependencies) => dependencies,
		Err(report) => {
			let marker = report.current_context();
			let status = match exit_status_family {
				ExitStatusFamily::Ordinary => error::exit_status(marker.code()),
				ExitStatusFamily::Execution => error::execution_exit_status(marker.code()),
			};
			let mut stderr = error::application_error(&report);
			stderr.push_str(&diagnostic_warning);
			if let Some(id) = session_id {
				stderr.push_str(&format!("diagnostic session: {id}\n"));
			}

			if let Some(session) = session.take() {
				session.finish("failure");
			}

			let mut presentation = cli
				.json
				.then(|| JsonPresentation::Problem(json_output::marker_problem(marker, status)));
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
				status,
				stdout: String::new(),
				stderr,
			}
			.publish_json();
		}
	};

	let work = dispatch(
		cli.command,
		dependencies,
		root,
		startup_directory,
		cli.log_level.to_string(),
		cli.json,
	);
	let mut result = match session.as_ref() {
		Some(session) => session.capture(work).await,
		None => work.await,
	};

	result.diagnostic_log = diagnostic_log;
	let terminal = if result.status == 0 {
		"success"
	} else if result.status == error::STATUS_CONTROL_C_EXIT {
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
	dependencies: CommandDependencies,
	root: EnvironmentRoot,
	startup: PathBuf,
	log_level: String,
	json_mode: bool,
) -> RunOutcome {
	match (command, dependencies) {
		(Command::Init { game_install }, CommandDependencies::InitializeEnvironment(dependencies)) => {
			let game_install = match game_install
				.map(|path| resolve_path(&path, &startup))
				.map(GameInstallationPath::new)
				.transpose()
			{
				Ok(game_install) => game_install,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::game_install_invalid()),
						json_mode,
					);
				}
			};

			match initialize_environment(dependencies, root, game_install, operation::ctrl_c_token()).await
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
		(
			Command::Config {
				command: ConfigCommand::List,
			},
			CommandDependencies::ListSettings { dependencies, settings },
		) => match list_settings(dependencies, settings.settings).await {
			Ok(output) => success(
				output::settings(&output.settings),
				String::new(),
				json_mode,
				json!({"settings": output.settings.iter().map(json_output::setting).collect::<Vec<_>>() }),
				vec![],
			),
			Err(report) => report_outcome(&report, json_mode),
		},
		(
			Command::Config {
				command: ConfigCommand::Get { key },
			},
			CommandDependencies::GetSetting { dependencies, settings },
		) => match get_setting(dependencies, settings.settings, key.into()).await {
			Ok(output) => success(
				output::setting(&output.setting),
				String::new(),
				json_mode,
				json!({"setting": json_output::setting(&output.setting)}),
				vec![],
			),
			Err(report) => report_outcome(&report, json_mode),
		},
		(
			Command::Config {
				command: ConfigCommand::Set {
					command: SetCommand::GameDir { value },
				},
			},
			CommandDependencies::SetGameDirectory(dependencies),
		) => {
			let path = match GameInstallationPath::new(resolve_path(&value, &startup)) {
				Ok(path) => path,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::setting_value_invalid()),
						json_mode,
					);
				}
			};

			match set_game_directory(dependencies, path, operation::ctrl_c_token()).await {
				Ok(output) => {
					let (stdout, stderr) = output::set_game_directory(&output);
					success(stdout, stderr, json_mode, json!({}), vec![])
				}
				Err(report) => report_outcome(&report, json_mode),
			}
		}
		(Command::Install(arguments), CommandDependencies::InstallMod(dependencies)) => {
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
				let archive = match ArchivePath::new(resolve_path(&arguments.archive, &startup)) {
					Ok(archive) => archive,
					Err(report) => {
						return report_outcome(
							&report.context(ErrorMarker::unsafe_archive()),
							json_mode,
						);
					}
				};
				ModSource::Local(archive)
			};

			let mod_name = match arguments.name.map(ModName::new).transpose() {
				Ok(mod_name) => mod_name,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::invalid_mod_name()),
						json_mode,
					);
				}
			};
			let choices = match parse_choices(arguments.choice) {
				Ok(choices) => choices,
				Err(report) => return report_outcome(&report, json_mode),
			};

			match install_mod(
				dependencies,
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
		(
			Command::Conflicts {
				command: ConflictsCommand::List { compare_content },
			},
			CommandDependencies::ListEffectiveConflicts(dependencies),
		) => match list_effective_conflicts(dependencies, compare_content, operation::ctrl_c_token()).await {
			Ok(output) => success(
				conflict_output::list(&output),
				String::new(),
				json_mode,
				json_conflicts::list(&output),
				vec![],
			),
			Err(report) => report_outcome(&report, json_mode),
		},
		(
			Command::Conflicts {
				command: ConflictsCommand::Inspect {
					mod_name,
					compare_content,
				},
			},
			CommandDependencies::InspectModConflicts(dependencies),
		) => {
			let mod_name = match ModName::new(mod_name) {
				Ok(mod_name) => mod_name,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::invalid_mod_name()),
						json_mode,
					);
				}
			};

			match inspect_mod_conflicts(dependencies, mod_name, compare_content, operation::ctrl_c_token())
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
		(
			Command::Conflicts {
				command: ConflictsCommand::Explain { path, compare_content },
			},
			CommandDependencies::ExplainPath(dependencies),
		) => {
			let path = match DataRelativePath::new(path) {
				Ok(path) => path,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::invalid_data_path()),
						json_mode,
					);
				}
			};

			match explain_path(dependencies, path, compare_content, operation::ctrl_c_token()).await {
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
		(Command::Export(arguments), CommandDependencies::ExportEnvironment(dependencies)) => {
			let output_path = resolve_path(&arguments.output, &startup);

			match export_environment(
				dependencies,
				output_path.clone(),
				arguments.include_saves,
				arguments.include_game_data,
				arguments.dry_run,
				operation::ctrl_c_token(),
			)
			.await
			{
				Ok(result) => {
					let stdout = if arguments.dry_run {
						export_output::preview(&output_path, &result)
					} else {
						String::new()
					};
					let stderr = result.warnings.iter().map(output::plugin_warning).collect();
					let value = json_export::export(&output_path, &result);
					let warnings = json_export::plugin_warnings(&result.warnings);
					success(stdout, stderr, json_mode, value, warnings)
				}
				Err(report) => {
					let marker = error::application_marker(&report);
					let status = marker.map_or(1, |marker| error::exit_status(marker.code()));

					let mut details = json_output::report_details(&report, "retained_export_stage");
					details.insert("output".into(), json!(output_path.display().to_string()));

					with_problem_details(
						problem_outcome(
							status,
							error::export_error(&report, &output_path),
							json_mode,
							marker,
						),
						details,
					)
				}
			}
		}
		(Command::Shortcut(arguments), CommandDependencies::CreateShortcut { dependencies, settings }) => {
			let output_target = match arguments.output_target.map_or(Ok(OutputTarget::Overwrite), |name| {
				ModName::new(name).map(OutputTarget::DataMod)
			}) {
				Ok(output_target) => output_target,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::invalid_output_target()),
						json_mode,
					);
				}
			};
			let working_directory = match arguments
				.cwd
				.map(|path| WorkingDirectory::new(resolve_path(&path, &startup)))
				.transpose()
			{
				Ok(working_directory) => working_directory,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::invalid_working_directory()),
						json_mode,
					);
				}
			};
			let mut command = arguments.command.into_iter();
			let Some(program) = command.next() else {
				return marker_outcome(ErrorMarker::program_not_found(), json_mode);
			};
			let program = match Program::new(program) {
				Ok(program) => program,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::program_not_found()),
						json_mode,
					);
				}
			};
			let program_arguments = match command.map(ProgramArgument::new).collect::<Result<Vec<_>, _>>() {
				Ok(program_arguments) => program_arguments,
				Err(report) => {
					return report_outcome(
						&report.context(ErrorMarker::program_launch_failed()),
						json_mode,
					);
				}
			};

			match create_shortcut(
				dependencies,
				settings,
				output_target,
				working_directory,
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
		(
			Command::Exec(arguments),
			CommandDependencies::ExecuteProgram {
				execute_program,
				force_cancellation,
			},
		) => {
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

			let output_target = match arguments.output_target.map_or(Ok(OutputTarget::Overwrite), |name| {
				ModName::new(name).map(OutputTarget::DataMod)
			}) {
				Ok(output_target) => output_target,
				Err(report) => {
					return execution_report_outcome(
						&report.context(ErrorMarker::invalid_output_target()),
						json_mode,
						false,
					);
				}
			};
			let working_directory = match arguments
				.cwd
				.map(|path| {
					WorkingDirectory::new(path).and_then(|path| {
						WorkingDirectory::new(resolve_path(path.as_path(), &startup))
					})
				})
				.transpose()
			{
				Ok(working_directory) => working_directory,
				Err(report) => {
					return execution_report_outcome(
						&report.context(ErrorMarker::invalid_working_directory()),
						json_mode,
						false,
					);
				}
			};
			let mut command = arguments.command.into_iter();
			let Some(program) = command.next() else {
				return execution_report_outcome(
					&report!(ErrorMarker::program_not_found()),
					json_mode,
					false,
				);
			};
			let program = match Program::new(program) {
				Ok(program) => program,
				Err(report) => {
					return execution_report_outcome(
						&report.context(ErrorMarker::program_not_found()),
						json_mode,
						false,
					);
				}
			};
			let program_arguments = match command.map(ProgramArgument::new).collect::<Result<Vec<_>, _>>() {
				Ok(program_arguments) => program_arguments,
				Err(report) => {
					return execution_report_outcome(
						&report.context(ErrorMarker::program_launch_failed()),
						json_mode,
						false,
					);
				}
			};

			let launched = Arc::new(AtomicBool::new(false));
			let launch_state = launched.clone();
			let report_progress: ReportProgress = Arc::new(move |event| {
				let launch_state = launch_state.clone();
				Box::pin(async move {
					if event == ProgressEvent::ExecutionPrepared {
						launch_state.store(true, Ordering::Release);
					}
				})
			});

			let signals = if arguments.hidden {
				None
			} else {
				Some(operation::ExecutionSignals::new(force_cancellation))
			};
			let cancellation = signals
				.as_ref()
				.map_or_else(CancellationToken::new, |signals| signals.cancellation.clone());

			match execute_program
				.call_once((
					Some(report_progress),
					output_target,
					working_directory,
					program,
					program_arguments,
					cancellation,
				))
				.await
			{
				Ok(output) => {
					let mut stderr = String::new();
					for warning in output.warnings {
						match warning {
							ExecutionWarning::Plugin(warning) => {
								stderr.push_str(&output::plugin_warning(&warning))
							}
							ExecutionWarning::ProfileStateInvalid => stderr.push_str(
								"warning [profile_state_invalid]: retained Profile State is invalid; correct it before the next execution\n",
							),
						}
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
		_ => marker_outcome(ErrorMarker::environment_invalid(None), json_mode),
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
	let outcome = RunOutcome {
		execution_failed: true,
		..problem_outcome(status, error::execution_error(report), json_mode && !launched, marker)
	};
	with_problem_details(outcome, json_output::report_details(report, "retained_execution_inis"))
}

fn with_problem_details(mut outcome: RunOutcome, details: Map<String, Value>) -> RunOutcome {
	let Some(JsonPresentation::Problem(Value::Object(problem))) = &mut outcome.presentation else {
		return outcome;
	};
	if details.is_empty() {
		return outcome;
	}

	match problem.entry("details").or_insert_with(|| json!({})) {
		Value::Object(existing) => existing.extend(details),
		other => *other = Value::Object(details),
	}

	outcome
}

fn parse_choices(values: Vec<String>) -> RootResult<Vec<FomodChoice>, ErrorMarker> {
	values.into_iter()
		.enumerate()
		.map(|(sequence, value)| {
			let (group_id, option_id) = value.split_once('=').ok_or_else(|| {
				report!(ErrorMarker::invalid_selection(
					"choices",
					None,
					None,
					Some(sequence as u64),
				))
			})?;
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

pub(crate) async fn run_current_process(
	cli: Result<Cli, ClapError>,
	dependency_factory: impl AsyncFnOnce(
		&EnvironmentRoot,
		&Path,
		Composition,
	) -> RootResult<CommandDependencies, ErrorMarker>,
) -> Result<RunOutcome, ClapError> {
	let startup_directory = current_dir().map_err(|error| ClapError::raw(ErrorKind::Io, error.to_string()))?;
	let local_app_data = var_os("LOCALAPPDATA").map(PathBuf::from);
	let cli = cli?;
	Ok(execute(cli, startup_directory, local_app_data, dependency_factory).await)
}

#[cfg(test)]
mod tests {
	use super::Cli;
	use super::CommandDependencies;
	use super::RunOutcome;
	use super::execute;
	use super::hidden_failure_dialog;
	use super::parse_choices;
	use super::run_current_process;
	use crate::commands::parse_from;
	use crate::diagnostics::SINK_WARNING;
	use crate::invocation::compose;
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
	use application::execution::ExecuteProgram;
	use application::execution::ExecuteProgramError;
	use application::execution::ExecuteProgramOutput;
	use application::execution::ExecutionWarning;
	use application::export::ExportEnvironmentDependencies;
	use application::export::ExportFile;
	use application::export::ExportListing;
	use application::export::ExportProvider;
	use application::export::ExportSelection;
	use application::export::ExportSources;
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
	use application::ports::AdapterState;
	use application::ports::EnvironmentPlan;
	use application::ports::EnvironmentProvider;
	use application::ports::GameInstallationSource;
	use application::ports::InitializationProfileSources;
	use application::ports::InitializationTargetAssessment;
	use application::ports::InstallationChange;
	use application::ports::LaunchTarget;
	use application::ports::LoadOrderFile;
	use application::ports::PortFuture;
	use application::ports::ProfileProjection;
	use application::ports::ProfileWarning;
	use application::ports::ProgressEvent;
	use application::ports::ResolvedGameInstallation;
	use application::ports::RetainedProfile;
	use application::ports::StagedProfile;
	use application::preparation::PluginWarning;
	use application::settings::ListSettingsDependencies;
	use application::settings::ResolvedSettings;
	use application::settings::SettingKey;
	use application::settings::SettingRecord;
	use application::settings::SettingSource;
	use application::settings::SettingValue;
	use application::shortcut::CreateShortcutDependencies;
	use clap::Error as ClapError;
	use clap::Parser;
	use domain::ArchiveIdentity;
	use domain::ArchivePath;
	use domain::DataRelativePath;
	use domain::EnvironmentRoot;
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
	#[cfg(windows)]
	use domain::WorkingDirectory;
	#[cfg(not(windows))]
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

	async fn run(
		arguments: impl IntoIterator<Item = OsString>,
		startup_directory: PathBuf,
		local_app_data: Option<PathBuf>,
		dependency_factory: impl FnOnce(&EnvironmentRoot) -> Result<CommandDependencies, ErrorMarker>,
	) -> Result<RunOutcome, ClapError> {
		let cli = parse_from(arguments)?;
		Ok(
			execute(cli, startup_directory, local_app_data, async move |root, _, _| {
				dependency_factory(root).map_err(|marker| report!(marker))
			})
			.await,
		)
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

	fn launch_target(working_directory: PathBuf, program: PathBuf) -> LaunchTarget {
		LaunchTarget {
			program,
			working_directory,
			state: AdapterState::new(()),
		}
	}

	fn empty_plan() -> PortFuture<EnvironmentPlan> {
		Box::pin(async {
			Ok(EnvironmentPlan {
				providers: Vec::new(),
				state: AdapterState::new(()),
			})
		})
	}

	fn shortcut_dependencies() -> CreateShortcutDependencies {
		CreateShortcutDependencies {
			locate_launcher: Arc::new(|| {
				Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
			}),
			resolve_launch_target: Arc::new(|program, _, cwd, _| {
				let target = launch_target(cwd.as_path().to_owned(), program.as_os_str().into());
				Box::pin(async move { Ok(target) }) as PortFuture<_>
			}),
			prepare_environment_plan: Arc::new(|_| empty_plan()),
			project_profile: Arc::new(|_: &EnvironmentPlan| {
				Box::pin(async { Ok(ProfileProjection { warnings: Vec::new() }) }) as PortFuture<_>
			}),
			locate_environment_root: Arc::new(|| {
				Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })
			}),
			persist: Arc::new(|_| Box::pin(async { Err(report!(ErrorMarker::shortcut_unsupported())) })),
		}
	}

	fn initialize_environment_dependencies(binding: GameBinding) -> InitializeEnvironmentDependencies {
		InitializeEnvironmentDependencies {
			assess_target: Arc::new(|_, _| {
				Box::pin(async { Ok(InitializationTargetAssessment::Available) }) as PortFuture<_>
			}),
			read_game_override: Arc::new(|| Box::pin(async { Ok(None) }) as PortFuture<_>),
			validate_game_directory: Arc::new({
				let binding = binding.clone();
				move |_, _| {
					let binding = binding.clone();
					Box::pin(async move { Ok(binding) }) as PortFuture<_>
				}
			}),
			discover_game_installation: Arc::new(move |_| {
				let binding = binding.clone();
				Box::pin(async move {
					Ok(ResolvedGameInstallation {
						binding,
						source: GameInstallationSource::Steam,
					})
				}) as PortFuture<_>
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
		}
	}

	fn unavailable_install_mod_dependencies() -> InstallModDependencies {
		InstallModDependencies {
			install_archive: unavailable_install_archive_dependencies(),
			download_mod: Arc::new(|_, _| {
				Box::pin(async { Err(report!(ErrorMarker::nexus_network_failure())) }) as PortFuture<_>
			}),
		}
	}

	fn execution_dependencies(execute_program: ExecuteProgram) -> CommandDependencies {
		CommandDependencies::ExecuteProgram {
			execute_program,
			force_cancellation: CancellationToken::new(),
		}
	}

	fn failing_execution_dependencies() -> CommandDependencies {
		execution_dependencies(Box::new(|_, _, _, _, _, _| {
			Box::pin(async { Err(report!(ErrorMarker::vfs_failed()).context(ExecuteProgramError)) })
				as PortFuture<_, _>
		}))
	}

	/// Export ports that list one profile file and record whether it was written.
	fn export_dependencies(
		expected_output: PathBuf,
		expected_selection: ExportSelection,
		written: Arc<AtomicBool>,
	) -> ExportEnvironmentDependencies {
		ExportEnvironmentDependencies {
			validate_export_destination: Arc::new(move |output| {
				assert_eq!(output, expected_output);
				Box::pin(async { Ok(()) })
			}),
			prepare_environment_plan: Arc::new(|_| {
				Box::pin(async {
					Ok(EnvironmentPlan {
						providers: Vec::new(),
						state: AdapterState::new(()),
					})
				})
			}),
			project_profile: Arc::new(|_: &EnvironmentPlan| {
				Box::pin(async {
					Ok(ProfileProjection {
						warnings: vec![ProfileWarning::Unavailable {
							plugin: "Missing.esp".into(),
						}],
					})
				})
			}),
			stage_profile: Arc::new(|_: &EnvironmentPlan, _, _| {
				Box::pin(async {
					Ok(StagedProfile {
						directory: PathBuf::from("stage"),
						state: AdapterState::new(()),
					})
				})
			}),
			list_export_files: Arc::new(
				move |_: &EnvironmentPlan, _: &StagedProfile, selection: ExportSelection, _| {
					assert_eq!(selection, expected_selection);
					Box::pin(async {
						Ok(ExportListing {
							files: vec![ExportFile {
								source_id: 0,
								path: DataRelativePath::new(
									"profile/Fallout.ini".to_owned(),
								)
								.map_err(|_| {
									report!(ErrorMarker::invalid_data_path())
								})?,
								provider: ExportProvider::Profile,
								bytes: 17,
							}],
							sources: ExportSources(AdapterState::new(())),
						})
					})
				},
			),
			write_export: Arc::new(move |_, _, _, _| {
				written.store(true, Ordering::SeqCst);
				Box::pin(async { Ok(()) })
			}),
			set_load_order_times: Arc::new(|_: &EnvironmentPlan, _, _| Box::pin(async { Ok(()) })),
			discard_staged_profile: Arc::new(|_| Box::pin(async { Ok(()) })),
		}
	}

	fn test_binding(root: &Path) -> Result<GameBinding, ErrorMarker> {
		let game = GameInstallationPath::new(root.join("game"))
			.map_err(|_| ErrorMarker::environment_invalid(None))?;
		Ok(GameBinding::new(game))
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

	fn resolved_settings(root: &Path) -> Result<ResolvedSettings, ErrorMarker> {
		let binding = test_binding(root)?;
		Ok(ResolvedSettings {
			settings: Vec::new(),
			effective_binding: binding.clone(),
			manifest_binding: binding,
		})
	}

	fn list_settings_dependencies(root: &Path) -> Result<CommandDependencies, ErrorMarker> {
		Ok(CommandDependencies::ListSettings {
			dependencies: ListSettingsDependencies { report_progress: None },
			settings: resolved_settings(root)?,
		})
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
		let game_binding =
			GameBinding::new(GameInstallationPath::new(temp.path().join("game")).expect("game fixture"));
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
			let mut dependencies = unavailable_install_mod_dependencies();
			dependencies.download_mod = Arc::new({
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
			dependencies.install_archive.load_installation_state = Arc::new({
				let game_binding = game_binding.clone();
				move |_, _| {
					let game_binding = game_binding.clone();
					Box::pin(async move {
						Ok(InstallationState {
							game_binding,
							installed_mods: Vec::new(),
							unlisted_mod_names: Vec::new(),
							current_winners: HashMap::new(),
							file_dependencies: HashMap::new(),
						})
					}) as PortFuture<_>
				}
			});
			dependencies.install_archive.index_archive = Arc::new({
				let archive = archive.clone();
				let index = index.clone();
				move |received, _, _| {
					assert_eq!(received, archive);
					let index = index.clone();
					Box::pin(async move { Ok(index) }) as PortFuture<_>
				}
			});
			dependencies.install_archive.assess_installation = Arc::new(|_, _| {
				Box::pin(async { Ok(InstallationAssessment { overlaps: Vec::new() }) }) as PortFuture<_>
			});
			dependencies.install_archive.scan_environment_conflicts = Arc::new(|_| {
				Box::pin(async {
					Ok(EnvironmentConflictScan {
						providers: Vec::new(),
						problems: Vec::new(),
					})
				}) as PortFuture<_>
			});
			let began = Arc::new(AtomicBool::new(false));
			let published = Arc::new(AtomicBool::new(false));
			dependencies.install_archive.begin_installation = Arc::new({
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
			dependencies.install_archive.extract_approved_files = Arc::new(|_, _, _, _, _, _| {
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
				Ok(CommandDependencies::InstallMod(dependencies))
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
			let execute_program: ExecuteProgram = Box::new(move |_, target, cwd, program, arguments, _| {
				assert_eq!(target, OutputTarget::Overwrite);
				assert!(cwd.is_none());
				assert_eq!(program.as_os_str(), "tool.exe");
				assert_eq!(
					arguments.iter().map(|value| value.as_os_str()).collect::<Vec<_>>(),
					["", "--", "雪"]
				);
				Box::pin(async move {
					Ok(ExecuteProgramOutput {
						status: ProcessStatus::new(status),
						warnings: Vec::new(),
					})
				}) as PortFuture<_, _>
			});
			let outcome = run(
				arguments!["mods", "--log-level", "off", "exec", "--", "tool.exe", "", "--", "雪"],
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				|_| Ok(execution_dependencies(execute_program)),
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
		let outcome = run(
			arguments!["mods", "exec", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(failing_execution_dependencies()),
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
			let called = Arc::new(AtomicBool::new(false));
			let observed = called.clone();
			let expected_cwd = temp.path().join("tools");
			let execute_program: ExecuteProgram =
				Box::new(move |_, target, cwd, program, arguments, cancellation| {
					observed.store(true, Ordering::SeqCst);
					assert!(
						matches!(target, OutputTarget::DataMod(name) if name.as_str() == "Tool Output")
					);
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
					Box::pin(async move {
						if failed {
							return Err(report!(
								ErrorMarker::execution_supervision_failed()
							)
							.context(ExecuteProgramError));
						}

						Ok(ExecuteProgramOutput {
							status: ProcessStatus::new(125),
							warnings: Vec::new(),
						})
					}) as PortFuture<_, _>
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
				|_| Ok(execution_dependencies(execute_program)),
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
		let outcome = run(
			arguments!["mods", "--log-level", "off", "exec", "--hidden", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(failing_execution_dependencies()),
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
		let execute_program: ExecuteProgram = Box::new(|_, _, _, _, _, _| {
			Box::pin(async {
				Ok(ExecuteProgramOutput {
					status: ProcessStatus::new(259),
					warnings: vec![
						ExecutionWarning::Plugin(PluginWarning::StalePluginEntry {
							name: "Missing.esp".into(),
						}),
						ExecutionWarning::Plugin(PluginWarning::DuplicatePluginEntry {
							name: "Duplicate.esp".into(),
						}),
						ExecutionWarning::ProfileStateInvalid,
					],
				})
			}) as PortFuture<_, _>
		});

		let outcome = run(
			arguments!["mods", "--log-level", "off", "exec", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(execution_dependencies(execute_program)),
		)
		.await?;

		assert_eq!(outcome.status, 259);
		assert!(outcome.stdout.is_empty());
		assert_eq!(
			outcome.stderr,
			"warning [stale_plugin_entry]: analysis projection: plugins.txt entry \"Missing.esp\" is absent from the analytical Data view; runtime availability is not established.\nwarning [duplicate_plugin_entry]: duplicate entry \"Duplicate.esp\" in \"plugins.txt\"; analysis projection uses the first occurrence; canonical file is unchanged.\nwarning [profile_state_invalid]: retained Profile State is invalid; correct it before the next execution\n"
		);
		Ok(())
	}

	#[tokio::test]
	async fn exec_rejects_reserved_output_target_before_calling_port() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		let execute_program: ExecuteProgram = Box::new(move |_, _, _, _, _, _| {
			observed.store(true, Ordering::SeqCst);
			Box::pin(async { Err(report!(ErrorMarker::vfs_failed()).context(ExecuteProgramError)) })
				as PortFuture<_, _>
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
			|_| Ok(execution_dependencies(execute_program)),
		)
		.await?;
		assert_eq!(outcome.status, 125);
		assert!(outcome.stderr.contains("invalid_output_target"));
		assert!(!called.load(Ordering::SeqCst));
		Ok(())
	}

	#[test]
	fn direct_choices_preserve_occurrence_order_and_whitespace() -> Result<(), Box<dyn Error>> {
		let choices = parse_choices(vec![" first = one ".to_owned(), "second=two=parts".to_owned()])
			.map_err(|report| -> Box<dyn Error> { report.into_boxed_error() })?;

		assert_eq!(choices[0].group_id, " first ");
		assert_eq!(choices[0].option_id, " one ");
		assert_eq!(choices[1].group_id, "second");
		assert_eq!(choices[1].option_id, "two=parts");
		Ok(())
	}

	#[tokio::test]
	async fn malformed_direct_choice_reports_its_total_sequence() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let outcome = run(
			arguments![
				"mods",
				"--json",
				"--log-level",
				"off",
				"install",
				"archive.zip",
				"--choice",
				"valid=choice",
				"--choice",
				"malformed",
			],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(CommandDependencies::InstallMod(unavailable_install_mod_dependencies())),
		)
		.await?;
		let problem: Value = from_str(&outcome.stderr)?;

		assert_eq!(outcome.status, 2);
		assert_eq!(problem["code"], "invalid_selection");
		assert_eq!(problem["details"]["field"], "choices");
		assert_eq!(problem["details"]["sequence"], 1);
		Ok(())
	}

	#[tokio::test]
	async fn default_and_relative_explicit_roots_resolve_against_fixed_bases() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		assert_eq!(
			selected_root(
				arguments!["mods", "--log-level", "off", "config", "list"],
				temp.path(),
				Some(temp.path())
			)
			.await?,
			temp.path().join("mods/environments/default")
		);
		assert_eq!(
			selected_root(
				arguments![
					"mods",
					"--environment",
					"portable",
					"--log-level",
					"off",
					"config",
					"list"
				],
				temp.path(),
				Some(Path::new("/ignored")),
			)
			.await?,
			temp.path().join("portable")
		);
		assert_eq!(
			selected_root(
				arguments![
					"mods",
					"--environment",
					"../portable",
					"--log-level",
					"off",
					"config",
					"list"
				],
				temp.path(),
				None,
			)
			.await?,
			temp.path()
				.parent()
				.ok_or("temporary directory must have a parent")?
				.join("portable")
		);
		Ok(())
	}

	async fn selected_root(
		arguments: Vec<OsString>,
		startup_directory: &Path,
		local_app_data: Option<&Path>,
	) -> Result<PathBuf, Box<dyn Error>> {
		let mut selected = None;
		run(
			arguments,
			startup_directory.to_owned(),
			local_app_data.map(Path::to_owned),
			|root| {
				selected = Some(root.as_path().to_owned());
				list_settings_dependencies(startup_directory)
			},
		)
		.await?;
		Ok(selected.ok_or("the dependency factory must receive the selected root")?)
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
		let (list, inspect, explain) = successful_conflict_dependencies(scan);

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
			|_| Ok(CommandDependencies::ListEffectiveConflicts(list)),
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
			|_| Ok(CommandDependencies::InspectModConflicts(inspect)),
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
			|_| Ok(CommandDependencies::ExplainPath(explain)),
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
			|_| {
				Ok(CommandDependencies::InspectModConflicts(
					unavailable_inspect_mod_conflicts_dependencies(),
				))
			},
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
			|_| Ok(CommandDependencies::ExplainPath(unavailable_explain_path_dependencies())),
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
			|_| list_settings_dependencies(&root),
		)
		.await?;
		let result = run(
			arguments!["mods", "--environment", root.as_os_str(), "config", "list"],
			temp.path().to_path_buf(),
			None,
			|_| list_settings_dependencies(&root),
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
			|_| Ok(CommandDependencies::InstallMod(unavailable_install_mod_dependencies())),
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
			|_| list_settings_dependencies(temp.path()),
		)
		.await?;
		let document: Value = from_str(&settings.stdout)?;
		assert_eq!(document, json!({"settings": [], "warnings": []}));
		assert!(settings.stderr.is_empty());

		let mutation = run(
			arguments!["mods", "--json", "--log-level", "off", "init", "--game-install", "game"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| {
				test_binding(temp.path()).map(|binding| {
					CommandDependencies::InitializeEnvironment(initialize_environment_dependencies(
						binding,
					))
				})
			},
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
			let execute_program: ExecuteProgram = Box::new(move |reporter, _, _, _, _, _| {
				Box::pin(async move {
					if launched && let Some(reporter) = reporter {
						reporter.call((ProgressEvent::ExecutionPrepared,)).await;
					}
					Err(report!(ErrorMarker::execution_supervision_failed())
						.context(ExecuteProgramError))
				}) as PortFuture<_, _>
			});
			let outcome = run(
				arguments!["mods", "--json", "--log-level", "off", "exec", "--", "tool.exe"],
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				|_| Ok(execution_dependencies(execute_program)),
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
			|_| list_settings_dependencies(&root),
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
			|_| Ok(CommandDependencies::InstallMod(unavailable_install_mod_dependencies())),
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
		);
		let mut outcomes = Vec::new();
		for json_flag in [true, false] {
			let mut dependencies = unavailable_install_mod_dependencies();
			dependencies.install_archive.load_installation_state = Arc::new({
				let game_binding = game_binding.clone();
				move |_, _| {
					let game_binding = game_binding.clone();
					Box::pin(async move {
						Ok(InstallationState {
							game_binding,
							installed_mods: Vec::new(),
							unlisted_mod_names: Vec::new(),
							current_winners: HashMap::new(),
							file_dependencies: HashMap::new(),
						})
					}) as PortFuture<_>
				}
			});
			dependencies.download_mod = Arc::new({
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
					Ok(CommandDependencies::InstallMod(dependencies))
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
	async fn stored_nexus_api_key_never_reaches_cli_output() -> Result<(), Box<dyn Error>> {
		let secret = "synthetic-nexus-secret";
		let temp = TempDir::new()?;
		let root = temp.path().canonicalize()?;
		let game = TempDir::new()?;
		for directory in ["mods", "profile", "profile/saves", "overwrite", "cache", "temp"] {
			create_dir_all(root.join(directory))?;
		}
		for file in ["plugins.txt", "modlist.txt"] {
			write(root.join("profile").join(file), b"")?;
		}
		write(
			root.join("profile/Fallout.ini"),
			concat!(
				"[General]\nbUseMyGamesDirectory=1\nSLocalSavePath=Saves\\\n",
				"[Archive]\nbInvalidateOlderFiles=1\nSInvalidationFile=\n",
				"sArchiveList=Fallout - Invalidation.bsa\n",
			),
		)?;
		let mut empty_bsa = b"BSA\0".to_vec();
		for value in [0x68_u32, 36, 0x3, 0, 0, 0, 0, 0] {
			empty_bsa.extend_from_slice(&value.to_le_bytes());
		}
		write(root.join("cache/Fallout - Invalidation.bsa"), empty_bsa)?;
		let manifest = format!(
			"schema_version = 1\ngame_dir = '{}'\nnexus_api_key = '{secret}'\n",
			game.path().display()
		);
		let missing_archive = root.join("missing.zip");
		let environment = root.as_os_str().to_owned();

		for (manifest, expect_success) in [(manifest.clone(), true), (format!("{manifest}broken = [\n"), false)]
		{
			write(root.join("mods.toml"), &manifest)?;
			for (command, succeeds) in [
				(arguments!["config", "list"], expect_success),
				(arguments!["config", "get", "game-dir"], expect_success),
				(arguments!["install", missing_archive.as_os_str(), "--dry-run"], false),
			] {
				for json_flag in [true, false] {
					let mut arguments = arguments![
						"mods",
						"--log-level",
						"off",
						"--environment",
						environment.clone()
					];
					if json_flag {
						arguments.push(OsString::from("--json"));
					}
					arguments.extend(command.clone());
					let outcome =
						execute(Cli::try_parse_from(arguments)?, root.clone(), None, compose)
							.await;
					assert_eq!(outcome.status == 0, succeeds, "{}", outcome.stderr);
					assert!(!outcome.stdout.contains(secret));
					assert!(!outcome.stderr.contains(secret));
				}
			}
		}
		Ok(())
	}
	#[tokio::test]
	async fn shortcut_forwards_startup_relative_paths_and_publishes_quietly() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for custom in [false, true] {
			let expected_root = temp.path().join("environment");
			let expected_cwd = if custom {
				temp.path().join("work")
			} else {
				temp.path().join("game")
			};
			let expected_destination = custom.then(|| temp.path().join("links"));
			let launcher = temp.path().join("mods.exe");
			let program = temp.path().join("tool.exe");
			let published = Arc::new(AtomicBool::new(false));
			let observed = published.clone();
			let resolved_cwd = expected_cwd.clone();
			let resolved_program = program.clone();
			let binding = test_binding(temp.path())
				.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
			let settings = named_settings(binding, "Vanilla Plus");
			let dependencies = CreateShortcutDependencies {
				locate_launcher: Arc::new(move || {
					let launcher = launcher.clone();
					Box::pin(async move { Ok(launcher) })
				}),
				resolve_launch_target: Arc::new(move |input, arguments, cwd, _| {
					assert_eq!(input.as_os_str(), "tool.exe");
					assert_eq!(cwd.as_path(), resolved_cwd.as_path());
					assert_eq!(
						arguments.iter().map(|value| value.as_os_str()).collect::<Vec<_>>(),
						["", "a\"b", "雪", "--"]
					);
					let target = launch_target(resolved_cwd.clone(), resolved_program.clone());
					Box::pin(async move { Ok(target) }) as PortFuture<_>
				}),
				prepare_environment_plan: Arc::new(move |_| {
					Box::pin(async move {
						let generated = ModName::new("--Generated".into())
							.map_err(|_| report!(ErrorMarker::invalid_mod_name()))?;
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
					Box::pin(async { Ok(ProfileProjection { warnings: Vec::new() }) })
						as PortFuture<_>
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
			let outcome = run(values, temp.path().to_owned(), None, |_| {
				Ok(CommandDependencies::CreateShortcut { dependencies, settings })
			})
			.await?;

			assert_eq!(outcome.status, 0);
			assert!(outcome.stdout.is_empty());
			assert!(outcome.stderr.is_empty());
			assert!(published.load(Ordering::SeqCst));
		}
		Ok(())
	}
	#[tokio::test]
	async fn shortcut_logging_setup_failure_is_quiet_only_after_success() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		write(temp.path().join("logs"), b"block diagnostic directory creation")?;
		for succeeds in [true, false] {
			let settings = resolved_settings(temp.path())
				.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
			let mut dependencies = shortcut_dependencies();
			let root = temp.path().to_owned();
			let launcher = root.join("mods.exe");
			let environment = root.clone();
			dependencies.locate_launcher = Arc::new(move || {
				let launcher = launcher.clone();
				Box::pin(async move { Ok(launcher) })
			});
			dependencies.locate_environment_root = Arc::new(move || {
				let environment = environment.clone();
				Box::pin(async move { Ok(environment) })
			});
			dependencies.persist = Arc::new(move |_| {
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
				|_| Ok(CommandDependencies::CreateShortcut { dependencies, settings }),
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
		let settings = resolved_settings(temp.path())
			.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
		let outcome = run(
			arguments!["mods", "--log-level", "off", "shortcut", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|root| {
				let dependencies = Resources::system(root.clone()).create_shortcut_dependencies(
					settings.effective_binding.clone(),
					temp.path().to_owned(),
				);
				Ok(CommandDependencies::CreateShortcut { dependencies, settings })
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
			let settings = resolved_settings(temp.path())
				.map_err(|error| -> Box<dyn Error> { report!(error).into_boxed_error() })?;
			let mut dependencies = shortcut_dependencies();
			let launcher = temp.path().join("mods.exe");
			let environment = temp.path().to_owned();
			dependencies.locate_launcher = Arc::new(move || {
				let launcher = launcher.clone();
				Box::pin(async move { Ok(launcher) })
			});
			dependencies.locate_environment_root = Arc::new(move || {
				let environment = environment.clone();
				Box::pin(async move { Ok(environment) })
			});
			dependencies.persist = Arc::new(|_| Box::pin(async { Ok(()) }));
			let mut values = arguments!["mods", "--json", "--environment", temp.path().as_os_str()];
			values.extend(command);
			let outcome = run(values, temp.path().to_owned(), None, |_| {
				Ok(CommandDependencies::CreateShortcut { dependencies, settings })
			})
			.await?;

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

	#[tokio::test]
	async fn help_and_version_never_construct_or_load_command_resources() {
		for argument in ["--help", "--version"] {
			let called = AtomicBool::new(false);
			let result = run_current_process(parse_from(arguments!["mods", argument]), async |_, _, _| {
				called.store(true, Ordering::SeqCst);
				Err(report!(ErrorMarker::environment_invalid(None)))
			})
			.await;
			assert!(result.is_err());
			assert!(!called.load(Ordering::SeqCst));
		}
	}

	#[tokio::test]
	async fn command_load_errors_keep_safe_output_and_execution_status() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for (code, expected) in [
			(ErrorMarker::environment_invalid(None), 125),
			(ErrorMarker::operation_cancelled(), 0xC000_013A),
		] {
			let cli = Cli::try_parse_from(arguments![
				"mods",
				"--log-level",
				"off",
				"exec",
				"--",
				"tool.exe"
			])?;
			let result = execute(
				cli,
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				async |_, _, _| Err(report!(std::io::Error::other("private source path")).context(code)),
			)
			.await;
			assert_eq!(result.status, expected);
			assert!(!result.stderr.contains("private source path"));
		}
		Ok(())
	}

	#[tokio::test]
	async fn exec_failure_keeps_status_and_reports_retained_ini_path_without_raw_report()
	-> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let execute_program: ExecuteProgram = Box::new(|_, _, _, _, _, _| {
			let mut failure =
				report!(ErrorMarker::execution_supervision_failed().with_phase("profile_retained"));
			failure.children_mut().push(report!(RetainedProfile {
				path: PathBuf::from("C:\\private\\inis")
			})
			.into_dynamic()
			.into_cloneable());
			Box::pin(async move { Err(failure.context(ExecuteProgramError)) }) as PortFuture<_, _>
		});

		let outcome = run(
			arguments!["mods", "--log-level", "off", "exec", "--", "tool.exe"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(execution_dependencies(execute_program)),
		)
		.await?;
		assert_eq!(outcome.status, 125);
		assert!(outcome.stdout.is_empty());
		assert!(outcome
			.stderr
			.contains("retained_execution_inis = \"C:\\\\private\\\\inis\""));
		assert!(outcome.stderr.contains("after all managed processes have stopped"));
		assert!(!outcome.stderr.contains("ExecuteProgramError"));
		Ok(())
	}

	#[tokio::test]
	async fn export_dispatch_previews_without_publication_and_publishes_quietly() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let published = Arc::new(AtomicBool::new(false));
		for dry_run in [true, false] {
			// The preview selects both optional kinds of content; the publication selects neither.
			let selection = ExportSelection {
				include_saves: dry_run,
				include_game_data: dry_run,
			};

			let dependencies =
				export_dependencies(temp.path().join("payload"), selection, published.clone());

			let arguments = if dry_run {
				arguments![
					"mods",
					"--log-level",
					"off",
					"export",
					"payload",
					"--include-saves",
					"--include-game-data",
					"--dry-run"
				]
			} else {
				arguments!["mods", "--log-level", "off", "export", "payload"]
			};
			let outcome = run(arguments, temp.path().to_owned(), Some(temp.path().to_owned()), |_| {
				Ok(CommandDependencies::ExportEnvironment(dependencies))
			})
			.await?;
			assert_eq!(outcome.status, 0);
			assert!(outcome.stderr.starts_with("warning [stale_plugin_entry]"));
			if dry_run {
				assert!(outcome.stdout.contains("files.count = 1"));
				assert!(outcome.stdout.contains("total_bytes = 17"));
				assert!(!published.load(Ordering::SeqCst));
				assert!(!temp.path().join("payload").exists());
			} else {
				assert!(outcome.stdout.is_empty());
				assert!(published.load(Ordering::SeqCst));
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn json_export_reports_both_outcomes_with_plugin_warnings() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for dry_run in [true, false] {
			let selection = ExportSelection {
				include_saves: false,
				include_game_data: false,
			};
			let dependencies = export_dependencies(
				temp.path().join("payload"),
				selection,
				Arc::new(AtomicBool::new(false)),
			);
			let mut arguments = arguments!["mods", "--json", "--log-level", "off", "export", "payload"];
			if dry_run {
				arguments.push(OsString::from("--dry-run"));
			}

			let outcome = run(arguments, temp.path().to_owned(), Some(temp.path().to_owned()), |_| {
				Ok(CommandDependencies::ExportEnvironment(dependencies))
			})
			.await?;

			assert_eq!(outcome.status, 0);
			assert!(outcome.stderr.is_empty());
			let document: Value = from_str(&outcome.stdout)?;
			assert_eq!(document["outcome"], if dry_run { "preview" } else { "published" });
			assert_eq!(document["total_bytes"], 17);
			assert_eq!(
				document["files"],
				json!([{"path": "profile\\Fallout.ini", "bytes": 17, "provider": {"kind": "profile"}}])
			);
			assert_eq!(
				document["warnings"],
				json!([{
					"code": "stale_plugin_entry",
					"message": "plugins.txt entry is absent from the analytical Data view",
					"details": {"plugin": "Missing.esp"},
				}])
			);
		}
		Ok(())
	}

	#[tokio::test]
	async fn json_export_failure_exposes_only_allowlisted_retained_paths() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let selection = ExportSelection {
			include_saves: false,
			include_game_data: false,
		};
		let mut dependencies =
			export_dependencies(temp.path().join("payload"), selection, Arc::new(AtomicBool::new(false)));
		dependencies.discard_staged_profile =
			Arc::new(|_| {
				Box::pin(async {
					Err(report!(std::io::Error::other("private cause"))
						.context(ErrorMarker::io_failure()))
				})
			});

		let outcome = run(
			arguments!["mods", "--json", "--log-level", "off", "export", "payload"],
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			|_| Ok(CommandDependencies::ExportEnvironment(dependencies)),
		)
		.await?;

		assert_eq!(outcome.status, 1);
		assert!(outcome.stdout.is_empty());
		assert!(!outcome.stderr.contains("private cause"));
		let problem: Value = from_str(&outcome.stderr)?;
		assert_eq!(problem["code"], "io_failure");
		assert_eq!(
			problem["details"],
			json!({
				"retained_export_stage": "stage",
				"output_complete": true,
				"output": temp.path().join("payload").display().to_string(),
			})
		);
		Ok(())
	}

	#[tokio::test]
	async fn json_exec_problem_names_the_load_order_file_and_unlisted_mod() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for with_file in [true, false] {
			let execute_program: ExecuteProgram = Box::new(move |_, _, _, _, _, _| {
				Box::pin(async move {
					if !with_file {
						let missing = ModName::new("Missing Mod".into()).map_err(|_| {
							report!(ErrorMarker::invalid_mod_name())
								.context(ExecuteProgramError)
						})?;
						return Err(report!(
							ErrorMarker::environment_invalid(None).with_mod_name(missing)
						)
						.context(ExecuteProgramError));
					}

					let mut failure = report!(ErrorMarker::io_failure().with_phase("load_order"));
					failure.children_mut().push(report!(LoadOrderFile {
						path: PathBuf::from("Data/FalloutNV.esm"),
					})
					.into_dynamic()
					.into_cloneable());
					Err(failure.context(ExecuteProgramError))
				}) as PortFuture<_, _>
			});

			let outcome = run(
				arguments!["mods", "--json", "--log-level", "off", "exec", "--", "tool.exe"],
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				|_| Ok(execution_dependencies(execute_program)),
			)
			.await?;

			let problem: Value = from_str(&outcome.stderr)?;
			if with_file {
				assert_eq!(problem["code"], "io_failure");
				assert_eq!(
					problem["details"],
					json!({"phase": "load_order", "load_order_file": "Data/FalloutNV.esm"})
				);
			} else {
				assert_eq!(problem["code"], "environment_invalid");
				assert_eq!(problem["details"], json!({"mod_name": "Missing Mod"}));
			}
		}
		Ok(())
	}
}
