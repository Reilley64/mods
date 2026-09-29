use std::fmt;
use std::path::PathBuf;

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
