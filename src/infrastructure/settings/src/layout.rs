use application::ErrorMarker;
use domain::IGNORED_PROFILE_FILES;
use domain::canonical_profile_routing_valid;
use domain::case_fold_key;
use encoding_rs::WINDOWS_1252;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashSet;
use std::path::Path;
use std::str;
use tokio::fs::metadata;
use tokio::fs::read;
use tokio::fs::read_dir;
use toml::Value;
use toml::from_str;
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

const PROFILE_FILES: [&str; 7] = [
	"Fallout.ini",
	"FalloutPrefs.ini",
	"FalloutCustom.ini",
	"GECKCustom.ini",
	"GECKPrefs.ini",
	"plugins.txt",
	"Plugins.fnvviewsettings",
];
/// Validates the canonical Mod Environment layout for settings commands.
///
/// Only entry names, entry types, and file contents are checked. Links are
/// followed like ordinary entries.
pub(crate) async fn validate(root: &Path) -> Result<(), ErrorMarker> {
	validate_root_entries(root).await?;

	let overwrite = root.join("overwrite");
	require_directory(&overwrite).await?;
	if entry_names(&overwrite).await?.contains("meta.toml") {
		validate_meta(&read_regular(&overwrite, "meta.toml").await?)?;
	}

	let listed_mods = validate_profile(&root.join("profile")).await?;
	validate_mods(&root.join("mods"), &listed_mods).await
}

async fn validate_root_entries(root: &Path) -> Result<(), ErrorMarker> {
	let allowed = HashSet::from(["mods.toml", "mods", "profile", "overwrite", "cache", "temp", "logs"]);
	for name in entry_names(root).await? {
		if !allowed.contains(name.as_str()) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

async fn validate_profile(profile: &Path) -> Result<HashSet<String>, ErrorMarker> {
	let allowed = PROFILE_FILES
		.into_iter()
		.chain(IGNORED_PROFILE_FILES)
		.chain(["modlist.txt", "saves"])
		.collect::<HashSet<_>>();
	let names = entry_names(profile).await?;
	if names.iter().any(|name| !allowed.contains(name.as_str()))
		|| ["Fallout.ini", "plugins.txt", "modlist.txt", "saves"]
			.into_iter()
			.any(|required| !names.contains(required))
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	for name in names.iter().filter(|name| name.as_str() != "saves") {
		require_file(&profile.join(name)).await?;
	}
	require_directory(&profile.join("saves")).await?;

	let fallout = decode_ini(&read_regular(profile, "Fallout.ini").await?)?;
	if !canonical_profile_routing_valid("Fallout.ini", &fallout) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	for name in ["FalloutPrefs.ini", "FalloutCustom.ini"] {
		if !names.contains(name) {
			continue;
		}

		let text = decode_ini(&read_regular(profile, name).await?)?;
		if !canonical_profile_routing_valid(name, &text) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	validate_plugin_list(&read_regular(profile, "plugins.txt").await?)?;
	parse_modlist(&read_regular(profile, "modlist.txt").await?)
}

async fn validate_mods(mods: &Path, listed_mods: &HashSet<String>) -> Result<(), ErrorMarker> {
	let mut installed = HashSet::new();
	for name in entry_names(mods).await? {
		if !valid_windows_component(&name) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}

		let directory = mods.join(&name);
		require_directory(&directory).await?;

		if entry_names(&directory).await?.contains("meta.toml") {
			validate_meta(&read_regular(&directory, "meta.toml").await?)?;
		}
		if !installed.insert(case_fold_key(&name)) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	if &installed != listed_mods {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

fn validate_meta(bytes: &[u8]) -> Result<(), ErrorMarker> {
	let text = str::from_utf8(bytes).context(ErrorMarker::environment_invalid(None))?;
	let metadata: Value = from_str(text).context(ErrorMarker::environment_invalid(None))?;
	if metadata.get("schema_version").and_then(Value::as_integer) != Some(1) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

fn parse_modlist(bytes: &[u8]) -> Result<HashSet<String>, ErrorMarker> {
	let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
	let text = str::from_utf8(bytes).context(ErrorMarker::environment_invalid(None))?;
	let mut listed = HashSet::new();
	for line in text.lines() {
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		let Some((state, name)) = line.split_at_checked(1) else {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		};
		if !matches!(state, "+" | "-") || !valid_windows_component(name) || !listed.insert(case_fold_key(name))
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(listed)
}

fn valid_windows_component(name: &str) -> bool {
	!name.is_empty()
		&& name.trim() == name
		&& !name.ends_with('.')
		&& !name.chars().any(char::is_control)
		&& !name.chars()
			.any(|character| matches!(character, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'))
		&& !is_reserved_name(name)
}

async fn require_directory(path: &Path) -> Result<(), ErrorMarker> {
	let metadata = metadata(path).await.context(ErrorMarker::environment_invalid(None))?;
	if !metadata.is_dir() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

async fn require_file(path: &Path) -> Result<(), ErrorMarker> {
	let metadata = metadata(path).await.context(ErrorMarker::environment_invalid(None))?;
	if !metadata.is_file() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

async fn entry_names(directory: &Path) -> Result<HashSet<String>, ErrorMarker> {
	let mut names = HashSet::new();
	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		let name = entry
			.file_name()
			.into_string()
			.map_err(|_| report!(ErrorMarker::environment_invalid(None)))?;
		if !names.insert(name) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(names)
}

async fn read_regular(directory: &Path, name: &str) -> Result<Vec<u8>, ErrorMarker> {
	read(directory.join(name))
		.await
		.context(ErrorMarker::environment_invalid(None))
}

fn decode_ini(bytes: &[u8]) -> Result<String, ErrorMarker> {
	if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
		return String::from_utf8(bytes.to_vec()).context(ErrorMarker::environment_invalid(None));
	}
	if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
		if bytes.len() % 2 != 0 {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let mut values = Vec::with_capacity(bytes.len() / 2);
		let mut index = 0;
		while index < bytes.len() {
			values.push(u16::from_le_bytes([bytes[index], bytes[index + 1]]));
			index += 2;
		}
		return String::from_utf16(&values).context(ErrorMarker::environment_invalid(None));
	}
	match String::from_utf8(bytes.to_vec()) {
		Ok(text) => Ok(text),
		Err(_) => {
			let (text, _, _) = WINDOWS_1252.decode(bytes);
			Ok(text.into_owned())
		}
	}
}

fn validate_plugin_list(bytes: &[u8]) -> Result<(), ErrorMarker> {
	// A UTF-8 byte order mark is not part of the first plugin name.
	let text = decode_active_code_page(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))?;
	if text.replace("\r\n", "").contains(['\r', '\n']) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	for line in text.split_terminator("\r\n").filter(|line| !line.is_empty()) {
		if line.starts_with('#') {
			continue;
		}
		let folded = case_fold_key(line);
		if line.trim() != line
			|| line.starts_with('*')
			|| line.chars().any(char::is_control)
			|| line.chars().any(|character| {
				matches!(character, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
			}) || is_reserved_name(line)
			|| ![".esm", ".esp", ".esl"]
				.iter()
				.any(|extension| folded.ends_with(extension))
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

#[cfg(windows)]
fn decode_active_code_page(bytes: &[u8]) -> Result<String, ErrorMarker> {
	if bytes.is_empty() {
		return Ok(String::new());
	}

	// SAFETY: GetACP has no preconditions and returns the current process ANSI code page.
	let code_page = unsafe { GetACP() };
	if code_page == CP_UTF8 {
		return String::from_utf8(bytes.to_vec()).context(ErrorMarker::environment_invalid(None));
	}

	// SAFETY: The input slice is valid, and None requests the required output length.
	let length = unsafe { MultiByteToWideChar(code_page, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None) };
	if length <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	let mut wide = vec![0_u16; usize::try_from(length).context(ErrorMarker::environment_invalid(None))?];
	// SAFETY: `wide` was sized by the preceding query and both slices remain valid for the call.
	let written =
		unsafe { MultiByteToWideChar(code_page, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut wide)) };
	if written <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	if written != length {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	let text = String::from_utf16(&wide).context(ErrorMarker::environment_invalid(None))?;
	let mut used_default = BOOL(0);

	// SAFETY: The UTF-16 slice and BOOL pointer are valid; None requests the output length.
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

	// SAFETY: `encoded` was sized by the preceding query and all slices/pointers remain valid.
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

#[cfg(not(windows))]
fn decode_active_code_page(bytes: &[u8]) -> Result<String, ErrorMarker> {
	Ok(WINDOWS_1252.decode(bytes).0.into_owned())
}

fn is_reserved_name(name: &str) -> bool {
	let base = name.split('.').next().unwrap_or("");
	matches!(
		base.to_ascii_uppercase().as_str(),
		"CON" | "PRN" | "AUX" | "NUL" | "CLOCK$"
	) || (base.len() == 4
		&& base.get(..3)
			.is_some_and(|prefix| prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
		&& base.as_bytes()[3].is_ascii_digit()
		&& base.as_bytes()[3] != b'0')
}
