use crate::runner::RunOutcome;
use std::io::Result;
use std::io::Write;

// Required plan and choice output is part of a successful command result, so publication errors,
// including broken pipes, must override success rather than silently losing that output.
pub(crate) const OUTPUT_FAILURE_STATUS: i32 = 1;

pub(crate) fn publish(outcome: &RunOutcome, stdout: &mut impl Write, stderr: &mut impl Write) -> Result<()> {
	stdout.write_all(outcome.stdout.as_bytes())?;
	stderr.write_all(outcome.stderr.as_bytes())?;

	flush(stdout, stderr)
}

pub(crate) fn flush(stdout: &mut impl Write, stderr: &mut impl Write) -> Result<()> {
	stdout.flush()?;
	stderr.flush()
}

pub(crate) fn exit_status(command_status: i32, publication_result: &Result<()>) -> i32 {
	if publication_result.is_err() {
		return OUTPUT_FAILURE_STATUS;
	}
	command_status
}

#[cfg(test)]
mod tests {
	use super::exit_status;
	use super::publish;
	use crate::runner::RunOutcome;
	use std::io::Error;
	use std::io::ErrorKind;
	use std::io::Result;
	use std::io::Write;

	#[derive(Clone, Copy)]
	enum FailurePoint {
		Write,
		Flush,
	}

	struct FailingWriter {
		failure_point: FailurePoint,
	}

	impl FailingWriter {
		fn at(failure_point: FailurePoint) -> Self {
			Self { failure_point }
		}
	}

	impl Write for FailingWriter {
		fn write(&mut self, buffer: &[u8]) -> Result<usize> {
			if matches!(self.failure_point, FailurePoint::Write) {
				return Err(Error::new(ErrorKind::BrokenPipe, "test writer rejected output"));
			}
			Ok(buffer.len())
		}

		fn flush(&mut self) -> Result<()> {
			if matches!(self.failure_point, FailurePoint::Flush) {
				return Err(Error::new(ErrorKind::BrokenPipe, "test writer rejected flush"));
			}
			Ok(())
		}
	}

	fn required_output() -> RunOutcome {
		RunOutcome {
			status: 0,
			stdout: "outcome = \"additional_selections_required\"\n".to_owned(),
			stderr: "warning: review required\n".to_owned(),
		}
	}

	#[test]
	fn stdout_write_failure_returns_error_and_requires_nonzero_exit() {
		let outcome = required_output();
		let mut stdout = FailingWriter::at(FailurePoint::Write);
		let mut stderr = Vec::new();

		let result = publish(&outcome, &mut stdout, &mut stderr);

		assert!(matches!(
			&result,
			Err(error) if error.kind() == ErrorKind::BrokenPipe
		));
		assert_ne!(exit_status(outcome.status as i32, &result), 0);
	}

	#[test]
	fn stderr_flush_failure_returns_error_and_requires_nonzero_exit() {
		let outcome = required_output();
		let mut stdout = Vec::new();
		let mut stderr = FailingWriter::at(FailurePoint::Flush);

		let result = publish(&outcome, &mut stdout, &mut stderr);

		assert!(matches!(
			&result,
			Err(error) if error.kind() == ErrorKind::BrokenPipe
		));
		assert_ne!(exit_status(outcome.status as i32, &result), 0);
	}
}
