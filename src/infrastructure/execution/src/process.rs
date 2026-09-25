use crate::ExecutionError;
use crate::VirtualGameView;
use crate::usvfs::CLEANUP_TIMEOUT_MS;
use crate::usvfs::STARTUPINFOW;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::ffi::OsStr;
use std::io::Error as IoError;
use std::mem::forget;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::BorrowedHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;
use windows::Win32::Foundation::ERROR_TIMEOUT;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::WAIT_FAILED;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::System::Console::CTRL_BREAK_EVENT;
use windows::Win32::System::Console::GenerateConsoleCtrlEvent;
use windows::Win32::System::JobObjects::AssignProcessToJobObject;
use windows::Win32::System::JobObjects::CreateJobObjectW;
use windows::Win32::System::JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
use windows::Win32::System::JobObjects::JOBOBJECT_BASIC_ACCOUNTING_INFORMATION;
use windows::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
use windows::Win32::System::JobObjects::JobObjectBasicAccountingInformation;
use windows::Win32::System::JobObjects::JobObjectExtendedLimitInformation;
use windows::Win32::System::JobObjects::QueryInformationJobObject;
use windows::Win32::System::JobObjects::SetInformationJobObject;
use windows::Win32::System::JobObjects::TerminateJobObject;
use windows::Win32::System::Threading::GetExitCodeProcess;
use windows::Win32::System::Threading::ResumeThread;
use windows::Win32::System::Threading::STARTF_USESTDHANDLES;
use windows::Win32::System::Threading::TerminateProcess;
use windows::Win32::System::Threading::WaitForSingleObject;

/// Already-resolved Windows launch inputs. #31 owns command-line quoting,
/// resolution, stream policy, cancellation, and complete Job draining.
pub struct LaunchRequest<'a> {
	pub new_process_group: bool,
	pub application: &'a Path,
	pub command_line: &'a OsStr,
	pub directory: &'a Path,
	/// Inheritable handles chosen by composition, ordered stdin/stdout/stderr.
	pub standard_streams: Option<[BorrowedHandle<'a>; 3]>,
}

/// Owns the hooked root, its Job, and the upstream session until Job drain.
///
/// Normal cleanup is explicit through `finish`, which retains ownership on early
/// calls. Drop requests termination and waits at most five seconds for Job drain.
/// If drain cannot be confirmed, it pins native state until controller exit.
pub struct HookedProcess {
	view: Option<VirtualGameView>,
	job: Job,
	process: OwnedHandle,
	thread: OwnedHandle,
	resume_attempted: bool,
	cancellation_group: Option<u32>,
}

impl VirtualGameView {
	/// Creates a suspended hooked root and assigns it to a kill-on-close Job.
	/// Upstream injection occurs before the API returns, not after Job assignment.
	///
	/// # Errors
	///
	/// Returns invalid-input, hooked-launch, or Job setup failures. Failed roots
	/// are never resumed by this adapter.
	pub fn launch(mut self, request: LaunchRequest<'_>) -> Result<HookedProcess, ExecutionError> {
		// SAFETY: unnamed Job has no borrowed name or security descriptor.
		let job = unsafe { CreateJobObjectW(None, None) }.context(ExecutionError)?;
		// SAFETY: successful CreateJobObjectW transfers one valid owned handle.
		let job = unsafe { OwnedHandle::from_raw_handle(job.0) };
		let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
		limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
		// SAFETY: handle is live; buffer and size match the requested information class.
		unsafe {
			SetInformationJobObject(
				HANDLE(job.as_raw_handle()),
				JobObjectExtendedLimitInformation,
				&limits as *const _ as *const _,
				size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
			)
		}
		.context(ExecutionError)?;

		let mut startup = STARTUPINFOW {
			cb: size_of::<STARTUPINFOW>() as u32,
			..Default::default()
		};
		if let Some(streams) = request.standard_streams {
			startup.dwFlags = STARTF_USESTDHANDLES.0;
			startup.hStdInput = streams[0].as_raw_handle();
			startup.hStdOutput = streams[1].as_raw_handle();
			startup.hStdError = streams[2].as_raw_handle();
		}
		let created = self.launch_native(
			request.application,
			request.command_line,
			request.directory,
			&mut startup,
			request.standard_streams.is_some(),
			request.new_process_group,
		)?;
		// SAFETY: successful shim launch exclusively transfers the non-null process
		// and thread handles returned by CreateProcessW. Each enters one owner.
		let (process, thread) = unsafe {
			(
				OwnedHandle::from_raw_handle(created.hProcess),
				OwnedHandle::from_raw_handle(created.hThread),
			)
		};
		// SAFETY: both handles remain owned here; root was requested suspended.
		if let Err(error) = unsafe {
			AssignProcessToJobObject(HANDLE(job.as_raw_handle()), HANDLE(process.as_raw_handle()))
		} {
			let mut failure = report!(error).context(ExecutionError);
			// SAFETY: process is the still-suspended owned root, never resumed on this path.
			if let Err(cleanup) = unsafe { TerminateProcess(HANDLE(process.as_raw_handle()), 125) } {
				failure.children_mut()
					.push(report!(cleanup).into_dynamic().into_cloneable());
			}
			// SAFETY: the owned process handle remains live through the bounded wait.
			let waited =
				unsafe { WaitForSingleObject(HANDLE(process.as_raw_handle()), CLEANUP_TIMEOUT_MS) };
			if waited != WAIT_OBJECT_0 {
				let cleanup = if waited == WAIT_FAILED {
					IoError::last_os_error()
				} else {
					IoError::from_raw_os_error(ERROR_TIMEOUT.0 as i32)
				};
				failure.children_mut()
					.push(report!(cleanup).into_dynamic().into_cloneable());
				// Keep controller code/state live if termination could not be established.
				forget(self);
			}
			return Err(failure);
		}
		Ok(HookedProcess {
			view: Some(self),
			job: Job { handle: job },
			process,
			thread,
			resume_attempted: false,
			cancellation_group: request.new_process_group.then_some(created.dwProcessId),
		})
	}
}

impl HookedProcess {
	/// CLI retains its existing console grace. MCP only grants grace after the
	/// OS accepts delivery to the private child group; it never broadcasts.
	pub fn request_cancellation_grace(&self) -> bool {
		let Some(group) = self.cancellation_group else {
			return true;
		};
		if group == 0 {
			return false;
		}

		// SAFETY: the nonzero ID belongs to the owned root launched with
		// CREATE_NEW_PROCESS_GROUP. This cannot address the controller group.
		unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, group) }.is_ok()
	}

	/// Resumes the root once, after successful Job assignment.
	///
	/// # Errors
	/// Returns a native error if resuming fails or a resume was already attempted.
	pub fn resume(&mut self) -> Result<(), ExecutionError> {
		if self.resume_attempted {
			return Err(report!(ExecutionError));
		}
		self.resume_attempted = true;
		// SAFETY: thread is the owned suspended root thread; no other owner can resume it.
		if unsafe { ResumeThread(HANDLE(self.thread.as_raw_handle())) } == u32::MAX {
			return Err(report!(IoError::last_os_error()).context(ExecutionError));
		}
		Ok(())
	}

	/// Reports whether the complete owned Job has drained.
	///
	/// # Errors
	/// Preserves a native query failure. #31 chooses its waiting/cancellation strategy.
	pub fn job_is_empty(&self) -> Result<bool, ExecutionError> {
		self.job.is_empty()
	}

	/// Returns the root's full 32-bit status after it exits.
	///
	/// # Errors
	/// Preserves native status-query failures.
	pub fn root_status(&self) -> Result<Option<u32>, ExecutionError> {
		// SAFETY: owned process handle supports synchronization; a zero timeout does not block.
		let wait = unsafe { WaitForSingleObject(HANDLE(self.process.as_raw_handle()), 0) };
		if wait == WAIT_FAILED {
			return Err(report!(IoError::last_os_error()).context(ExecutionError));
		}
		if wait != WAIT_OBJECT_0 {
			return Ok(None);
		}
		let mut status = 0;
		// SAFETY: live process handle and writable status output.
		unsafe { GetExitCodeProcess(HANDLE(self.process.as_raw_handle()), &mut status) }
			.context(ExecutionError)?;
		Ok(Some(status))
	}

	/// Requests termination of the owned Job. Composition must still confirm complete drain.
	///
	/// # Errors
	/// Preserves a native Job termination failure.
	pub fn terminate(&self, status: u32) -> Result<(), ExecutionError> {
		self.job.request_termination(status)
	}

	/// Releases the upstream session after the Job is empty.
	///
	/// # Errors
	/// A live Job or query failure leaves this execution owned by the caller, so
	/// draining can continue. Native teardown errors are reported without retry.
	pub fn finish(&mut self) -> Result<(), ExecutionError> {
		finish_drained(&mut self.view, self.job.is_empty(), VirtualGameView::close)
	}
}

const EMERGENCY_DRAIN_TIMEOUT: Duration = Duration::from_millis(CLEANUP_TIMEOUT_MS as u64);

struct Job {
	handle: OwnedHandle,
}

// This seam owns cleanup decisions; tests fake the Job, not Windows API behavior.
trait JobDrain {
	fn is_empty(&self) -> Result<bool, ExecutionError>;
	fn request_termination(&self, status: u32) -> Result<(), ExecutionError>;
	fn wait_for_drain(&self, timeout: Duration) -> Result<bool, ExecutionError>;
}

impl JobDrain for Job {
	fn is_empty(&self) -> Result<bool, ExecutionError> {
		let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
		// SAFETY: live Job and writable buffer of the SDK-defined size/class.
		unsafe {
			QueryInformationJobObject(
				Some(HANDLE(self.handle.as_raw_handle())),
				JobObjectBasicAccountingInformation,
				&mut information as *mut _ as *mut _,
				size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
				None,
			)
		}
		.context(ExecutionError)?;
		Ok(information.ActiveProcesses == 0)
	}

	fn request_termination(&self, status: u32) -> Result<(), ExecutionError> {
		// SAFETY: this adapter exclusively owns the live Job handle.
		unsafe { TerminateJobObject(HANDLE(self.handle.as_raw_handle()), status) }.context(ExecutionError)
	}

	fn wait_for_drain(&self, timeout: Duration) -> Result<bool, ExecutionError> {
		let deadline = Instant::now() + timeout;
		loop {
			if self.is_empty()? {
				return Ok(true);
			}
			let remaining = deadline.saturating_duration_since(Instant::now());
			if remaining.is_zero() {
				return Ok(false);
			}
			// Drop is synchronous and cannot await Tokio. Poll only for this bounded
			// emergency path; #31 owns normal asynchronous waiting and cancellation.
			sleep(remaining.min(Duration::from_millis(10)));
		}
	}
}

fn finish_drained<Session>(
	view: &mut Option<Session>,
	drained: Result<bool, ExecutionError>,
	close: impl FnOnce(Session) -> Result<(), ExecutionError>,
) -> Result<(), ExecutionError> {
	if !drained? {
		return Err(report!(ExecutionError));
	}
	if let Some(view) = view.take() {
		close(view)?;
	}
	Ok(())
}

fn emergency_cleanup<Session>(
	view: &mut Option<Session>,
	job: &impl JobDrain,
	close: impl FnOnce(Session) -> Result<(), ExecutionError>,
) {
	if view.is_none() {
		return;
	}
	let drained = if job.is_empty().is_ok_and(|empty| empty) {
		true
	} else {
		let _ = job.request_termination(125);
		job.wait_for_drain(EMERGENCY_DRAIN_TIMEOUT).unwrap_or(false)
	};
	if drained {
		// Drop cannot return an error. Native close itself pins uncertain state;
		// callers that need cleanup errors must use explicit finish instead.
		let _ = finish_drained(view, Ok(true), close);
	} else if let Some(view) = view.take() {
		// Last resort only: never unload native state under possibly-live children.
		forget(view);
	}
}

impl Drop for HookedProcess {
	fn drop(&mut self) {
		emergency_cleanup(&mut self.view, &self.job, VirtualGameView::close);
	}
}

#[cfg(test)]
mod tests {
	use super::EMERGENCY_DRAIN_TIMEOUT;
	use super::JobDrain;
	use super::emergency_cleanup;
	use super::finish_drained;
	use crate::ExecutionError;
	use rootcause::Result;
	use rootcause::report;
	use std::cell::Cell;
	use std::cell::RefCell;
	use std::rc::Rc;
	use std::time::Duration;

	struct Session(Rc<Cell<usize>>);
	impl Drop for Session {
		fn drop(&mut self) {
			self.0.set(self.0.get() + 1);
		}
	}
	fn close(session: Session) -> Result<(), ExecutionError> {
		drop(session);
		Ok(())
	}

	#[derive(Debug, PartialEq, Eq)]
	enum Call {
		Query,
		Terminate(u32),
		Wait(Duration),
	}
	struct FakeJob {
		empty: bool,
		query_error: bool,
		termination_error: bool,
		drained: bool,
		wait_error: bool,
		calls: RefCell<Vec<Call>>,
	}
	impl JobDrain for FakeJob {
		fn is_empty(&self) -> Result<bool, ExecutionError> {
			self.calls.borrow_mut().push(Call::Query);
			if self.query_error {
				return Err(report!(ExecutionError));
			}
			Ok(self.empty)
		}
		fn request_termination(&self, status: u32) -> Result<(), ExecutionError> {
			self.calls.borrow_mut().push(Call::Terminate(status));
			if self.termination_error {
				return Err(report!(ExecutionError));
			}
			Ok(())
		}
		fn wait_for_drain(&self, timeout: Duration) -> Result<bool, ExecutionError> {
			self.calls.borrow_mut().push(Call::Wait(timeout));
			if self.wait_error {
				return Err(report!(ExecutionError));
			}
			Ok(self.drained)
		}
	}
	fn job(empty: bool, drained: bool) -> FakeJob {
		FakeJob {
			empty,
			drained,
			query_error: false,
			termination_error: false,
			wait_error: false,
			calls: RefCell::new(Vec::new()),
		}
	}

	#[test]
	fn early_finish_retains_ownership_until_normal_drain() -> Result<(), ExecutionError> {
		let released = Rc::new(Cell::new(0));
		let mut view = Some(Session(released.clone()));
		assert!(finish_drained(&mut view, Ok(false), close).is_err());
		assert!(view.is_some());
		assert_eq!(released.get(), 0);
		assert!(finish_drained(&mut view, Err(report!(ExecutionError)), close).is_err());
		assert!(view.is_some());
		assert_eq!(released.get(), 0);
		finish_drained(&mut view, Ok(true), close)?;
		assert!(view.is_none());
		assert_eq!(released.get(), 1);
		finish_drained(&mut view, Ok(true), close)?;
		assert_eq!(released.get(), 1);
		Ok(())
	}

	#[test]
	fn explicit_cleanup_errors_propagate_without_retry() {
		let released = Rc::new(Cell::new(0));
		let mut view = Some(Session(released.clone()));
		let result = finish_drained(&mut view, Ok(true), |_session| Err(report!(ExecutionError)));
		assert!(result.is_err());
		assert!(view.is_none());
		assert_eq!(released.get(), 1);
	}

	#[test]
	fn emergency_cleanup_closes_only_after_confirmed_drain() {
		let released = Rc::new(Cell::new(0));
		let mut view = Some(Session(released.clone()));
		let job = job(false, true);
		emergency_cleanup(&mut view, &job, close);
		assert_eq!(
			*job.calls.borrow(),
			vec![Call::Query, Call::Terminate(125), Call::Wait(EMERGENCY_DRAIN_TIMEOUT)]
		);
		assert_eq!(EMERGENCY_DRAIN_TIMEOUT, Duration::from_secs(5));
		assert_eq!(released.get(), 1);
		assert!(view.is_none());
	}

	#[test]
	fn emergency_cleanup_pins_only_when_drain_is_unconfirmed() {
		for wait_error in [false, true] {
			let released = Rc::new(Cell::new(0));
			let mut view = Some(Session(released.clone()));
			let mut job = job(false, false);
			job.query_error = true;
			job.termination_error = true;
			job.wait_error = wait_error;
			emergency_cleanup(&mut view, &job, close);
			assert_eq!(
				*job.calls.borrow(),
				vec![Call::Query, Call::Terminate(125), Call::Wait(EMERGENCY_DRAIN_TIMEOUT)]
			);
			assert_eq!(released.get(), 0);
			assert!(view.is_none());
		}
	}

	#[test]
	fn drained_drop_does_not_request_termination_or_wait() {
		let released = Rc::new(Cell::new(0));
		let mut view = Some(Session(released.clone()));
		let job = job(true, true);
		emergency_cleanup(&mut view, &job, close);
		assert_eq!(*job.calls.borrow(), vec![Call::Query]);
		assert_eq!(released.get(), 1);
	}
}
