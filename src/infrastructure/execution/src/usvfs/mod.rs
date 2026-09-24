mod ffi;

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
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use std::env::current_exe;
use std::ffi::CString;
use std::ffi::OsStr;
use std::fs::read;
use std::marker::PhantomData;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::process::id;
use std::ptr::NonNull;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

include!(concat!(env!("OUT_DIR"), "/artifacts.rs"));

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The one upstream controller session in this process.
///
/// Load artifacts only from the package's `usvfs` directory beside its executable.
/// A failed native teardown poisons this process's session gate rather than
/// reconnecting to uncertain upstream state.
pub struct VirtualGameView {
	native: Option<NonNull<ModsUsvfs>>,
	// Upstream controller state is global; do not permit cross-thread access.
	_thread: PhantomData<Rc<()>>,
}

impl VirtualGameView {
	/// Loads the exact packaged native artifacts and applies mods' mapping decisions.
	///
	/// # Errors
	///
	/// Returns input, artifact, concurrent-session, or native setup failures.
	pub fn configure(configuration: &ViewConfiguration) -> Result<Self, ExecutionError> {
		let executable = current_exe().context(ExecutionError)?;
		let Some(parent) = executable.parent() else {
			return Err(report!(ExecutionError));
		};
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
		let Some(native) = NonNull::new(native) else {
			return Err(report!(ExecutionError));
		};
		Ok(Self {
			native: Some(native),
			_thread: PhantomData,
		})
	}

	pub(crate) fn launch_native(
		&mut self,
		application: &Path,
		command: &OsStr,
		directory: &Path,
		startup: &mut STARTUPINFOW,
		inherit: bool,
	) -> Result<PROCESS_INFORMATION, ExecutionError> {
		if !application.is_absolute() || !directory.is_absolute() {
			return Err(report!(ExecutionError));
		}
		let application = wide(application.as_os_str())?;
		let mut command = wide(command)?;
		let directory = wide(directory.as_os_str())?;
		if command.len() > 32767 {
			return Err(report!(ExecutionError));
		}
		let Some(native) = self.native else {
			return Err(report!(ExecutionError));
		};
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
		let Some(native) = self.native else {
			return Err(report!(ExecutionError));
		};
		// SAFETY: exclusive live session, exception-contained call, no borrowed buffers.
		check(unsafe { mods_usvfs_clear_bypasses(native.as_ptr()) })
	}
	fn create_target(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError> {
		let source = wide(mapping.source.as_os_str())?;
		let destination = wide(mapping.destination.as_os_str())?;
		let flags = LINKFLAG_CREATETARGET | if recursive { LINKFLAG_RECURSIVE } else { 0 };
		let Some(native) = self.native else {
			return Err(report!(ExecutionError));
		};
		// SAFETY: exclusive live session and checked terminated buffers live for the
		// call. Flags come from the pinned upstream header, not handwritten ABI values.
		check(unsafe {
			mods_usvfs_link_directory(native.as_ptr(), source.as_ptr(), destination.as_ptr(), flags)
		})
	}
	fn link_directory(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError> {
		let source = wide(mapping.source.as_os_str())?;
		let destination = wide(mapping.destination.as_os_str())?;
		let Some(native) = self.native else {
			return Err(report!(ExecutionError));
		};
		// SAFETY: exclusive live session; checked terminated buffers live through the
		// nonrecursive link call. No creation target or read files are changed by flags.
		check(unsafe { mods_usvfs_link_directory(native.as_ptr(), source.as_ptr(), destination.as_ptr(), 0) })
	}
	fn link_file(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError> {
		let source = wide(mapping.source.as_os_str())?;
		let destination = wide(mapping.destination.as_os_str())?;
		let Some(native) = self.native else {
			return Err(report!(ExecutionError));
		};
		// SAFETY: exclusive live session; checked terminated buffers remain live and
		// the native boundary does not retain their addresses.
		check(unsafe { mods_usvfs_link_file(native.as_ptr(), source.as_ptr(), destination.as_ptr()) })
	}
}

fn wide(value: &OsStr) -> Result<Vec<u16>, ExecutionError> {
	let mut value: Vec<u16> = value.encode_wide().collect();
	if value.is_empty() || value.contains(&0) {
		return Err(report!(ExecutionError));
	}
	value.push(0);
	Ok(value)
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

	#[test]
	fn packaged_exports_obey_single_session_ownership() -> Result<(), ExecutionError> {
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
