use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use rootcause::Result;
use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutFailure {
	Unsupported,
	InvalidName,
	InvalidDestination,
	InvalidLaunch,
	ArgumentsTooLong,
	Publication,
}
impl fmt::Display for ShortcutFailure {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "shortcut failure: {self:?}")
	}
}

#[derive(Debug, Clone)]
pub struct ValidatedShortcutLaunch {
	pub launcher: PathBuf,
	pub environment: PathBuf,
	pub program: PathBuf,
	pub working_directory: PathBuf,
	pub environment_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ShortcutDefinition {
	pub launcher: PathBuf,
	pub arguments: Vec<OsString>,
	pub working_directory: PathBuf,
	pub icon: PathBuf,
	pub name: String,
	pub destination: Option<PathBuf>,
}

pub type ValidateShortcutLaunch = Arc<
	dyn Fn(
			OutputTarget,
			Option<WorkingDirectory>,
			Program,
			Vec<ProgramArgument>,
		) -> Pin<Box<dyn Future<Output = Result<ValidatedShortcutLaunch, ShortcutFailure>> + Send>>
		+ Send
		+ Sync,
>;
pub type PersistShortcut = Arc<
	dyn Fn(ShortcutDefinition) -> Pin<Box<dyn Future<Output = Result<(), ShortcutFailure>> + Send>> + Send + Sync,
>;
