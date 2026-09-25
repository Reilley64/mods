use crate::ExecutionError;
#[cfg(windows)]
use crate::HookedProcess;
use rootcause::Result;
use rootcause::report;
use std::time::Duration;
use tokio::time::Instant;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

pub trait ManagedProcess {
	/// Returns whether cancellation may wait for cooperative exit.
	/// CLI preserves its existing grace; MCP requires successful group delivery.
	fn request_cancellation_grace(&self) -> bool {
		true
	}
	fn resume(&mut self) -> Result<(), ExecutionError>;
	fn job_is_empty(&self) -> Result<bool, ExecutionError>;
	fn root_status(&self) -> Result<Option<u32>, ExecutionError>;
	fn terminate(&self, status: u32) -> Result<(), ExecutionError>;
	fn finish(&mut self) -> Result<(), ExecutionError>;
}

#[cfg(windows)]
impl ManagedProcess for HookedProcess {
	fn request_cancellation_grace(&self) -> bool {
		self.request_cancellation_grace()
	}
	fn resume(&mut self) -> Result<(), ExecutionError> {
		self.resume()
	}
	fn job_is_empty(&self) -> Result<bool, ExecutionError> {
		self.job_is_empty()
	}
	fn root_status(&self) -> Result<Option<u32>, ExecutionError> {
		self.root_status()
	}
	fn terminate(&self, status: u32) -> Result<(), ExecutionError> {
		self.terminate(status)
	}
	fn finish(&mut self) -> Result<(), ExecutionError> {
		self.finish()
	}
}

pub struct SupervisedExit {
	pub status: u32,
	pub forced: bool,
}

pub async fn supervise(
	process: &mut impl ManagedProcess,
	cancellation: CancellationToken,
	force: CancellationToken,
) -> Result<SupervisedExit, ExecutionError> {
	let result = async {
		process.resume()?;
		let mut deadline = None;
		let mut forced = false;
		loop {
			if process.job_is_empty()? {
				let status = process.root_status()?.ok_or_else(|| report!(ExecutionError))?;
				process.finish()?;
				return Ok(SupervisedExit {
					status: if forced { 0xc000013a } else { status },
					forced,
				});
			}
			if !forced {
				if cancellation.is_cancelled() && deadline.is_none() {
					deadline = Some(if process.request_cancellation_grace() {
						Instant::now() + Duration::from_secs(5)
					} else {
						Instant::now()
					});
				}
				if force.is_cancelled() || deadline.is_some_and(|deadline| Instant::now() >= deadline) {
					process.terminate(0xc000013a)?;
					forced = true;
				}
			}
			sleep(Duration::from_millis(10)).await;
		}
	}
	.await;
	let Err(mut failure) = result else {
		return result;
	};

	// Once execution has started, ordinary failure cleanup must retain the view
	// until the Job drains. Drop remains only the fallback if cleanup itself fails.
	if let Err(cleanup) = process.terminate(125) {
		failure.children_mut().push(cleanup.into_dynamic().into_cloneable());
		return Err(failure);
	}
	loop {
		match process.job_is_empty() {
			Ok(true) => break,
			Ok(false) => sleep(Duration::from_millis(10)).await,
			Err(cleanup) => {
				failure.children_mut().push(cleanup.into_dynamic().into_cloneable());
				return Err(failure);
			}
		}
	}
	if let Err(cleanup) = process.finish() {
		failure.children_mut().push(cleanup.into_dynamic().into_cloneable());
	}
	Err(failure)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::NativeFailure;
	use std::cell::Cell;
	use std::time::Duration;
	use tokio::time::Instant;

	struct Process {
		polls: Cell<usize>,
		drain_after: usize,
		status: u32,
		terminated: Cell<bool>,
		finished: bool,
		resume_failure: bool,
		termination_failure: bool,
		console_delivery: bool,
	}
	impl ManagedProcess for Process {
		fn request_cancellation_grace(&self) -> bool {
			self.console_delivery
		}
		fn resume(&mut self) -> Result<(), ExecutionError> {
			if self.resume_failure {
				return Err(report!(NativeFailure {
					status: 1,
					native_error: 5,
					cleanup_status: 0,
					cleanup_error: 0
				})
				.context(ExecutionError));
			}
			Ok(())
		}
		fn job_is_empty(&self) -> Result<bool, ExecutionError> {
			let polls = self.polls.get();
			self.polls.set(polls + 1);
			Ok(polls >= self.drain_after || self.terminated.get())
		}
		fn root_status(&self) -> Result<Option<u32>, ExecutionError> {
			Ok(Some(self.status))
		}
		fn terminate(&self, status: u32) -> Result<(), ExecutionError> {
			assert!(matches!(status, 125 | 0xc000013a));
			if self.termination_failure {
				return Err(report!(NativeFailure {
					status: 1,
					native_error: 6,
					cleanup_status: 0,
					cleanup_error: 0
				})
				.context(ExecutionError));
			}
			self.terminated.set(true);
			Ok(())
		}
		fn finish(&mut self) -> Result<(), ExecutionError> {
			self.finished = true;
			Ok(())
		}
	}
	fn process(status: u32, drain_after: usize) -> Process {
		Process {
			polls: Cell::new(0),
			drain_after,
			status,
			terminated: Cell::new(false),
			finished: false,
			resume_failure: false,
			termination_failure: false,
			console_delivery: true,
		}
	}

	#[tokio::test(start_paused = true)]
	async fn supervision_failure_terminates_and_drains_before_returning() {
		struct FailedQuery {
			queries: Cell<usize>,
			terminated: Cell<bool>,
			finished: bool,
		}
		impl ManagedProcess for FailedQuery {
			fn resume(&mut self) -> Result<(), ExecutionError> {
				Ok(())
			}
			fn job_is_empty(&self) -> Result<bool, ExecutionError> {
				let queries = self.queries.get();
				self.queries.set(queries + 1);
				if queries == 0 {
					return Err(report!(ExecutionError));
				}
				Ok(queries >= 3)
			}
			fn root_status(&self) -> Result<Option<u32>, ExecutionError> {
				Ok(Some(0))
			}
			fn terminate(&self, status: u32) -> Result<(), ExecutionError> {
				assert_eq!(status, 125);
				self.terminated.set(true);
				Ok(())
			}
			fn finish(&mut self) -> Result<(), ExecutionError> {
				self.finished = true;
				Ok(())
			}
		}
		let mut process = FailedQuery {
			queries: Cell::new(0),
			terminated: Cell::new(false),
			finished: false,
		};
		assert!(
			supervise(&mut process, CancellationToken::new(), CancellationToken::new())
				.await
				.is_err()
		);
		assert!(process.terminated.get());
		assert!(process.finished);
		assert_eq!(process.queries.get(), 4);
	}

	#[tokio::test(start_paused = true)]
	async fn failed_resume_drains_or_preserves_cleanup_failure_for_emergency_fallback() {
		for termination_failure in [false, true] {
			let mut process = process(0, usize::MAX);
			process.resume_failure = true;
			process.termination_failure = termination_failure;
			let result = supervise(&mut process, CancellationToken::new(), CancellationToken::new()).await;
			let Err(report) = result else {
				unreachable!("failed resume must fail execution")
			};
			let codes: Vec<_> = report
				.iter_reports()
				.filter_map(|cause| {
					cause.downcast_current_context::<NativeFailure>()
						.map(|failure| failure.native_error)
				})
				.collect();
			assert!(codes.contains(&5));
			assert_eq!(codes.contains(&6), termination_failure);
			assert_eq!(process.finished, !termination_failure);
			assert_eq!(process.terminated.get(), !termination_failure);
		}
	}

	#[tokio::test(start_paused = true)]
	async fn complete_job_drain_preserves_full_root_status() -> Result<(), ExecutionError> {
		let mut process = process(259, 3);
		let status = supervise(&mut process, CancellationToken::new(), CancellationToken::new()).await?;
		assert_eq!(status.status, 259);
		assert!(!status.forced);
		assert!(process.finished);
		assert_eq!(process.polls.get(), 4);
		Ok(())
	}
	#[tokio::test(start_paused = true)]
	async fn grace_period_keeps_job_alive_and_returns_actual_status() -> Result<(), ExecutionError> {
		let mut process = process(256, 3);
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let status = supervise(&mut process, cancellation, CancellationToken::new()).await?;
		assert_eq!(status.status, 256);
		assert!(!process.terminated.get());
		assert!(process.finished);
		Ok(())
	}
	#[tokio::test(start_paused = true)]
	async fn grace_timeout_terminates_then_drains() -> Result<(), ExecutionError> {
		let mut process = process(0, usize::MAX);
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let started = Instant::now();
		let status = supervise(&mut process, cancellation, CancellationToken::new()).await?;
		assert_eq!(status.status, 0xc000013a);
		assert!(started.elapsed() >= Duration::from_secs(5));
		assert!(process.terminated.get());
		assert!(process.finished);
		Ok(())
	}
	#[tokio::test(start_paused = true)]
	async fn unavailable_console_delivery_terminates_without_grace() -> Result<(), ExecutionError> {
		let mut process = process(0, usize::MAX);
		process.console_delivery = false;
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let started = Instant::now();

		let status = supervise(&mut process, cancellation, CancellationToken::new()).await?;

		assert_eq!(status.status, 0xc000013a);
		assert!(started.elapsed() < Duration::from_secs(5));
		assert!(process.finished);
		Ok(())
	}

	#[tokio::test(start_paused = true)]
	async fn force_cancellation_does_not_wait_for_grace() -> Result<(), ExecutionError> {
		let mut process = process(0, usize::MAX);
		let force = CancellationToken::new();
		force.cancel();
		let started = Instant::now();
		let status = supervise(&mut process, CancellationToken::new(), force).await?;
		assert_eq!(status.status, 0xc000013a);
		assert!(started.elapsed() < Duration::from_secs(5));
		assert!(process.finished);
		Ok(())
	}
}
