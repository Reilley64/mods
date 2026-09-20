use application::ErrorMarker;
#[cfg(not(windows))]
use encoding_rs::WINDOWS_1252;
use rootcause::Result;
#[cfg(windows)]
use rootcause::prelude::ResultExt;
use rootcause::report;
#[cfg(windows)]
use windows::Win32::Globalization::CP_UTF8;
#[cfg(windows)]
use windows::Win32::Globalization::GetACP;
#[cfg(windows)]
use windows::Win32::Globalization::MULTI_BYTE_TO_WIDE_CHAR_FLAGS;
#[cfg(windows)]
use windows::Win32::Globalization::MultiByteToWideChar;
#[cfg(windows)]
use windows::Win32::Globalization::WC_NO_BEST_FIT_CHARS;
#[cfg(windows)]
use windows::Win32::Globalization::WideCharToMultiByte;
#[cfg(windows)]
use windows::core::BOOL;
#[cfg(windows)]
use windows::core::Error as WindowsError;
#[cfg(windows)]
use windows::core::PCSTR;

#[cfg(not(windows))]
pub(crate) fn encode(text: &str) -> Result<Vec<u8>, ErrorMarker> {
	let (bytes, _, had_errors) = WINDOWS_1252.encode(text);
	if had_errors {
		Err(report!(ErrorMarker::environment_invalid(None)))
	} else {
		Ok(bytes.into_owned())
	}
}

#[cfg(windows)]
pub(crate) fn encode(text: &str) -> Result<Vec<u8>, ErrorMarker> {
	if text.is_empty() {
		return Ok(Vec::new());
	}

	let code_page = active_code_page()?;
	if code_page == CP_UTF8 {
		return Ok(text.as_bytes().to_vec());
	}
	let wide = text.encode_utf16().collect::<Vec<_>>();
	let mut used_default = BOOL(0);
	// SAFETY: `wide` is initialized and remains live for the size query. `code_page` came from
	// `active_code_page`, the default-character pointer is null as allowed, `used_default` is an exclusive
	// output borrow, and the API retains no pointers.
	let length = unsafe {
		WideCharToMultiByte(
			code_page,
			WC_NO_BEST_FIT_CHARS,
			&wide,
			None,
			PCSTR::null(),
			Some(&mut used_default),
		)
	};
	if length <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	if used_default.as_bool() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	let mut encoded = vec![0_u8; usize::try_from(length).context(ErrorMarker::environment_invalid(None))?];
	used_default = BOOL(0);
	// SAFETY: `encoded` has the exact queried length and is exclusively borrowed. `wide` and
	// `used_default` remain live and disjoint, all other arguments match the successful query, and the API
	// retains no pointers.
	let written = unsafe {
		WideCharToMultiByte(
			code_page,
			WC_NO_BEST_FIT_CHARS,
			&wide,
			Some(&mut encoded),
			PCSTR::null(),
			Some(&mut used_default),
		)
	};
	if written <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	if written != length || used_default.as_bool() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(encoded)
}

#[cfg(not(windows))]
pub(crate) fn decode(bytes: &[u8]) -> Result<String, ErrorMarker> {
	if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(WINDOWS_1252.decode(bytes).0.into_owned())
}

#[cfg(windows)]
pub(crate) fn decode(bytes: &[u8]) -> Result<String, ErrorMarker> {
	if bytes.is_empty() {
		return Ok(String::new());
	}

	let code_page = active_code_page()?;
	if code_page == CP_UTF8 {
		return String::from_utf8(bytes.to_vec()).context(ErrorMarker::environment_invalid(None));
	}
	// SAFETY: `bytes` is initialized and remains live for the call. `code_page` came from
	// `active_code_page`, zero is a valid flag set, None is the documented size query, and the API retains no
	// pointers.
	let length = unsafe { MultiByteToWideChar(code_page, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None) };
	if length <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	let mut wide = vec![0_u16; usize::try_from(length).context(ErrorMarker::environment_invalid(None))?];
	let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
	// SAFETY: `wide` has the exact queried length and is exclusively borrowed. Both slices remain live, all
	// other arguments match the successful query, and the API retains no pointers.
	let written = unsafe { MultiByteToWideChar(code_page, flags, bytes, Some(&mut wide)) };
	if written <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	if written != length {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	let text = String::from_utf16(&wide).context(ErrorMarker::environment_invalid(None))?;
	let mut used_default = BOOL(0);
	// SAFETY: `wide` contains initialized UTF-16 validated above and remains live. None is the documented
	// size query, `used_default` is exclusively borrowed, the default-character pointer is null as allowed,
	// and the API retains no pointers.
	let encoded_length = unsafe {
		WideCharToMultiByte(
			code_page,
			WC_NO_BEST_FIT_CHARS,
			&wide,
			None,
			PCSTR::null(),
			Some(&mut used_default),
		)
	};
	if encoded_length <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	if used_default.as_bool() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	let mut encoded = vec![0_u8; usize::try_from(encoded_length).context(ErrorMarker::environment_invalid(None))?];
	used_default = BOOL(0);
	// SAFETY: `encoded` has the exact queried length and is exclusively borrowed. `wide` and
	// `used_default` remain live and disjoint, all other arguments match the successful query, and the API
	// retains no pointers.
	let encoded_written = unsafe {
		WideCharToMultiByte(
			code_page,
			WC_NO_BEST_FIT_CHARS,
			&wide,
			Some(&mut encoded),
			PCSTR::null(),
			Some(&mut used_default),
		)
	};
	if encoded_written <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	if encoded_written != encoded_length || used_default.as_bool() || encoded != bytes {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(text)
}

#[cfg(windows)]
fn active_code_page() -> Result<u32, ErrorMarker> {
	// SAFETY: GetACP takes no arguments, accesses no caller-owned memory, and retains no pointers.
	let code_page = unsafe { GetACP() };
	if code_page == 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	Ok(code_page)
}
