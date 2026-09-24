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
	#[arg(long, action = ArgAction::Append)]
	pub(crate) choice: Vec<String>,
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
	use super::Cli;
	use super::Command;
	use super::ConfigCommand;
	use super::ConflictsCommand;
	use super::LogLevel;
	use super::SettingKeyArgument;
	use super::parse_from;
	use std::error::Error;
	use std::ffi::OsString;

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
	fn install_preserves_choice_occurrence_order() -> Result<(), Box<dyn Error>> {
		let parsed = parse_from([
			"mods",
			"install",
			"archive.zip",
			"--choice",
			" group = option ",
			"--choice",
			"second=two",
		]);
		let Ok(Cli {
			command: Command::Install(arguments),
			..
		}) = parsed
		else {
			return Err("install command must parse".into());
		};
		assert_eq!(arguments.choice, [" group = option ", "second=two"]);
		Ok(())
	}

	#[test]
	fn parses_exact_conflict_commands_and_compare_content_defaults() -> Result<(), Box<dyn Error>> {
		let list = parse_from(["mods", "conflicts", "list"])?;
		assert!(matches!(
			list.command,
			Command::Conflicts {
				command: ConflictsCommand::List { compare_content: false }
			}
		));

		let compared_list = parse_from(["mods", "conflicts", "list", "--compare-content"])?;
		assert!(matches!(
			compared_list.command,
			Command::Conflicts {
				command: ConflictsCommand::List { compare_content: true }
			}
		));

		let default_inspect = parse_from(["mods", "conflicts", "inspect", "Mojave Textures"])?;
		assert!(matches!(
			default_inspect.command,
			Command::Conflicts {
				command: ConflictsCommand::Inspect {
					compare_content: false,
					..
				}
			}
		));

		let inspect = parse_from(["mods", "conflicts", "inspect", "Mojave Textures", "--compare-content"])?;
		assert!(matches!(
			inspect.command,
			Command::Conflicts {
				command: ConflictsCommand::Inspect {
					mod_name,
					compare_content: true
				}
			} if mod_name == "Mojave Textures"
		));

		let default_explain = parse_from(["mods", "conflicts", "explain", r"textures\weapons\rifle.dds"])?;
		assert!(matches!(
			default_explain.command,
			Command::Conflicts {
				command: ConflictsCommand::Explain {
					compare_content: false,
					..
				}
			}
		));

		let explain = parse_from([
			"mods",
			"conflicts",
			"explain",
			r"textures\weapons\rifle.dds",
			"--compare-content",
		])?;
		assert!(matches!(
			explain.command,
			Command::Conflicts {
				command: ConflictsCommand::Explain {
					path,
					compare_content: true
				}
			} if path == r"textures\weapons\rifle.dds"
		));
		Ok(())
	}

	#[test]
	fn conflict_commands_reject_unapproved_arguments() {
		assert!(parse_from(["mods", "conflicts", "list", "extra"]).is_err());
		assert!(parse_from(["mods", "conflicts", "inspect", "mod", "--filter", "x"]).is_err());
		assert!(parse_from(["mods", "conflicts", "resolve", "path"]).is_err());
	}

	#[test]
	fn rejects_json_and_unknown_setting_keys() {
		assert!(parse_from(["mods", "--json", "config", "list"]).is_err());
		assert!(parse_from(["mods", "config", "get", "unknown"]).is_err());
	}

	#[test]
	fn exec_preserves_every_child_value_and_requires_a_program() -> Result<(), Box<dyn Error>> {
		let values = [
			"tool.exe",
			"",
			" ",
			"a\"b",
			"tail\\",
			"雪",
			"--",
			"--environment",
			"child",
		];
		let parsed = parse_from(["mods", "exec", "--"].into_iter().chain(values))?;
		let Command::Exec(arguments) = parsed.command else {
			return Err("exec command must parse".into());
		};
		assert_eq!(arguments.command, values.map(OsString::from));
		assert!(parse_from(["mods", "exec", "--"]).is_err());
		Ok(())
	}

	#[test]
	fn keeps_later_commands_parse_compatible_and_exec_requires_separator() {
		assert!(parse_from(["mods", "install", "example.zip", "--dry-run"]).is_ok());
		assert!(parse_from(["mods", "exec", "tool.exe"]).is_err());
		assert!(parse_from(["mods", "exec", "--", "tool.exe", "--literal"]).is_ok());
	}
}
