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
use crate::export_output;
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
use application::environment::initialize_environment;
use application::execution::ExecuteProgram;
use application::execution::ExecutionWarning;
use application::export::ExportEnvironmentDependencies;
use application::export::export_environment;
use application::installation::InstallArchiveDependencies;
use application::installation::InstallArchiveOutput;
use application::installation::install_archive;
use application::settings::GetSettingDependencies;
use application::settings::ListSettingsDependencies;
use application::settings::SetGameDirectoryDependencies;
use application::settings::SettingRecord;
use application::settings::get_setting;
use application::settings::list_settings;
use application::settings::set_game_directory;
use clap::Error as ClapError;
use clap::error::ErrorKind;
use domain::ArchivePath;
use domain::DataRelativePath;
use domain::EnvironmentRoot;
use domain::FomodChoice;
use domain::GameInstallationPath;
use domain::ModName;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use rootcause::Report;
use rootcause::Result as RootResult;
use rootcause::report;
use std::env::current_dir;
use std::env::var_os;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub(crate) enum CommandDependencies {
	Initialize(InitializeEnvironmentDependencies),
	Existing(Box<Dependencies>),
}

pub(crate) struct Dependencies {
	pub(crate) settings: Vec<SettingRecord>,
	pub(crate) execution_force_cancellation: CancellationToken,
	pub(crate) execute_program: ExecuteProgram,
	pub(crate) initialize_environment: InitializeEnvironmentDependencies,
	pub(crate) list_settings: ListSettingsDependencies,
	pub(crate) get_setting: GetSettingDependencies,
	pub(crate) set_game_directory: SetGameDirectoryDependencies,
	pub(crate) install_archive: InstallArchiveDependencies,
	pub(crate) export_environment: ExportEnvironmentDependencies,
	pub(crate) list_effective_conflicts: ListEffectiveConflictsDependencies,
	pub(crate) inspect_mod_conflicts: InspectModConflictsDependencies,
	pub(crate) explain_path: ExplainPathDependencies,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunOutcome {
	pub(crate) status: u32,
	pub(crate) stdout: String,
	pub(crate) stderr: String,
}

fn select_environment_root(
	cli: &Cli,
	startup_directory: &Path,
	local_app_data: Option<&Path>,
) -> Result<EnvironmentRoot, String> {
	let path = match &cli.environment {
		Some(path) => resolve_path(path, startup_directory),
		None => local_app_data
			.ok_or_else(|| "LOCALAPPDATA is unavailable".to_owned())?
			.join("mods/environments/default"),
	};
	EnvironmentRoot::new(path).map_err(|report| report.current_context().to_string())
}

pub(crate) async fn execute(
	cli: Cli,
	startup_directory: PathBuf,
	local_app_data: Option<PathBuf>,
	dependency_factory: impl AsyncFnOnce(&EnvironmentRoot, &Command) -> RootResult<CommandDependencies, ErrorMarker>,
) -> RunOutcome {
	let root = match select_environment_root(&cli, &startup_directory, local_app_data.as_deref()) {
		Ok(root) => root,
		Err(message) => {
			return RunOutcome {
				status: 2,
				stdout: String::new(),
				stderr: format!("error: {message}\n"),
			};
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
		Command::Export(_) => "export",
	};
	let (mut session, diagnostic_warning) =
		match DiagnosticSession::start(root.as_path(), cli.log_level, operation_name) {
			SessionStart::FileBacked(session) => (Some(session), String::new()),
			SessionStart::Disabled => (None, String::new()),
			SessionStart::SetupFailed => (None, format!("{SINK_WARNING}\n")),
		};

	let session_id = session.as_ref().map(DiagnosticSession::id);
	let dependencies = match dependency_factory(&root, &cli.command).await {
		Ok(dependencies) => dependencies,
		Err(report) => {
			let mut stderr = error::application_error(&report);
			stderr.push_str(&diagnostic_warning);
			if let Some(id) = session_id {
				stderr.push_str(&format!("diagnostic session: {id}\n"));
			}
			if let Some(session) = session.take() {
				session.finish("failure");
			}
			return RunOutcome {
				status: if matches!(cli.command, Command::Exec(_)) {
					error::execution_exit_status(report.current_context().code())
				} else {
					error::exit_status(report.current_context().code())
				},
				stdout: String::new(),
				stderr,
			};
		}
	};

	let work = async move {
		match dependencies {
			CommandDependencies::Initialize(dependencies) => match cli.command {
				Command::Init { game_install } => {
					dispatch_initialization(dependencies, root, game_install, &startup_directory)
						.await
				}
				_ => marker_outcome(ErrorMarker::environment_invalid(None)),
			},
			CommandDependencies::Existing(dependencies) => {
				dispatch(cli.command, *dependencies, root, startup_directory).await
			}
		}
	};
	let mut result = match session.as_ref() {
		Some(session) => session.capture(work).await,
		None => work.await,
	};

	result.stderr.push_str(&diagnostic_warning);
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

	if result.status != 0
		&& let Some(id) = session_id
	{
		result.stderr.push_str(&format!("diagnostic session: {id}\n"));
	}
	result
}

async fn dispatch_initialization(
	dependencies: InitializeEnvironmentDependencies,
	root: EnvironmentRoot,
	game_install: Option<PathBuf>,
	startup: &Path,
) -> RunOutcome {
	let Ok(game_install) = game_install
		.map(|path| resolve_path(&path, startup))
		.map(GameInstallationPath::new)
		.transpose()
	else {
		return marker_outcome(ErrorMarker::game_install_invalid());
	};
	match initialize_environment(dependencies, root, game_install, operation::ctrl_c_token()).await {
		Ok(output) => {
			let (stdout, stderr) = output::initialization(&output);
			RunOutcome {
				status: 0,
				stdout,
				stderr,
			}
		}
		Err(report) => report_outcome(&report),
	}
}

async fn dispatch(command: Command, dependencies: Dependencies, root: EnvironmentRoot, startup: PathBuf) -> RunOutcome {
	match command {
		Command::Init { game_install } => {
			dispatch_initialization(dependencies.initialize_environment, root, game_install, &startup).await
		}
		Command::Config { command } => match command {
			ConfigCommand::List => {
				match list_settings(dependencies.list_settings, dependencies.settings).await {
					Ok(output) => RunOutcome {
						status: 0,
						stdout: output::settings(&output.settings),
						stderr: String::new(),
					},
					Err(report) => report_outcome(&report),
				}
			}
			ConfigCommand::Get { key } => {
				match get_setting(dependencies.get_setting, dependencies.settings, key.into()).await {
					Ok(output) => RunOutcome {
						status: 0,
						stdout: output::setting(&output.setting),
						stderr: String::new(),
					},
					Err(report) => report_outcome(&report),
				}
			}
			ConfigCommand::Set {
				command: SetCommand::GameDir { value },
			} => {
				let Ok(path) = GameInstallationPath::new(resolve_path(&value, &startup)) else {
					return marker_outcome(ErrorMarker::setting_value_invalid());
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
						RunOutcome {
							status: 0,
							stdout,
							stderr,
						}
					}
					Err(report) => report_outcome(&report),
				}
			}
		},
		Command::Install(arguments) => {
			let cancellation = operation::ctrl_c_token();

			let archive_path = resolve_path(&arguments.archive, &startup);
			let Ok(archive) = ArchivePath::new(archive_path) else {
				return marker_outcome(ErrorMarker::unsafe_archive());
			};
			let mod_name = if let Some(value) = arguments.name {
				let Ok(value) = ModName::new(value) else {
					return marker_outcome(ErrorMarker::invalid_mod_name());
				};
				Some(value)
			} else {
				None
			};
			let choices = match parse_choices(arguments.choice) {
				Ok(choices) => choices,
				Err(report) => return report_outcome(&report),
			};
			match install_archive(
				dependencies.install_archive,
				archive,
				mod_name,
				arguments.replace,
				choices,
				arguments.dry_run,
				cancellation,
			)
			.await
			{
				Ok(InstallArchiveOutput::AdditionalSelectionsRequired(output_value)) => {
					let (stdout, stderr) = output::additional_selections(&output_value);
					RunOutcome {
						status: 0,
						stdout,
						stderr,
					}
				}
				Ok(InstallArchiveOutput::Preview(output_value)) => {
					let (stdout, stderr) = output::install_preview(&output_value);
					RunOutcome {
						status: 0,
						stdout,
						stderr,
					}
				}
				Ok(InstallArchiveOutput::Installed(output_value)) => RunOutcome {
					status: 0,
					stdout: String::new(),
					stderr: output::install_warnings(&output_value.warnings),
				},
				Err(report) => report_outcome(&report),
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
				Ok(output) => RunOutcome {
					status: 0,
					stdout: conflict_output::list(&output),
					stderr: String::new(),
				},
				Err(report) => report_outcome(&report),
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
					Ok(output) => RunOutcome {
						status: 0,
						stdout: conflict_output::inspection(&output),
						stderr: String::new(),
					},
					Err(report) => report_outcome(&report),
				}
			}
			ConflictsCommand::Explain { path, compare_content } => {
				let path = match DataRelativePath::new(path) {
					Ok(path) => path,
					Err(report) => {
						return report_outcome(
							&report.context(ErrorMarker::invalid_data_path()),
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
					Ok(output) => RunOutcome {
						status: 0,
						stdout: conflict_output::explanation(&output),
						stderr: String::new(),
					},
					Err(report) => report_outcome(&report),
				}
			}
		},
		Command::Export(arguments) => {
			let output_path = resolve_path(&arguments.output, &startup);

			match export_environment(
				dependencies.export_environment,
				output_path.clone(),
				arguments.include_saves,
				arguments.dry_run,
				operation::ctrl_c_token(),
			)
			.await
			{
				Ok(result) => RunOutcome {
					status: 0,
					stdout: if arguments.dry_run {
						export_output::preview(&output_path, &result)
					} else {
						String::new()
					},
					stderr: result.warnings.iter().map(output::plugin_warning).collect(),
				},
				Err(report) => RunOutcome {
					status: error::application_marker(&report)
						.map_or(1, |marker| error::exit_status(marker.code())),
					stdout: String::new(),
					stderr: error::export_error(&report, &output_path),
				},
			}
		}
		Command::Exec(arguments) => {
			let output_target = if let Some(name) = arguments.output_target {
				let Ok(name) = ModName::new(name) else {
					return execution_report_outcome(
						&report!(ErrorMarker::invalid_output_target()),
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
				return execution_report_outcome(&report!(ErrorMarker::invalid_working_directory()));
			};
			let mut command = arguments.command.into_iter();
			let Some(program) = command.next() else {
				return execution_report_outcome(&report!(ErrorMarker::program_not_found()));
			};
			let Ok(program) = Program::new(program) else {
				return execution_report_outcome(&report!(ErrorMarker::program_not_found()));
			};
			let Ok(arguments) = command.map(ProgramArgument::new).collect::<Result<Vec<_>, _>>() else {
				return execution_report_outcome(&report!(ErrorMarker::program_launch_failed()));
			};

			let signals = operation::ExecutionSignals::new(dependencies.execution_force_cancellation);
			match dependencies
				.execute_program
				.call_once((
					output_target,
					working_directory,
					program,
					arguments,
					signals.cancellation.clone(),
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
						status: output.status.value(),
						stdout: String::new(),
						stderr,
					}
				}
				Err(report) => execution_report_outcome(&report),
			}
		}
	}
}

fn execution_report_outcome<E>(report: &Report<E>) -> RunOutcome {
	RunOutcome {
		status: error::application_marker(report)
			.map_or(125, |marker| error::execution_exit_status(marker.code())),
		stdout: String::new(),
		stderr: error::execution_error(report),
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

fn marker_outcome(marker: ErrorMarker) -> RunOutcome {
	let status = error::exit_status(marker.code());
	RunOutcome {
		status,
		stdout: String::new(),
		stderr: error::marker(&marker),
	}
}

fn report_outcome<E>(report: &Report<E>) -> RunOutcome {
	let status = error::application_marker(report).map_or(1, |marker| error::exit_status(marker.code()));
	RunOutcome {
		status,
		stdout: String::new(),
		stderr: error::application_error(report),
	}
}

#[cfg(test)]
pub(crate) async fn run(
	arguments: impl IntoIterator<Item = OsString>,
	startup_directory: PathBuf,
	local_app_data: Option<PathBuf>,
	dependency_factory: impl FnOnce(&EnvironmentRoot) -> Result<Dependencies, ErrorMarker>,
) -> Result<RunOutcome, ClapError> {
	let cli = parse_from(arguments)?;
	Ok(execute(cli, startup_directory, local_app_data, async move |root, _| {
		dependency_factory(root)
			.map(|dependencies| CommandDependencies::Existing(Box::new(dependencies)))
			.map_err(|marker| report!(marker))
	})
	.await)
}

pub(crate) async fn run_current_process(
	arguments: impl IntoIterator<Item = OsString>,
	dependency_factory: impl AsyncFnOnce(
		&EnvironmentRoot,
		&Path,
		&Command,
	) -> RootResult<CommandDependencies, ErrorMarker>,
) -> Result<RunOutcome, ClapError> {
	let startup_directory = current_dir().map_err(|error| ClapError::raw(ErrorKind::Io, error.to_string()))?;
	let local_app_data = var_os("LOCALAPPDATA").map(PathBuf::from);
	let factory_startup = startup_directory.clone();
	let cli = parse_from(arguments)?;
	Ok(
		execute(cli, startup_directory, local_app_data, async move |root, command| {
			dependency_factory(root, &factory_startup, command).await
		})
		.await,
	)
}

#[cfg(test)]
mod tests {
	use super::Cli;
	use super::Dependencies;
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
	use application::execution::ExecuteProgramError;
	use application::execution::ExecuteProgramOutput;
	use application::execution::ExecutionWarning;
	use application::export::ExportEnvironmentDependencies;
	use application::export::ExportFile;
	use application::export::ExportListing;
	use application::export::ExportProvider;
	use application::export::ExportSources;
	use application::installation::InstallArchiveDependencies;
	use application::ports::AdapterState;
	use application::ports::EnvironmentPlan;
	use application::ports::GameInstallationSource;
	use application::ports::InitializationProfileSources;
	use application::ports::InitializationTargetAssessment;
	use application::ports::PortFuture;
	use application::ports::ProfileProjection;
	use application::ports::ProfileWarning;
	use application::ports::ResolvedGameInstallation;
	use application::ports::RetainedProfile;
	use application::ports::StagedProfile;
	use application::ports::StoredAndEffectiveBinding;
	use application::preparation::PluginWarning;
	use application::settings::GetSettingDependencies;
	use application::settings::ListSettingsDependencies;
	use application::settings::SetGameDirectoryDependencies;
	use application::settings::SettingSource;
	use clap::Parser;
	use domain::DataRelativePath;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::ModName;
	use domain::ModPriority;
	use domain::OutputTarget;
	use domain::ParticipationReason;
	use domain::ProcessStatus;
	use domain::ProviderIdentity;
	use domain::ProviderReference;
	use rootcause::report;
	use std::error::Error;
	use std::ffi::OsString;
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

	fn dependencies_with_list(binding: GameBinding, list_settings: ListSettingsDependencies) -> Dependencies {
		let install_archive = unavailable_install_archive_dependencies();
		Dependencies {
			settings: Vec::new(),
			execution_force_cancellation: CancellationToken::new(),
			execute_program: Box::new(|_, _, _, _, _| {
				Box::pin(async { Err(report!(ErrorMarker::vfs_failed()).context(ExecuteProgramError)) })
					as PortFuture<_, _>
			}),
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
			get_setting: GetSettingDependencies { report_progress: None },
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
			install_archive,
			export_environment: unavailable_export_dependencies(),
			list_effective_conflicts: unavailable_list_effective_conflicts_dependencies(),
			inspect_mod_conflicts: unavailable_inspect_mod_conflicts_dependencies(),
			explain_path: unavailable_explain_path_dependencies(),
		}
	}

	fn failed<T: Send + 'static>() -> PortFuture<T> {
		Box::pin(async { Err(report!(ErrorMarker::io_failure())) })
	}

	fn unavailable_export_dependencies() -> ExportEnvironmentDependencies {
		ExportEnvironmentDependencies {
			validate_export_destination: Arc::new(|_| failed()),
			prepare_environment_plan: Arc::new(|_| failed()),
			project_profile: Arc::new(|_: &EnvironmentPlan| failed()),
			stage_profile: Arc::new(|_: &EnvironmentPlan, _, _| failed()),
			list_export_files: Arc::new(|_: &EnvironmentPlan, _: &StagedProfile, _, _| failed()),
			write_export: Arc::new(|_, _, _, _| failed()),
			set_load_order_times: Arc::new(|_: &EnvironmentPlan, _, _| failed()),
			discard_staged_profile: Arc::new(|_| failed()),
		}
	}

	/// Export ports that list one profile file and record whether it was written.
	fn export_dependencies(
		expected_output: PathBuf,
		include_saves: bool,
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
						warnings: vec![ProfileWarning::Unlisted {
							plugin: "Unlisted.esp".into(),
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
			list_export_files: Arc::new(move |_: &EnvironmentPlan, _: &StagedProfile, saves, _| {
				assert_eq!(saves, include_saves);
				Box::pin(async {
					Ok(ExportListing {
						files: vec![ExportFile {
							source_id: 0,
							path: DataRelativePath::new("profile/Fallout.ini".to_owned())
								.map_err(|_| report!(ErrorMarker::invalid_data_path()))?,
							provider: ExportProvider::Profile,
							bytes: 17,
						}],
						sources: ExportSources(AdapterState::new(())),
					})
				})
			}),
			write_export: Arc::new(move |_, _, _, _| {
				written.store(true, Ordering::SeqCst);
				Box::pin(async { Ok(()) })
			}),
			set_load_order_times: Arc::new(|_: &EnvironmentPlan, _, _| Box::pin(async { Ok(()) })),
			discard_staged_profile: Arc::new(|_| Box::pin(async { Ok(()) })),
		}
	}

	fn successful_dependencies(root: &Path) -> Result<Dependencies, ErrorMarker> {
		let game = GameInstallationPath::new(root.join("game"))
			.map_err(|_| ErrorMarker::environment_invalid(None))?;
		let binding = GameBinding::new(game);
		Ok(dependencies_with_list(
			binding,
			ListSettingsDependencies { report_progress: None },
		))
	}

	#[tokio::test]
	async fn help_and_version_never_construct_or_load_command_resources() {
		for argument in ["--help", "--version"] {
			let called = AtomicBool::new(false);
			let result = super::run_current_process(arguments!["mods", argument], async |_, _, _| {
				called.store(true, Ordering::SeqCst);
				Err(report!(ErrorMarker::environment_invalid(None)))
			})
			.await;
			assert!(result.is_err());
			assert!(!called.load(Ordering::SeqCst));
		}
	}

	#[tokio::test]
	async fn initialization_composition_does_not_need_an_existing_manifest() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		let cli = Cli::try_parse_from(arguments!["mods", "--log-level", "off", "init"])?;
		let result = super::execute(
			cli,
			temp.path().to_owned(),
			Some(temp.path().to_owned()),
			async move |_, command| {
				assert!(matches!(command, super::Command::Init { .. }));
				Ok(super::CommandDependencies::Initialize(
					dependencies.initialize_environment,
				))
			},
		)
		.await;
		assert_eq!(result.status, 0);
		assert!(!temp.path().join("mods.toml").exists());
		Ok(())
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
			let result = super::execute(
				cli,
				temp.path().to_owned(),
				Some(temp.path().to_owned()),
				async |_, _| Err(report!(std::io::Error::other("private source path")).context(code)),
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
		let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		dependencies.execute_program = Box::new(|_, _, _, _, _| {
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
			|_| Ok(dependencies),
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
		for (dry_run, include_saves) in [(true, true), (false, false)] {
			let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
			dependencies.export_environment =
				export_dependencies(temp.path().join("payload"), include_saves, published.clone());
			let arguments = if dry_run {
				arguments![
					"mods",
					"--log-level",
					"off",
					"export",
					"payload",
					"--include-saves",
					"--dry-run"
				]
			} else {
				arguments!["mods", "--log-level", "off", "export", "payload"]
			};
			let outcome = run(arguments, temp.path().to_owned(), Some(temp.path().to_owned()), |_| {
				Ok(dependencies)
			})
			.await?;
			assert_eq!(outcome.status, 0);
			assert!(outcome.stderr.starts_with("warning [unlisted_plugin]"));
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
	async fn exec_dispatch_preserves_child_status_and_does_not_capture_streams() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		for status in [0, 1, 125, 126, 127, 256, 259, 0xC000_0005] {
			let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
			dependencies.execute_program = Box::new(move |target, cwd, program, arguments, _| {
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
				|_| Ok(dependencies),
			)
			.await?;
			assert_eq!(outcome.status, status);
			assert!(outcome.stdout.is_empty());
			assert!(outcome.stderr.is_empty());
		}
		Ok(())
	}

	#[tokio::test]
	async fn exec_qualifies_projection_warnings_without_changing_child_output() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		dependencies.execute_program = Box::new(|_, _, _, _, _| {
			Box::pin(async {
				Ok(ExecuteProgramOutput {
					status: ProcessStatus::new(259),
					warnings: vec![
						ExecutionWarning::Plugin(PluginWarning::StalePluginEntry {
							name: "Missing.esp".into(),
						}),
						ExecutionWarning::Plugin(PluginWarning::StaleLoadOrderEntry {
							name: "Ordered.esp".into(),
						}),
						ExecutionWarning::Plugin(PluginWarning::DuplicatePluginEntry {
							file: "plugins.txt".into(),
							name: "Duplicate.esp".into(),
						}),
						ExecutionWarning::Plugin(PluginWarning::UnlistedPlugin {
							name: "Unlisted.esp".into(),
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
			|_| Ok(dependencies),
		)
		.await?;

		assert_eq!(outcome.status, 259);
		assert!(outcome.stdout.is_empty());
		assert_eq!(
			outcome.stderr,
			"warning [stale_plugin_entry]: analysis projection: plugins.txt entry \"Missing.esp\" is absent from the analytical Data view; runtime availability is not established.\nwarning [stale_load_order_entry]: analysis projection: loadorder.txt entry \"Ordered.esp\" is absent from the analytical Data view; runtime availability is not established.\nwarning [duplicate_plugin_entry]: duplicate entry \"Duplicate.esp\" in \"plugins.txt\"; analysis projection uses the first occurrence; canonical file is unchanged.\nwarning [unlisted_plugin]: \"Unlisted.esp\" is absent from loadorder.txt; it gets a load-order time after the listed plugins, in current modification-time order.\nwarning [profile_state_invalid]: retained Profile State is invalid; correct it before the next execution\n"
		);
		Ok(())
	}

	#[tokio::test]
	async fn exec_rejects_reserved_output_target_before_calling_port() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let mut dependencies = successful_dependencies(temp.path()).map_err(|_| "fixture failed")?;
		let called = Arc::new(AtomicBool::new(false));
		let observed = called.clone();
		dependencies.execute_program = Box::new(move |_, _, _, _, _| {
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
}
