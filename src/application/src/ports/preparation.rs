use crate::ports::AdapterState;
use crate::ports::PortFuture;
use domain::ProviderIdentity;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentProvider {
	pub identity: ProviderIdentity,
	pub enabled: bool,
}

/// Providers, analytical winners, and canonical profile inputs read from one environment.
pub struct EnvironmentPlan {
	pub providers: Vec<EnvironmentProvider>,
	pub state: AdapterState,
}

/// An advisory analytical plugin-projection diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileWarning {
	Unavailable { file: String, plugin: String },
	Duplicate { file: String, plugin: String },
	Unlisted { plugin: String },
}

/// The validated analytical plugin projection of the canonical profile.
///
/// Only its diagnostics cross the port. The adapter logs the projected order.
pub struct ProfileProjection {
	pub warnings: Vec<ProfileWarning>,
}

/// Selects how staged profile INIs route saves and archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfilePurpose {
	/// Copies for a program that runs inside the virtual file system.
	Execution,
	/// Copies for a standalone exported game layout.
	Export,
}

/// Which copies of the Data-root plugins and archives get load-order times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOrderTarget {
	/// The winning files in the game's Data folder, the Data Mods, and Overwrite.
	/// The virtual file system shows each file with its own time.
	Sources,
	/// The copies in this export output folder. The game's own Data files are not there.
	Export(PathBuf),
}

/// Derived profile INIs in a temporary directory. Dropping the handle keeps
/// `directory` on disk.
pub struct StagedProfile {
	pub directory: PathBuf,
	pub state: AdapterState,
}

/// Report attachment for a staged profile directory kept on disk after a
/// failure. Each command finds it by type and prints its own recovery advice;
/// it never formats the raw report tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedProfile {
	pub path: PathBuf,
}
impl fmt::Display for RetainedProfile {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("staged profile retained")
	}
}

pub type PrepareEnvironmentPlan = Arc<dyn Fn(CancellationToken) -> PortFuture<EnvironmentPlan> + Send + Sync>;
pub type ProjectProfile = Arc<dyn Fn(&EnvironmentPlan) -> PortFuture<ProfileProjection> + Send + Sync>;
pub type StageProfile =
	Arc<dyn Fn(&EnvironmentPlan, ProfilePurpose, CancellationToken) -> PortFuture<StagedProfile> + Send + Sync>;
/// Removes a staged profile that holds no edits to keep.
pub type DiscardStagedProfile = Arc<dyn Fn(StagedProfile) -> PortFuture<()> + Send + Sync>;
/// Gives the Data-root plugins and archives of the plan modification times in
/// load order, because the game orders them by time.
pub type SetLoadOrderTimes =
	Arc<dyn Fn(&EnvironmentPlan, LoadOrderTarget, CancellationToken) -> PortFuture<()> + Send + Sync>;
