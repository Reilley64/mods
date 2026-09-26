use crate::ExecutionError;
use rootcause::Result;
use rootcause::report;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

const BACKSLASH: u16 = 92;
const MAX_PATH_CHARACTERS: usize = 260;
const VERBATIM_PREFIX: [u16; 4] = [92, 92, 63, 92];
const VERBATIM_UNC_PREFIX: [u16; 8] = [92, 92, 63, 92, 85, 78, 67, 92];

pub(super) fn wide(value: &OsStr) -> Result<Vec<u16>, ExecutionError> {
	let mut value: Vec<u16> = value.encode_wide().collect();
	if value.is_empty() || value.contains(&0) {
		return Err(report!(ExecutionError));
	}

	value.push(0);
	Ok(value)
}

/// Encodes a virtual filesystem path using the spelling expected by upstream usvfs.
///
/// Upstream parses ordinary DOS roots consistently but disagrees internally on
/// verbatim roots. This conversion changes only the copy passed to native code.
///
/// # Errors
///
/// Returns [`ExecutionError`] when the path is empty, contains an embedded NUL, or contains a verbatim component whose meaning would change after conversion.
pub(super) fn native_path_wide(value: &OsStr) -> Result<Vec<u16>, ExecutionError> {
	normalize_native_path(wide(value)?)
}

/// Encodes a physical filesystem path without breaking long UNC traversal.
///
/// The pinned usvfs release prepends `\\?\` rather than `\\?\UNC\` to a
/// long ordinary UNC mapping source. Preserve that one valid namespace spelling
/// while applying the ordinary conversion to other physical paths.
///
/// # Errors
///
/// Returns [`ExecutionError`] when the path is empty, contains an embedded NUL, or contains a verbatim component whose meaning would change after conversion.
pub(super) fn physical_path_wide(value: &OsStr) -> Result<Vec<u16>, ExecutionError> {
	let value = wide(value)?;
	if value.starts_with(&VERBATIM_UNC_PREFIX) {
		let ordinary_characters = value.len() - 1 - (VERBATIM_UNC_PREFIX.len() - 2);
		if ordinary_characters >= MAX_PATH_CHARACTERS {
			return Ok(value);
		}
	}

	normalize_native_path(value)
}

fn normalize_native_path(value: Vec<u16>) -> Result<Vec<u16>, ExecutionError> {
	if !value.starts_with(&VERBATIM_PREFIX) {
		return Ok(value);
	}

	if value.starts_with(&VERBATIM_UNC_PREFIX) {
		if has_conversion_sensitive_component(&value[VERBATIM_UNC_PREFIX.len()..value.len() - 1]) {
			return Err(report!(ExecutionError));
		}

		let mut normalized = Vec::with_capacity(value.len() - 6);
		normalized.extend_from_slice(&[BACKSLASH, BACKSLASH]);
		normalized.extend_from_slice(&value[VERBATIM_UNC_PREFIX.len()..]);
		return Ok(normalized);
	}

	let drive = value
		.get(4)
		.copied()
		.is_some_and(|value| value <= u16::from(u8::MAX) && (value as u8).is_ascii_alphabetic());
	if drive && value.get(5) == Some(&u16::from(b':')) && value.get(6) == Some(&BACKSLASH) {
		if has_conversion_sensitive_component(&value[7..value.len() - 1]) {
			return Err(report!(ExecutionError));
		}

		return Ok(value[4..].to_vec());
	}

	Ok(value)
}

fn has_conversion_sensitive_component(path: &[u16]) -> bool {
	path.split(|value| *value == BACKSLASH || *value == u16::from(b'/'))
		.filter(|component| !component.is_empty())
		.any(|component| {
			component
				.last()
				.is_some_and(|value| *value == u16::from(b'.') || *value == u16::from(b' '))
				|| component.contains(&u16::from(b':'))
				|| is_dos_device_name(component)
		})
}

fn is_dos_device_name(component: &[u16]) -> bool {
	let stem = component
		.split(|value| *value == u16::from(b'.'))
		.next()
		.unwrap_or(component);
	let stem = &stem[..stem
		.iter()
		.rposition(|value| *value != u16::from(b' '))
		.map_or(0, |index| index + 1)];
	if [
		b"CON".as_slice(),
		b"PRN",
		b"AUX",
		b"NUL",
		b"CLOCK$",
		b"CONIN$",
		b"CONOUT$",
	]
	.into_iter()
	.any(|name| equals_ascii_case(stem, name))
	{
		return true;
	}

	if stem.len() != 4 || !(equals_ascii_case(&stem[..3], b"COM") || equals_ascii_case(&stem[..3], b"LPT")) {
		return false;
	}

	matches!(stem[3], 0x31..=0x39 | 0x00B9 | 0x00B2 | 0x00B3)
}

fn equals_ascii_case(value: &[u16], expected: &[u8]) -> bool {
	value.len() == expected.len()
		&& value.iter().zip(expected).all(|(value, expected)| {
			*value == u16::from(*expected) || *value == u16::from(expected.to_ascii_lowercase())
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalizes_verbatim_drive_paths() -> Result<(), ExecutionError> {
		assert_eq!(
			native_path_wide(OsStr::new(r"\\?\C:\mods\Data"))?,
			wide(OsStr::new(r"C:\mods\Data"))?
		);

		Ok(())
	}

	#[test]
	fn normalizes_verbatim_unc_paths() -> Result<(), ExecutionError> {
		assert_eq!(
			native_path_wide(OsStr::new(r"\\?\UNC\server\share\Data"))?,
			wide(OsStr::new(r"\\server\share\Data"))?
		);

		Ok(())
	}

	#[test]
	fn preserves_ordinary_paths() -> Result<(), ExecutionError> {
		assert_eq!(
			native_path_wide(OsStr::new(r"C:\mods\Data"))?,
			wide(OsStr::new(r"C:\mods\Data"))?
		);

		Ok(())
	}

	#[test]
	fn preserves_other_namespace_paths() -> Result<(), ExecutionError> {
		let volume = r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\Data";
		assert_eq!(native_path_wide(OsStr::new(volume))?, wide(OsStr::new(volume))?);

		Ok(())
	}

	#[test]
	fn does_not_truncate_long_paths() -> Result<(), ExecutionError> {
		let segment = "a".repeat(300);
		let verbatim = format!(r"\\?\C:\{segment}");
		let ordinary = format!(r"C:\{segment}");
		assert_eq!(native_path_wide(OsStr::new(&verbatim))?, wide(OsStr::new(&ordinary))?);

		Ok(())
	}

	#[test]
	fn rejects_verbatim_components_changed_by_dos_normalization() {
		for path in [
			r"\\?\C:\Data\trailing.",
			r"\\?\C:\Data\trailing ",
			r"\\?\C:\Data\NUL",
			r"\\?\C:\Data\com1.txt",
			r"\\?\C:\Data\NUL .txt",
			r"\\?\C:\Data\file.txt:stream",
			r"\\?\UNC\server\share\COM1 .log",
			r"\\?\UNC\server\share\LPT9.log",
		] {
			assert!(native_path_wide(OsStr::new(path)).is_err(), "{path}");
		}
	}

	#[test]
	fn normalizes_short_physical_unc_paths() -> Result<(), ExecutionError> {
		assert_eq!(
			physical_path_wide(OsStr::new(r"\\?\UNC\server\share\Data"))?,
			wide(OsStr::new(r"\\server\share\Data"))?
		);

		Ok(())
	}

	#[test]
	fn normalizes_physical_unc_below_native_long_path_threshold() -> Result<(), ExecutionError> {
		let ordinary_prefix = "\\\\server\\share\\";
		let segment = "a".repeat(MAX_PATH_CHARACTERS - 1 - ordinary_prefix.encode_utf16().count());
		let ordinary = format!(r"{ordinary_prefix}{segment}");
		let verbatim = format!(r"\\?\UNC\server\share\{segment}");
		assert_eq!(ordinary.encode_utf16().count(), MAX_PATH_CHARACTERS - 1);
		assert_eq!(physical_path_wide(OsStr::new(&verbatim))?, wide(OsStr::new(&ordinary))?);

		Ok(())
	}

	#[test]
	fn preserves_physical_unc_at_native_long_path_threshold() -> Result<(), ExecutionError> {
		let ordinary_prefix = "\\\\server\\share\\";
		let segment = "a".repeat(MAX_PATH_CHARACTERS - ordinary_prefix.encode_utf16().count());
		let ordinary = format!(r"{ordinary_prefix}{segment}");
		let verbatim = format!(r"\\?\UNC\server\share\{segment}");
		assert_eq!(ordinary.encode_utf16().count(), MAX_PATH_CHARACTERS);
		assert_eq!(physical_path_wide(OsStr::new(&verbatim))?, wide(OsStr::new(&verbatim))?);

		Ok(())
	}
}
