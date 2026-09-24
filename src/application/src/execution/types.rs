#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionWarning {
	LoadOrderNotEnforced,
	StalePluginEntry { name: String },
	StaleLoadOrderEntry { name: String },
	DuplicatePluginEntry { file: String, name: String },
	UnlistedPlugin { name: String },
	ProfileStateInvalid,
}
