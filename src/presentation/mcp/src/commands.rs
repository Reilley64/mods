use clap::Parser;
use clap::ValueEnum;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "mods-mcp", version, about = "Local stdio mod-management server")]
pub(crate) struct Cli {
	#[arg(long, value_name = "PATH")]
	pub(crate) environment: Option<PathBuf>,
	#[arg(long, value_enum, default_value = "info")]
	pub(crate) log_level: LogLevel,
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
