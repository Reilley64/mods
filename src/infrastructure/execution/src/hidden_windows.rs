use application::ErrorMarker;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::System::Console::FreeConsole;
use windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR;
use windows::Win32::UI::WindowsAndMessaging::MB_OK;
use windows::Win32::UI::WindowsAndMessaging::MessageBoxW;
use windows::core::Error as WindowsError;
use windows::core::PCWSTR;
use windows::core::w;

/// Detaches only the launcher, leaving shared caller terminals visible.
/// An exclusively owned console closes after its last client disconnects.
///
/// # Errors
///
/// Returns a supervision failure if Windows cannot detach the console.
/// Already-detached processes succeed under the Windows API contract.
pub fn detach_console() -> Result<(), ErrorMarker> {
	// SAFETY: FreeConsole detaches only this process and takes no pointers or handles.
	unsafe { FreeConsole() }.context(ErrorMarker::execution_supervision_failed())
}

/// Waits for the user to dismiss a modal error dialog without an owner window.
///
/// # Errors
///
/// Returns a supervision failure if Windows cannot display the modal error dialog.
pub fn show_error(message: &str) -> Result<(), ErrorMarker> {
	let text: Vec<u16> = OsStr::new(message).encode_wide().chain([0]).collect();

	// SAFETY: both UTF-16 strings are terminated and remain live through this synchronous
	// call. No owner window or borrowed window handle is supplied.
	let response = unsafe {
		MessageBoxW(
			None,
			PCWSTR(text.as_ptr()),
			w!("mods hidden execution failed"),
			MB_OK | MB_ICONERROR,
		)
	};
	if response.0 == 0 {
		return Err(WindowsError::from_thread()).context(ErrorMarker::execution_supervision_failed());
	}

	Ok(())
}
