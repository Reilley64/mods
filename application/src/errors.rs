use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
	EnvironmentNotInitialized,
	EnvironmentAlreadyInitialized,
	EnvironmentRootNotEmpty,
	EnvironmentRootUnsafe,
	EnvironmentSchemaUnsupported,
	EnvironmentInvalid,
	EnvironmentRecoveryFailed,
	GameInstallNotFound,
	GameInstallInvalid,
	GameBuildMismatch,
	SettingUnknown,
	SettingReadOnly,
	SettingValueInvalid,
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
			Self::EnvironmentRecoveryFailed => "environment_recovery_failed",
			Self::GameInstallNotFound => "game_install_not_found",
			Self::GameInstallInvalid => "game_install_invalid",
			Self::GameBuildMismatch => "game_build_mismatch",
			Self::SettingUnknown => "setting_unknown",
			Self::SettingReadOnly => "setting_read_only",
			Self::SettingValueInvalid => "setting_value_invalid",
			Self::OperationCancelled => "operation_cancelled",
		}
	}
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
}

impl ErrorMarker {
	const fn simple(code: ErrorCode) -> Self {
		Self {
			code,
			phase: None,
			field: None,
			setting_key: None,
			expected_build_id: None,
			actual_build_id: None,
			message_override: None,
		}
	}

	pub const fn environment_not_initialized() -> Self {
		Self::simple(ErrorCode::EnvironmentNotInitialized)
	}
	pub const fn environment_already_initialized() -> Self {
		Self::simple(ErrorCode::EnvironmentAlreadyInitialized)
	}
	pub const fn environment_root_not_empty() -> Self {
		Self::simple(ErrorCode::EnvironmentRootNotEmpty)
	}
	pub const fn environment_root_unsafe() -> Self {
		Self::simple(ErrorCode::EnvironmentRootUnsafe)
	}
	pub const fn environment_schema_unsupported() -> Self {
		Self::simple(ErrorCode::EnvironmentSchemaUnsupported)
	}
	pub const fn environment_invalid(phase: Option<&'static str>) -> Self {
		Self {
			phase,
			..Self::simple(ErrorCode::EnvironmentInvalid)
		}
	}
	pub const fn environment_recovery_failed(phase: Option<&'static str>) -> Self {
		Self {
			phase,
			..Self::simple(ErrorCode::EnvironmentRecoveryFailed)
		}
	}
	pub const fn game_install_not_found() -> Self {
		Self {
			field: Some("game_dir"),
			..Self::simple(ErrorCode::GameInstallNotFound)
		}
	}
	pub const fn game_install_invalid() -> Self {
		Self {
			field: Some("game_dir"),
			..Self::simple(ErrorCode::GameInstallInvalid)
		}
	}
	pub const fn game_build_mismatch(expected: u64, actual: u64) -> Self {
		Self {
			code: ErrorCode::GameBuildMismatch,
			phase: None,
			field: None,
			setting_key: None,
			expected_build_id: Some(expected),
			actual_build_id: Some(actual),
			message_override: None,
		}
	}
	pub const fn setting_unknown() -> Self {
		Self {
			setting_key: Some("unknown"),
			..Self::simple(ErrorCode::SettingUnknown)
		}
	}
	pub const fn setting_read_only() -> Self {
		Self {
			setting_key: Some("game_dir"),
			..Self::simple(ErrorCode::SettingReadOnly)
		}
	}
	pub const fn setting_value_invalid() -> Self {
		Self {
			field: Some("game_dir"),
			setting_key: Some("game_dir"),
			..Self::simple(ErrorCode::SettingValueInvalid)
		}
	}
	pub const fn operation_cancelled() -> Self {
		Self::simple(ErrorCode::OperationCancelled)
	}

	pub const fn settings_environment_invalid() -> Self {
		Self {
			message_override: Some("environment variables are invalid; only MODS_GAME_DIR is accepted"),
			..Self::environment_invalid(None)
		}
	}
	pub const fn initialization_environment_invalid() -> Self {
		Self {
			field: Some("game_dir"),
			message_override: Some("environment variables are invalid; only MODS_GAME_DIR is accepted"),
			..Self::game_install_invalid()
		}
	}

	pub const fn code(&self) -> ErrorCode {
		self.code
	}
	pub const fn phase(&self) -> Option<&'static str> {
		self.phase
	}
	pub const fn field(&self) -> Option<&'static str> {
		self.field
	}
	pub const fn setting_key(&self) -> Option<&'static str> {
		self.setting_key
	}
	pub const fn build_ids(&self) -> Option<(u64, u64)> {
		let (Some(expected), Some(actual)) = (self.expected_build_id, self.actual_build_id) else {
			return None;
		};
		Some((expected, actual))
	}

	pub const fn message(&self) -> &'static str {
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
			ErrorCode::EnvironmentRecoveryFailed => "environment recovery failed",
			ErrorCode::GameInstallNotFound => "game installation was not found",
			ErrorCode::GameInstallInvalid => "game installation is invalid",
			ErrorCode::GameBuildMismatch => "game build does not match the recorded build",
			ErrorCode::SettingUnknown => "setting is unknown",
			ErrorCode::SettingReadOnly => "setting is read-only",
			ErrorCode::SettingValueInvalid => "setting value is invalid",
			ErrorCode::OperationCancelled => "operation cancelled",
		}
	}
}

impl fmt::Display for ErrorMarker {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str(self.message())
	}
}
