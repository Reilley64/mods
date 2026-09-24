use rootcause::Result;
use rootcause::report;
use std::error::Error;
use std::fmt;
use std::iter::repeat_n;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchInputError {
	NotFound,
	InvalidTarget,
	InvalidDirectory,
	InvalidString,
	CommandLineTooLong,
	StandardStreams,
	Snapshot,
}

impl fmt::Display for LaunchInputError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(formatter, "launch input failure: {self:?}")
	}
}
impl Error for LaunchInputError {}

// The upstream API takes an already encoded command line, not an argument vector.
// Encode UTF-16 directly so unpaired Windows surrogates are never replaced.
fn encode_command_line(arguments: &[Vec<u16>]) -> Result<Vec<u16>, LaunchInputError> {
	let mut output = Vec::new();
	for (index, argument) in arguments.iter().enumerate() {
		if argument.contains(&0) {
			return Err(report!(LaunchInputError::InvalidString));
		}
		if index != 0 {
			output.push(32);
		}
		output.push(34);
		let mut backslashes = 0;
		for &unit in argument {
			if unit == 92 {
				backslashes += 1;
				continue;
			}
			let count = if unit == 34 { backslashes * 2 + 1 } else { backslashes };
			output.extend(repeat_n(92, count));
			output.push(unit);
			backslashes = 0;
		}
		output.extend(repeat_n(92, backslashes * 2));
		output.push(34);
	}
	if output.len() >= 32767 {
		return Err(report!(LaunchInputError::CommandLineTooLong));
	}
	Ok(output)
}

#[cfg(windows)]
#[path = "launch_inputs/windows_inputs.rs"]
mod windows_inputs;
#[cfg(windows)]
pub use windows_inputs::CallerSnapshot;
#[cfg(windows)]
pub use windows_inputs::InheritedStreams;

#[cfg(test)]
mod tests {
	use super::encode_command_line;

	#[test]
	fn preserves_empty_whitespace_quotes_trailing_slashes_and_surrogates() {
		let arguments = vec![vec![], vec![32], vec![92, 34], vec![97, 92], vec![0xd800]];
		let encoded = encode_command_line(&arguments).unwrap_or_default();
		assert_eq!(
			encoded,
			vec![
				34, 34, 32, 34, 32, 34, 32, 34, 92, 92, 92, 34, 34, 32, 34, 97, 92, 92, 34, 32, 34,
				0xd800, 34
			]
		);
	}

	#[test]
	fn rejects_nul_and_limit_including_terminator() {
		assert!(encode_command_line(&[vec![0]]).is_err());
		assert!(encode_command_line(&[vec![97; 32764]]).is_ok());
		assert!(encode_command_line(&[vec![97; 32765]]).is_err());
	}
}
