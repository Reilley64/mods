use application::settings::SettingKey;
use clap::ArgAction;
use clap::Args;
use clap::Error as ClapError;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "mods", version, about = "Isolated Fallout: New Vegas mod environments")]
pub(crate) struct Cli {
	#[arg(long, global = true, value_name = "PATH")]
	pub(crate) environment: Option<PathBuf>,
	#[arg(long, global = true, value_enum, default_value_t = LogLevel::Info)]
	pub(crate) log_level: LogLevel,
	#[command(subcommand)]
	pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
	Init {
		#[arg(long, value_name = "PATH")]
		game_install: Option<PathBuf>,
	},
	Config {
		#[command(subcommand)]
		command: ConfigCommand,
	},
	Install(InstallArgs),
	Conflicts {
		#[command(subcommand)]
		command: ConflictsCommand,
	},
	Exec(ExecArgs),
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
	List,
	Get {
		#[arg(value_enum)]
		key: SettingKeyArgument,
	},
	Set {
		#[command(subcommand)]
		command: SetCommand,
	},
}

#[derive(Debug, Subcommand)]
pub(crate) enum SetCommand {
	GameDir { value: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SettingKeyArgument {
	SchemaVersion,
	Name,
	SteamAppId,
	GameDir,
	ObservedBuildId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum LogLevel {
	Trace,
	Debug,
	Info,
	Warn,
	Error,
	Off,
}
impl fmt::Display for LogLevel {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(match self {
			Self::Trace => "trace",
			Self::Debug => "debug",
			Self::Info => "info",
			Self::Warn => "warn",
			Self::Error => "error",
			Self::Off => "off",
		})
	}
}

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
	pub(crate) archive: PathBuf,
	#[arg(long)]
	pub(crate) name: Option<String>,
	#[arg(long)]
	pub(crate) replace: bool,
	#[arg(long, action = ArgAction::Append, conflicts_with = "choices")]
	pub(crate) choice: Vec<String>,
	#[arg(long, conflicts_with = "choice")]
	pub(crate) choices: Option<PathBuf>,
	#[arg(long)]
	pub(crate) dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConflictsCommand {
	List {
		#[arg(long)]
		compare_content: bool,
	},
	Inspect {
		mod_name: String,
		#[arg(long)]
		compare_content: bool,
	},
	Explain {
		path: String,
		#[arg(long)]
		compare_content: bool,
	},
}

#[derive(Debug, Args)]
pub(crate) struct ExecArgs {
	#[arg(long)]
	pub(crate) output_target: Option<String>,
	#[arg(long)]
	pub(crate) cwd: Option<PathBuf>,
	#[arg(last = true, required = true, num_args = 1.., allow_hyphen_values = true)]
	pub(crate) command: Vec<OsString>,
}

impl From<SettingKeyArgument> for SettingKey {
	fn from(value: SettingKeyArgument) -> Self {
		match value {
			SettingKeyArgument::SchemaVersion => Self::SchemaVersion,
			SettingKeyArgument::Name => Self::Name,
			SettingKeyArgument::SteamAppId => Self::SteamAppId,
			SettingKeyArgument::GameDir => Self::GameDir,
			SettingKeyArgument::ObservedBuildId => Self::ObservedBuildId,
		}
	}
}

pub(crate) fn parse_from<I, T>(arguments: I) -> Result<Cli, ClapError>
where
	I: IntoIterator<Item = T>,
	T: Into<OsString> + Clone,
{
	Cli::try_parse_from(arguments)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_environment_log_level_and_issue_twenty_seven_commands() {
		let parsed = parse_from([
			"mods",
			"--environment",
			"env",
			"--log-level",
			"off",
			"config",
			"get",
			"game-dir",
		]);
		assert!(matches!(
			parsed,
			Ok(Cli {
				environment: Some(_),
				log_level: LogLevel::Off,
				command: Command::Config {
					command: ConfigCommand::Get {
						key: SettingKeyArgument::GameDir
					}
				}
			})
		));
	}

	#[test]
	fn rejects_json_and_unknown_setting_keys() {
		assert!(parse_from(["mods", "--json", "config", "list"]).is_err());
		assert!(parse_from(["mods", "config", "get", "unknown"]).is_err());
	}

	#[test]
	fn keeps_later_commands_parse_compatible_and_exec_requires_separator() {
		assert!(parse_from(["mods", "install", "example.zip", "--dry-run"]).is_ok());
		assert!(parse_from(["mods", "exec", "tool.exe"]).is_err());
		assert!(parse_from(["mods", "exec", "--", "tool.exe", "--literal"]).is_ok());
	}
}
