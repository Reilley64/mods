use crate::ports::PortFuture;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
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
pub struct ShortcutDefinition {
	pub launcher: PathBuf,
	pub arguments: Vec<OsString>,
	pub working_directory: PathBuf,
	pub icon: PathBuf,
	pub name: String,
	pub destination: Option<PathBuf>,
}

pub type LocateLauncher = Arc<dyn Fn() -> PortFuture<PathBuf, ShortcutFailure> + Send + Sync>;
pub type LocateEnvironmentRoot = Arc<dyn Fn() -> PortFuture<PathBuf, ShortcutFailure> + Send + Sync>;
pub type PersistShortcut = Arc<dyn Fn(ShortcutDefinition) -> PortFuture<(), ShortcutFailure> + Send + Sync>;
