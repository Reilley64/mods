use crate::execution::ExecuteProgramError;
use crate::execution::ExecuteProgramOutput;
use crate::ports::PortFuture;
use domain::OutputTarget;
use domain::Program;
use domain::ProgramArgument;
use domain::WorkingDirectory;
use std::fmt;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionWarning {
	LoadOrderNotEnforced,
	StalePluginEntry { name: String },
	StaleLoadOrderEntry { name: String },
	DuplicatePluginEntry { file: String, name: String },
	UnlistedPlugin { name: String },
	ProfileStateInvalid,
}

/// Temporary INIs remain available for manual inspection after uncertain
/// completion or failed preservation. Presentation may expose this typed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedExecutionInis {
	pub path: PathBuf,
}
impl fmt::Display for RetainedExecutionInis {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("execution INIs retained")
	}
}

/// Runs one composed exec use case. Composition chooses the thread it runs on.
pub type ExecuteProgram = Box<
	dyn FnOnce(
			OutputTarget,
			Option<WorkingDirectory>,
			Program,
			Vec<ProgramArgument>,
			CancellationToken,
		) -> PortFuture<ExecuteProgramOutput, ExecuteProgramError>
		+ Send,
>;
