/// Adapts canonical namespace paths because upstream usvfs disagrees internally
/// when parsing verbatim roots; canonical project state remains unchanged.
mod native_path;

use crate::ExecutionError;
use crate::NativeFailure;
use crate::PathMapping;
use crate::ViewConfiguration;
use crate::configuration::ConfigureView;
use ffi::LINKFLAG_CREATETARGET;
use ffi::LINKFLAG_RECURSIVE;
pub(crate) use ffi::MODS_CLEANUP_TIMEOUT_MS as CLEANUP_TIMEOUT_MS;
use ffi::ModsResult;
use ffi::ModsUsvfs;
pub(crate) use ffi::PROCESS_INFORMATION;
pub(crate) use ffi::STARTUPINFOW;
use ffi::mods_usvfs_clear_bypasses;
use ffi::mods_usvfs_close;
use ffi::mods_usvfs_launch;
use ffi::mods_usvfs_link_directory;
use ffi::mods_usvfs_link_file;
use ffi::mods_usvfs_open;
pub(crate) use native_path::native_path_wide;
use native_path::physical_path_wide;
use native_path::wide;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use std::env::current_exe;
use std::ffi::CString;
use std::ffi::OsStr;
use std::fs::read;
use std::path::Path;
use std::process::id;
use std::ptr::NonNull;
use std::ptr::null_mut;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use usvfs_sys as ffi;

include!(concat!(env!("OUT_DIR"), "/artifacts.rs"));

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The one upstream controller session in this process.
///
/// Load artifacts only from the package's `usvfs` directory beside its executable.
/// A failed native teardown poisons this process's session gate rather than
/// reconnecting to uncertain upstream state.
///
/// The view is `Send` but not `Sync`: it may move between threads, but upstream
/// calls must never overlap.
pub struct VirtualGameView {
	native: Option<NonNull<ModsUsvfs>>,
}

// SAFETY: Moving the session to another thread is sound; overlapping calls are
// not. In the pinned usvfs-rs revision `c23705c`, the controller exports that
// the shim calls (`usvfsVirtualLinkFile`, `usvfsVirtualLinkDirectoryStatic`,
// `usvfsCreateProcessHooked`, the clear functions, connect, and disconnect in
// `src/usvfs_dll/usvfs.cpp`) use the process-global `context` without a lock.
// `READ_CONTEXT`/`WRITE_CONTEXT` locking appears only in hook code for
// injected children. No controller state is thread-local: `src/` has no
// `thread_local`, `DllMain` ignores thread attach and detach, and the shim in
// `rust/usvfs-sys/native/barrier.cpp` keeps no thread or lock state. The
// caller's thread therefore does not matter, but calls must never overlap.
// This wrapper prevents overlap: `SESSION_ACTIVE` allows one session per
// process, `NonNull` keeps the view `!Sync`, and every native call takes
// `&mut self` or `self`. Moving the value transfers ownership, which also
// orders every call on the old thread before any call on the new one.
unsafe impl Send for VirtualGameView {}

impl VirtualGameView {
	/// Loads the exact packaged native artifacts and applies mods' mapping decisions.
	///
	/// # Errors
	///
	/// Returns input, artifact, concurrent-session, or native setup failures.
	pub fn configure(configuration: &ViewConfiguration) -> Result<Self, ExecutionError> {
		let executable = current_exe().context(ExecutionError)?;
		let parent = executable.parent().ok_or_else(|| report!(ExecutionError))?;
		let mut view = Self::load(&parent.join("usvfs"))?;

		if let Err(mut failure) = configuration.apply(&mut view) {
			if let Err(cleanup) = view.close() {
				failure.children_mut().push(cleanup.into_dynamic().into_cloneable());
			}
			return Err(failure);
		}

		Ok(view)
	}

	fn load(directory: &Path) -> Result<Self, ExecutionError> {
		if !directory.is_absolute() {
			return Err(report!(ExecutionError));
		}

		for (name, expected) in ARTIFACTS {
			let bytes = read(directory.join(name)).context(ExecutionError)?;
			let actual: [u8; 32] = Sha256::digest(bytes).into();
			if actual != *expected {
				return Err(report!(ExecutionError));
			}
		}

		let name = if cfg!(target_pointer_width = "64") {
			"usvfs_x64.dll"
		} else {
			"usvfs_x86.dll"
		};
		let library = wide(directory.join(name).as_os_str())?;
		let instance = CString::new(format!("mods-{}", id())).context(ExecutionError)?;

		if SESSION_ACTIVE
			.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
			.is_err()
		{
			return Err(report!(ExecutionError));
		}

		let mut native = null_mut();
		// SAFETY: checked terminated strings remain live; output is writable. The
		// process-wide gate excludes concurrent upstream sessions. The shim catches
		// C++ exceptions and owns all resources on failure.
		let result = unsafe { mods_usvfs_open(library.as_ptr(), instance.as_ptr(), &mut native) };
		if result.status != 0 {
			if result.cleanup_status == 0 {
				SESSION_ACTIVE.store(false, Ordering::Release);
			}
			check(result)?;
		}
		let native = NonNull::new(native).ok_or_else(|| report!(ExecutionError))?;
		Ok(Self { native: Some(native) })
	}

	pub(crate) fn launch_native(
		&mut self,
		application: &Path,
		command: &OsStr,
		directory: &Path,
		startup: &mut STARTUPINFOW,
		inherit: bool,
		new_process_group: bool,
	) -> Result<PROCESS_INFORMATION, ExecutionError> {
		if !application.is_absolute() || !directory.is_absolute() {
			return Err(report!(ExecutionError));
		}

		let application = physical_path_wide(application.as_os_str())?;
		let mut command = wide(command)?;
		let directory = physical_path_wide(directory.as_os_str())?;
		if command.len() > 32767 {
			return Err(report!(ExecutionError));
		}

		let native = self.native.ok_or_else(|| report!(ExecutionError))?;
		let mut output = PROCESS_INFORMATION::default();
		// SAFETY: this session exclusively owns the live controller. Buffers are
		// terminated and outlive the call; command is writable. Startup has the SDK
		// layout generated for this target. Only successful calls transfer handles.
		let result = unsafe {
			mods_usvfs_launch(
				native.as_ptr(),
				application.as_ptr(),
				command.as_mut_ptr(),
				directory.as_ptr(),
				startup,
				i32::from(inherit),
				i32::from(new_process_group),
				&mut output,
			)
		};
		if result.cleanup_status != 0 {
			// A failed native launch may leave a root alive. The shim already owns
			// handle cleanup; pin its session/library and keep the gate poisoned.
			self.native.take();
		}
		check(result)?;
		Ok(output)
	}

	/// Releases a controller that has not been transferred to a managed process.
	///
	/// # Errors
	/// Reports native teardown failure. A failed teardown prevents reconnecting in
	/// this controller process because upstream state may still reference its DLL.
	pub fn close(mut self) -> Result<(), ExecutionError> {
		self.release()
	}

	fn release(&mut self) -> Result<(), ExecutionError> {
		let Some(native) = self.native.take() else {
			return Ok(());
		};
		// SAFETY: pointer came from successful open and is consumed exactly once.
		// Process supervision retains this owner until its Job is empty.
		let result = unsafe { mods_usvfs_close(native.as_ptr()) };
		check(result)?;
		SESSION_ACTIVE.store(false, Ordering::Release);
		Ok(())
	}
}

impl Drop for VirtualGameView {
	fn drop(&mut self) {
		// Explicit close reports teardown errors. Drop still contains exceptions;
		// failed teardown leaves SESSION_ACTIVE set and pins native code in memory.
		let _ = self.release();
	}
}

impl ConfigureView for VirtualGameView {
	fn clear_bypasses(&mut self) -> Result<(), ExecutionError> {
		let native = self.native.ok_or_else(|| report!(ExecutionError))?;
		// SAFETY: exclusive live session, exception-contained call, no borrowed buffers.
		check(unsafe { mods_usvfs_clear_bypasses(native.as_ptr()) })
	}
	fn create_target(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError> {
		let source = physical_path_wide(mapping.source.as_os_str())?;
		let destination = native_path_wide(mapping.destination.as_os_str())?;
		let flags = LINKFLAG_CREATETARGET | if recursive { LINKFLAG_RECURSIVE } else { 0 };
		let native = self.native.ok_or_else(|| report!(ExecutionError))?;
		// SAFETY: exclusive live session and checked terminated buffers live for the
		// call. Flags come from the pinned upstream header, not handwritten ABI values.
		check(unsafe {
			mods_usvfs_link_directory(native.as_ptr(), source.as_ptr(), destination.as_ptr(), flags)
		})
	}
	fn link_directory(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError> {
		let source = physical_path_wide(mapping.source.as_os_str())?;
		let destination = native_path_wide(mapping.destination.as_os_str())?;
		let flags = if recursive { LINKFLAG_RECURSIVE } else { 0 };
		let native = self.native.ok_or_else(|| report!(ExecutionError))?;
		// SAFETY: exclusive live session; checked terminated buffers live through the
		// call. Flags come from the pinned header and never mark a creation target.
		check(unsafe {
			mods_usvfs_link_directory(native.as_ptr(), source.as_ptr(), destination.as_ptr(), flags)
		})
	}
	fn link_file(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError> {
		let source = physical_path_wide(mapping.source.as_os_str())?;
		let destination = native_path_wide(mapping.destination.as_os_str())?;
		let native = self.native.ok_or_else(|| report!(ExecutionError))?;
		// SAFETY: exclusive live session; checked terminated buffers remain live and
		// the native boundary does not retain their addresses.
		check(unsafe { mods_usvfs_link_file(native.as_ptr(), source.as_ptr(), destination.as_ptr()) })
	}
}

fn check(result: ModsResult) -> Result<(), ExecutionError> {
	if result.status == 0 {
		return Ok(());
	}
	Err(report!(NativeFailure {
		status: result.status,
		native_error: result.native_error,
		cleanup_status: result.cleanup_status,
		cleanup_error: result.cleanup_error
	})
	.context(ExecutionError))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::HookedProcess;
	use crate::LaunchRequest;
	use std::env::var_os;
	use std::fs;
	use std::path::PathBuf;
	use std::sync::Mutex;
	use std::sync::MutexGuard;
	use std::sync::PoisonError;
	use std::thread::sleep;
	use std::thread::spawn;
	use std::time::Duration;
	use std::time::Instant;
	use tempfile::TempDir;

	// Tests in one process share the upstream session gate, so they run one at a time.
	static SESSION_TESTS: Mutex<()> = Mutex::new(());

	fn exclusive_session() -> MutexGuard<'static, ()> {
		SESSION_TESTS.lock().unwrap_or_else(PoisonError::into_inner)
	}

	#[test]
	fn packaged_exports_obey_single_session_ownership() -> Result<(), ExecutionError> {
		let _session = exclusive_session();
		let directory = Path::new(env!("MODS_USVFS_ARTIFACTS"));
		let view = VirtualGameView::load(directory)?;
		assert!(VirtualGameView::load(directory).is_err());
		view.close()?;
		let view = VirtualGameView::load(directory)?;
		drop(view);
		assert!(!SESSION_ACTIVE.load(Ordering::Acquire));
		Ok(())
	}

	#[test]
	fn session_moves_between_threads_for_configure_launch_and_close() -> Result<(), ExecutionError> {
		let _session = exclusive_session();
		let temp = TempDir::new().context(ExecutionError)?;
		let root = temp.path().to_path_buf();
		let source = root.join("source");
		let mapped = root.join("virtual");
		fs::create_dir(&source).context(ExecutionError)?;
		fs::create_dir(&mapped).context(ExecutionError)?;
		fs::write(source.join("mapped.txt"), "mapped through usvfs").context(ExecutionError)?;
		let shell = PathBuf::from(var_os("ComSpec").ok_or_else(|| report!(ExecutionError))?);

		for cycle in 0..3 {
			let output = root.join(format!("output-{cycle}.txt"));

			let view = spawn({
				let mapping = PathMapping {
					source: source.clone(),
					destination: mapped.clone(),
				};
				move || -> Result<VirtualGameView, ExecutionError> {
					let mut view = VirtualGameView::load(Path::new(env!("MODS_USVFS_ARTIFACTS")))?;
					view.link_directory(&mapping, true)?;
					Ok(view)
				}
			})
			.join()
			.map_err(|_| report!(ExecutionError))??;

			let process = spawn({
				let command = format!(
					"cmd.exe /d /c type \"{}\" > \"{}\"",
					mapped.join("mapped.txt").display(),
					output.display()
				);
				let shell = shell.clone();
				let root = root.clone();
				move || -> Result<HookedProcess, ExecutionError> {
					let mut process = view.launch(LaunchRequest {
						new_process_group: false,
						application: &shell,
						command_line: OsStr::new(&command),
						directory: &root,
						standard_streams: None,
					})?;
					process.resume()?;

					let deadline = Instant::now() + Duration::from_secs(30);
					while !process.job_is_empty()? {
						if Instant::now() >= deadline {
							return Err(report!(ExecutionError));
						}
						sleep(Duration::from_millis(10));
					}

					assert_eq!(process.root_status()?, Some(0));
					Ok(process)
				}
			})
			.join()
			.map_err(|_| report!(ExecutionError))??;

			spawn(move || {
				let mut process = process;
				process.finish()
			})
			.join()
			.map_err(|_| report!(ExecutionError))??;

			assert!(!SESSION_ACTIVE.load(Ordering::Acquire));
			assert_eq!(
				fs::read_to_string(&output).context(ExecutionError)?,
				"mapped through usvfs"
			);
		}

		let view = VirtualGameView::load(Path::new(env!("MODS_USVFS_ARTIFACTS")))?;
		spawn(move || view.close())
			.join()
			.map_err(|_| report!(ExecutionError))??;

		assert!(!SESSION_ACTIVE.load(Ordering::Acquire));
		Ok(())
	}

	#[test]
	fn rejects_embedded_nul_before_native_calls() {
		assert!(wide(OsStr::new("bad\0path")).is_err());
		assert!(wide(OsStr::new("")).is_err());
	}

	#[test]
	fn native_failure_retains_original_and_cleanup_codes() {
		let result = check(ModsResult {
			status: 2,
			native_error: 5,
			cleanup_status: 1,
			cleanup_error: 6,
		});
		assert!(matches!(result, Err(report) if report.iter_reports().any(|report| {
			report.downcast_current_context::<NativeFailure>() == Some(&NativeFailure { status: 2, native_error: 5, cleanup_status: 1, cleanup_error: 6 })
		})));
	}
}
