use crate::ErrorMarker;
use crate::ports::PortFuture;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ShortcutDefinition {
	pub launcher: PathBuf,
	pub arguments: Vec<OsString>,
	pub working_directory: PathBuf,
	pub icon: PathBuf,
	pub name: String,
	pub destination: Option<PathBuf>,
}

pub type LocateLauncher = Arc<dyn Fn() -> PortFuture<PathBuf, ErrorMarker> + Send + Sync>;
pub type LocateEnvironmentRoot = Arc<dyn Fn() -> PortFuture<PathBuf, ErrorMarker> + Send + Sync>;
pub type PersistShortcut = Arc<dyn Fn(ShortcutDefinition) -> PortFuture<(), ErrorMarker> + Send + Sync>;
