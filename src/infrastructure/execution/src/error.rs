use std::error::Error;
use std::fmt;

/// The execution adapter could not configure or launch the upstream view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionError;

impl fmt::Display for ExecutionError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("upstream execution failed")
	}
}
impl Error for ExecutionError {}

/// A bounded failure at the native exception boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeFailure {
	pub status: u32,
	pub native_error: u32,
	pub cleanup_status: u32,
	pub cleanup_error: u32,
}

impl fmt::Display for NativeFailure {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			formatter,
			"native status {}; Windows error {}; cleanup status {}; cleanup error {}",
			self.status, self.native_error, self.cleanup_status, self.cleanup_error
		)
	}
}
impl Error for NativeFailure {}
