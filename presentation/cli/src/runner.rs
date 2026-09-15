use crate::commands::Cli;
use crate::commands::Command;
use crate::commands::ConfigCommand;
use crate::commands::SetCommand;
use crate::commands::parse_from;
use crate::diagnostics::DiagnosticSession;
use crate::diagnostics::SINK_WARNING;
use crate::diagnostics::SessionStart;
use crate::error;
use crate::operation;
use crate::output;
use application::ErrorCode;
use application::ErrorMarker;
use application::environment::InitializeEnvironmentDependencies;
use application::environment::initialize_environment;
use application::settings::GetSettingDependencies;
use application::settings::ListSettingsDependencies;
use application::settings::SetGameDirectoryDependencies;
use application::settings::get_setting;
use application::settings::list_settings;
use application::settings::set_game_directory;
use clap::Error as ClapError;
use clap::error::ErrorKind;
use domain::EnvironmentRoot;
use domain::GameInstallationPath;
use rootcause::Report;
use std::env;
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct Dependencies {
	pub(crate) initialize_environment: InitializeEnvironmentDependencies,
	pub(crate) list_settings: ListSettingsDependencies,
	pub(crate) get_setting: GetSettingDependencies,
	pub(crate) set_game_directory: SetGameDirectoryDependencies,
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
	dependency_factory: impl FnOnce(&EnvironmentRoot) -> Result<Dependencies, ErrorMarker>,
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

	let operation_name = operation_name(&cli.command);
	let (mut session, diagnostic_warning) =
		match DiagnosticSession::start(root.as_path(), cli.log_level, operation_name) {
			SessionStart::FileBacked(session) => (Some(session), String::new()),
			SessionStart::Disabled => (None, String::new()),
			SessionStart::SetupFailed => (None, format!("{SINK_WARNING}\n")),
		};

	let session_id = session.as_ref().map(DiagnosticSession::id);
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
			return RunOutcome {
				status: 1,
				stdout: String::new(),
				stderr,
			};
		}
	};

	let work = dispatch(cli.command, dependencies, root, startup_directory);
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

async fn dispatch(command: Command, dependencies: Dependencies, root: EnvironmentRoot, startup: PathBuf) -> RunOutcome {
	match command {
		Command::Init { game_install } => {
			let Ok(game_install) = game_install
				.map(|path| resolve_path(&path, &startup))
				.map(GameInstallationPath::new)
				.transpose()
			else {
				return marker_outcome(ErrorMarker::game_install_invalid());
			};
			if let Some(path) = game_install.as_ref()
				&& let Ok(false) = path.as_path().try_exists()
			{
				return marker_outcome(ErrorMarker::game_install_not_found());
			}
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
					RunOutcome {
						status: 0,
						stdout,
						stderr,
					}
				}
				Err(report) => report_outcome(&report),
			}
		}
		Command::Config { command } => match command {
			ConfigCommand::List => match list_settings(dependencies.list_settings).await {
				Ok(output) => RunOutcome {
					status: 0,
					stdout: output::settings(&output.settings),
					stderr: String::new(),
				},
				Err(report) => report_outcome(&report),
			},
			ConfigCommand::Get { key } => match get_setting(dependencies.get_setting, key.into()).await {
				Ok(output) => RunOutcome {
					status: 0,
					stdout: output::setting(&output.setting),
					stderr: String::new(),
				},
				Err(report) => report_outcome(&report),
			},
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
		Command::Install(_) | Command::Conflicts { .. } | Command::Exec(_) => RunOutcome {
			status: 1,
			stdout: String::new(),
			stderr: "error: command is not implemented in this product slice\n".to_owned(),
		},
	}
}

fn resolve_path(path: &Path, startup: &Path) -> PathBuf {
	if path.is_absolute() || is_windows_absolute(path) {
		return path.to_path_buf();
	}

	let mut resolved = startup.to_path_buf();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				resolved.pop();
			}
			Component::Normal(name) => resolved.push(name),
			Component::Prefix(_) | Component::RootDir => return path.to_path_buf(),
		}
	}
	resolved
}

fn is_windows_absolute(path: &Path) -> bool {
	#[cfg(windows)]
	{
		path.is_absolute()
	}
	#[cfg(not(windows))]
	{
		let Some(value) = path.to_str() else {
			return false;
		};
		let bytes = value.as_bytes();
		(bytes.len() >= 3
			&& bytes[0].is_ascii_alphabetic()
			&& bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
			|| value.strip_prefix("\\\\")
				.is_some_and(|unc| unc.split(['\\', '/']).filter(|part| !part.is_empty()).count() >= 2)
	}
}

fn operation_name(command: &Command) -> &'static str {
	match command {
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
		Command::Conflicts { .. } => "conflicts",
		Command::Exec(_) => "exec",
	}
}

fn marker_outcome(marker: ErrorMarker) -> RunOutcome {
	let status = if marker.code() == ErrorCode::OperationCancelled {
		0xC000_013A
	} else {
		1
	};
	RunOutcome {
		status,
		stdout: String::new(),
		stderr: error::marker(&marker),
	}
}

fn report_outcome<E>(report: &Report<E>) -> RunOutcome {
	let status = if error::application_marker(report)
		.is_some_and(|marker| marker.code() == ErrorCode::OperationCancelled)
	{
		0xC000_013A
	} else {
		1
	};
	RunOutcome {
		status,
		stdout: String::new(),
		stderr: error::application_error(report),
	}
}

pub(crate) async fn run_current_process(
	arguments: impl IntoIterator<Item = OsString>,
	dependency_factory: impl FnOnce(&EnvironmentRoot) -> Result<Dependencies, ErrorMarker>,
) -> Result<RunOutcome, ClapError> {
	let cli = parse_from(arguments)?;
	let startup = env::current_dir().map_err(|error| ClapError::raw(ErrorKind::Io, error.to_string()))?;
	let local_app_data = env::var_os("LOCALAPPDATA").map(PathBuf::from);
	Ok(execute(cli, startup, local_app_data, dependency_factory).await)
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::Parser;
	use std::error::Error;
	use tempfile::TempDir;

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
}
