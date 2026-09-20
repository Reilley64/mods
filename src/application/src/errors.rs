use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
	EnvironmentNotInitialized,
	EnvironmentAlreadyInitialized,
	EnvironmentRootNotEmpty,
	EnvironmentRootUnsafe,
	EnvironmentSchemaUnsupported,
	EnvironmentInvalid,
	EnvironmentPublicationFailed,
	ManualCleanupRequired,
	GameInstallNotFound,
	GameInstallInvalid,
	GameBuildMismatch,
	SettingUnknown,
	SettingReadOnly,
	SettingValueInvalid,
	InvalidSelection,
	UnsupportedInstaller,
	DependencyUnsatisfied,
	UnsafeArchive,
	AmbiguousInstallPlan,
	InvalidModName,
	InvalidDataPath,
	ModAlreadyExists,
	ModNotFound,
	IoFailure,
	TransactionFailure,
	OperationCancelled,
}

impl ErrorCode {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::EnvironmentNotInitialized => "environment_not_initialized",
			Self::EnvironmentAlreadyInitialized => "environment_already_initialized",
			Self::EnvironmentRootNotEmpty => "environment_root_not_empty",
			Self::EnvironmentRootUnsafe => "environment_root_unsafe",
			Self::EnvironmentSchemaUnsupported => "environment_schema_unsupported",
			Self::EnvironmentInvalid => "environment_invalid",
			Self::EnvironmentPublicationFailed => "environment_publication_failed",
			Self::ManualCleanupRequired => "manual_cleanup_required",
			Self::GameInstallNotFound => "game_install_not_found",
			Self::GameInstallInvalid => "game_install_invalid",
			Self::GameBuildMismatch => "game_build_mismatch",
			Self::SettingUnknown => "setting_unknown",
			Self::SettingReadOnly => "setting_read_only",
			Self::SettingValueInvalid => "setting_value_invalid",
			Self::InvalidSelection => "invalid_selection",
			Self::UnsupportedInstaller => "unsupported_installer",
			Self::DependencyUnsatisfied => "dependency_unsatisfied",
			Self::UnsafeArchive => "unsafe_archive",
			Self::AmbiguousInstallPlan => "ambiguous_install_plan",
			Self::InvalidModName => "invalid_mod_name",
			Self::InvalidDataPath => "invalid_data_path",
			Self::ModAlreadyExists => "mod_already_exists",
			Self::ModNotFound => "mod_not_found",
			Self::IoFailure => "io_failure",
			Self::TransactionFailure => "transaction_failure",
			Self::OperationCancelled => "operation_cancelled",
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectionDetails {
	group_id: Option<String>,
	option_id: Option<String>,
	supplied_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorMarker {
	code: ErrorCode,
	phase: Option<&'static str>,
	field: Option<&'static str>,
	setting_key: Option<&'static str>,
	expected_build_id: Option<u64>,
	actual_build_id: Option<u64>,
	message_override: Option<&'static str>,
	selection: Option<Box<SelectionDetails>>,
}

impl ErrorMarker {
	fn simple(code: ErrorCode) -> Self {
		Self {
			code,
			phase: None,
			field: None,
			setting_key: None,
			expected_build_id: None,
			actual_build_id: None,
			message_override: None,
			selection: None,
		}
	}

	pub fn environment_not_initialized() -> Self {
		Self::simple(ErrorCode::EnvironmentNotInitialized)
	}
	pub fn environment_already_initialized() -> Self {
		Self::simple(ErrorCode::EnvironmentAlreadyInitialized)
	}
	pub fn environment_root_not_empty() -> Self {
		Self::simple(ErrorCode::EnvironmentRootNotEmpty)
	}
	pub fn environment_root_unsafe() -> Self {
		Self::simple(ErrorCode::EnvironmentRootUnsafe)
	}
	pub fn environment_schema_unsupported() -> Self {
		Self::simple(ErrorCode::EnvironmentSchemaUnsupported)
	}
	pub fn environment_invalid(phase: Option<&'static str>) -> Self {
		Self {
			phase,
			..Self::simple(ErrorCode::EnvironmentInvalid)
		}
	}
	pub fn environment_publication_failed(phase: Option<&'static str>) -> Self {
		Self {
			phase,
			..Self::simple(ErrorCode::EnvironmentPublicationFailed)
		}
	}
	pub fn manual_cleanup_required() -> Self {
		Self::simple(ErrorCode::ManualCleanupRequired)
	}
	pub fn game_install_not_found() -> Self {
		Self {
			field: Some("game_dir"),
			..Self::simple(ErrorCode::GameInstallNotFound)
		}
	}
	pub fn game_install_invalid() -> Self {
		Self {
			field: Some("game_dir"),
			..Self::simple(ErrorCode::GameInstallInvalid)
		}
	}
	pub fn game_build_mismatch(expected: u64, actual: u64) -> Self {
		Self {
			code: ErrorCode::GameBuildMismatch,
			phase: None,
			field: None,
			setting_key: None,
			expected_build_id: Some(expected),
			actual_build_id: Some(actual),
			message_override: None,
			selection: None,
		}
	}
	pub fn setting_unknown() -> Self {
		Self {
			setting_key: Some("unknown"),
			..Self::simple(ErrorCode::SettingUnknown)
		}
	}
	pub fn setting_read_only() -> Self {
		Self {
			setting_key: Some("game_dir"),
			..Self::simple(ErrorCode::SettingReadOnly)
		}
	}
	pub fn setting_value_invalid() -> Self {
		Self {
			field: Some("game_dir"),
			setting_key: Some("game_dir"),
			..Self::simple(ErrorCode::SettingValueInvalid)
		}
	}
	pub fn invalid_selection(
		field: &'static str,
		group_id: Option<String>,
		option_id: Option<String>,
		supplied_sequence: Option<u64>,
	) -> Self {
		Self {
			field: Some(field),
			selection: Some(Box::new(SelectionDetails {
				group_id,
				option_id,
				supplied_sequence,
			})),
			..Self::simple(ErrorCode::InvalidSelection)
		}
	}
	pub fn unsupported_installer() -> Self {
		Self::simple(ErrorCode::UnsupportedInstaller)
	}
	pub fn dependency_unsatisfied() -> Self {
		Self::simple(ErrorCode::DependencyUnsatisfied)
	}
	pub fn unsafe_archive() -> Self {
		Self::simple(ErrorCode::UnsafeArchive)
	}
	pub fn ambiguous_install_plan() -> Self {
		Self::simple(ErrorCode::AmbiguousInstallPlan)
	}
	pub fn invalid_mod_name() -> Self {
		Self::simple(ErrorCode::InvalidModName)
	}
	pub fn invalid_data_path() -> Self {
		Self::simple(ErrorCode::InvalidDataPath)
	}
	pub fn mod_already_exists() -> Self {
		Self::simple(ErrorCode::ModAlreadyExists)
	}
	pub fn mod_not_found() -> Self {
		Self::simple(ErrorCode::ModNotFound)
	}
	pub fn io_failure() -> Self {
		Self::simple(ErrorCode::IoFailure)
	}
	pub fn transaction_failure() -> Self {
		Self::simple(ErrorCode::TransactionFailure)
	}
	pub fn operation_cancelled() -> Self {
		Self::simple(ErrorCode::OperationCancelled)
	}

	pub fn settings_environment_invalid() -> Self {
		Self {
			message_override: Some("environment variables are invalid; only MODS_GAME_DIR is accepted"),
			..Self::environment_invalid(None)
		}
	}
	pub fn initialization_environment_invalid() -> Self {
		Self {
			field: Some("game_dir"),
			message_override: Some("environment variables are invalid; only MODS_GAME_DIR is accepted"),
			..Self::game_install_invalid()
		}
	}

	pub fn code(&self) -> ErrorCode {
		self.code
	}
	pub fn phase(&self) -> Option<&'static str> {
		self.phase
	}
	pub fn field(&self) -> Option<&'static str> {
		self.field
	}
	pub fn setting_key(&self) -> Option<&'static str> {
		self.setting_key
	}
	pub fn group_id(&self) -> Option<&str> {
		self.selection.as_ref()?.group_id.as_deref()
	}
	pub fn option_id(&self) -> Option<&str> {
		self.selection.as_ref()?.option_id.as_deref()
	}
	pub fn supplied_sequence(&self) -> Option<u64> {
		self.selection.as_ref()?.supplied_sequence
	}
	pub fn build_ids(&self) -> Option<(u64, u64)> {
		let (Some(expected), Some(actual)) = (self.expected_build_id, self.actual_build_id) else {
			return None;
		};
		Some((expected, actual))
	}

	pub fn message(&self) -> &'static str {
		if let Some(message) = self.message_override {
			return message;
		}
		match self.code {
			ErrorCode::EnvironmentNotInitialized => "folder uninitialized",
			ErrorCode::EnvironmentAlreadyInitialized => "environment already initialized",
			ErrorCode::EnvironmentRootNotEmpty => "environment folder is not empty",
			ErrorCode::EnvironmentRootUnsafe => "environment folder is unsafe",
			ErrorCode::EnvironmentSchemaUnsupported => "environment schema is unsupported",
			ErrorCode::EnvironmentInvalid => "environment is invalid",
			ErrorCode::EnvironmentPublicationFailed => "environment publication failed",
			ErrorCode::ManualCleanupRequired => "unfinished operation requires manual cleanup",
			ErrorCode::GameInstallNotFound => "game installation was not found",
			ErrorCode::GameInstallInvalid => "game installation is invalid",
			ErrorCode::GameBuildMismatch => "game build does not match the recorded build",
			ErrorCode::SettingUnknown => "setting is unknown",
			ErrorCode::SettingReadOnly => "setting is read-only",
			ErrorCode::SettingValueInvalid => "setting value is invalid",
			ErrorCode::InvalidSelection => "FOMOD selection is invalid",
			ErrorCode::UnsupportedInstaller => "installer is unsupported",
			ErrorCode::DependencyUnsatisfied => "installer dependency is not satisfied",
			ErrorCode::UnsafeArchive => "archive is unsafe",
			ErrorCode::AmbiguousInstallPlan => "install plan is ambiguous",
			ErrorCode::InvalidModName => "mod name is invalid",
			ErrorCode::InvalidDataPath => "Data-relative path is invalid",
			ErrorCode::ModAlreadyExists => "mod already exists",
			ErrorCode::ModNotFound => "mod was not found",
			ErrorCode::IoFailure => "input/output operation failed",
			ErrorCode::TransactionFailure => "installation transaction failed",
			ErrorCode::OperationCancelled => "operation cancelled",
		}
	}
}

impl fmt::Display for ErrorMarker {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(self.message())
	}
}
