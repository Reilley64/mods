use crate::fs_access;
use application::ErrorMarker;
use cap_std::fs::Dir;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::str;
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

const PROFILE_FILES: [&str; 8] = [
	"Fallout.ini",
	"FalloutPrefs.ini",
	"FalloutCustom.ini",
	"GECKCustom.ini",
	"GECKPrefs.ini",
	"plugins.txt",
	"loadorder.txt",
	"Plugins.fnvviewsettings",
];
const MANAGED_ARCHIVE_KEYS: [&str; 3] = ["bInvalidateOlderFiles", "SInvalidationFile", "sArchiveList"];
const MANAGED_GENERAL_KEYS: [&str; 2] = ["bUseMyGamesDirectory", "SLocalSavePath"];

pub(crate) fn validate(root: &Dir) -> Result<(), ErrorMarker> {
	validate_root_entries(root)?;
	let mods = fs_access::open_dir(root, Path::new("mods")).context(ErrorMarker::environment_root_unsafe())?;
	let overwrite =
		fs_access::open_dir(root, Path::new("overwrite")).context(ErrorMarker::environment_root_unsafe())?;
	validate_safe_tree(&overwrite)?;
	if entry_names(&overwrite)?.contains("meta.toml") {
		validate_meta(&read_regular(&overwrite, "meta.toml")?)?;
	}
	let profile = fs_access::open_dir(root, Path::new("profile")).context(ErrorMarker::environment_root_unsafe())?;
	let listed_mods = validate_profile(&profile)?;
	validate_mods(&mods, &listed_mods)
}

fn validate_root_entries(root: &Dir) -> Result<(), ErrorMarker> {
	let allowed = HashSet::from(["mods.toml", "mods", "profile", "overwrite", "cache", "temp", "logs"]);
	for name in entry_names(root)? {
		if !allowed.contains(name.as_str()) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		if name == "temp" {
			let directory = open_real_dir(root, &name)?;
			validate_safe_tree(&directory)?;
		}
	}
	Ok(())
}

fn validate_profile(profile: &Dir) -> Result<HashSet<String>, ErrorMarker> {
	let allowed = PROFILE_FILES
		.into_iter()
		.chain(["modlist.txt", "saves"])
		.collect::<HashSet<_>>();
	let names = entry_names(profile)?;
	if names.iter().any(|name| !allowed.contains(name.as_str()))
		|| ["Fallout.ini", "plugins.txt", "loadorder.txt", "modlist.txt", "saves"]
			.into_iter()
			.any(|required| !names.contains(required))
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	for name in names.iter().filter(|name| name.as_str() != "saves") {
		open_real_file(profile, name)?;
	}
	let saves = open_real_dir(profile, "saves")?;
	validate_safe_tree(&saves)?;

	let fallout = decode_ini(&read_regular(profile, "Fallout.ini")?)?;
	let keys = archive_values(&fallout);
	if keys.get("binvalidateolderfiles")
		.is_none_or(|values| values.as_slice() != ["1"])
		|| keys.get("sinvalidationfile")
			.is_none_or(|values| values.as_slice() != [""])
		|| keys.get("sarchivelist")
			.is_none_or(|values| values.len() != 1 || !archive_list_valid(values[0]))
		|| contains_keys_outside_section(&fallout, "Archive", &MANAGED_ARCHIVE_KEYS)
		|| general_values(&fallout, "bUseMyGamesDirectory").as_slice() != ["1"]
		|| general_values(&fallout, "SLocalSavePath").as_slice() != ["__mods_saves\\"]
		|| contains_keys_outside_section(&fallout, "General", &MANAGED_GENERAL_KEYS)
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	for name in ["FalloutPrefs.ini", "FalloutCustom.ini"] {
		if names.contains(name) {
			let text = decode_ini(&read_regular(profile, name)?)?;
			if contains_keys(&text, &MANAGED_GENERAL_KEYS)
				|| (name == "FalloutCustom.ini" && contains_keys(&text, &MANAGED_ARCHIVE_KEYS))
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
		}
	}
	validate_plugin_list(&read_regular(profile, "plugins.txt")?, false)?;
	validate_plugin_list(&read_regular(profile, "loadorder.txt")?, true)?;
	parse_modlist(&read_regular(profile, "modlist.txt")?)
}

fn validate_mods(mods: &Dir, listed_mods: &HashSet<String>) -> Result<(), ErrorMarker> {
	let mut installed = HashSet::new();
	for name in entry_names(mods)? {
		if !valid_windows_component(&name) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let directory = open_real_dir(mods, &name)?;
		validate_safe_tree(&directory)?;
		open_real_file(&directory, "meta.toml")?;
		validate_meta(&read_regular(&directory, "meta.toml")?)?;
		if !installed.insert(name.to_lowercase()) {
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
	let metadata: toml::Value = toml::from_str(text).context(ErrorMarker::environment_invalid(None))?;
	if metadata.get("schema_version").and_then(toml::Value::as_integer) != Some(1) {
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
		if !matches!(state, "+" | "-") || !valid_windows_component(name) || !listed.insert(name.to_lowercase())
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

fn validate_safe_tree(directory: &Dir) -> Result<(), ErrorMarker> {
	for name in entry_names(directory)? {
		let metadata = directory
			.symlink_metadata(&name)
			.map_err(|error| report!(error).context(ErrorMarker::environment_root_unsafe()))?;
		if fs_access::is_reparse(&metadata) {
			return Err(report!(ErrorMarker::environment_root_unsafe()));
		}
		if metadata.is_dir() {
			let child = fs_access::open_dir(directory, Path::new(&name))
				.context(ErrorMarker::environment_root_unsafe())?;
			validate_safe_tree(&child)?;
		} else if metadata.is_file() {
			fs_access::open_regular(directory, Path::new(&name))
				.context(ErrorMarker::environment_root_unsafe())?;
		} else {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

fn open_real_dir(parent: &Dir, name: &str) -> Result<Dir, ErrorMarker> {
	let metadata = parent
		.symlink_metadata(name)
		.map_err(|error| report!(error).context(ErrorMarker::environment_invalid(None)))?;
	if fs_access::is_reparse(&metadata) {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}
	if !metadata.is_dir() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	fs_access::open_dir(parent, Path::new(name)).context(ErrorMarker::environment_root_unsafe())
}

fn open_real_file(parent: &Dir, name: &str) -> Result<(), ErrorMarker> {
	let metadata = parent
		.symlink_metadata(name)
		.map_err(|error| report!(error).context(ErrorMarker::environment_invalid(None)))?;
	if fs_access::is_reparse(&metadata) {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}
	if !metadata.is_file() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	fs_access::open_regular(parent, Path::new(name)).context(ErrorMarker::environment_root_unsafe())?;
	Ok(())
}

fn entry_names(directory: &Dir) -> Result<HashSet<String>, ErrorMarker> {
	let mut names = HashSet::new();
	for entry in directory.entries().context(ErrorMarker::environment_invalid(None))? {
		let entry = entry.context(ErrorMarker::environment_invalid(None))?;
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

fn read_regular(directory: &Dir, name: &str) -> Result<Vec<u8>, ErrorMarker> {
	let mut file =
		fs_access::open_regular(directory, Path::new(name)).context(ErrorMarker::environment_invalid(None))?;
	let mut bytes = Vec::new();
	file.read_to_end(&mut bytes)
		.context(ErrorMarker::environment_invalid(None))?;
	Ok(bytes)
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
			let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
			Ok(text.into_owned())
		}
	}
}

fn validate_plugin_list(bytes: &[u8], utf8: bool) -> Result<(), ErrorMarker> {
	let text = if utf8 {
		str::from_utf8(bytes)
			.context(ErrorMarker::environment_invalid(None))?
			.to_owned()
	} else {
		decode_active_code_page(bytes)?
	};
	if text.replace("\r\n", "").contains(['\r', '\n']) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	for line in text.split_terminator("\r\n").filter(|line| !line.is_empty()) {
		if line.starts_with('#') {
			continue;
		}
		if line.trim() != line
			|| line.starts_with('*')
			|| line.chars().any(char::is_control)
			|| line.chars().any(|character| {
				matches!(character, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
			}) || is_reserved_name(line)
			|| !line.get(line.len().saturating_sub(4)..).is_some_and(|extension| {
				extension.eq_ignore_ascii_case(".esm") || extension.eq_ignore_ascii_case(".esp")
			}) {
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
	if code_page == 65_001 {
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
	Ok(encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned())
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

fn archive_list_valid(value: &str) -> bool {
	let values = value
		.split(',')
		.map(str::trim)
		.filter(|item| !item.is_empty())
		.collect::<Vec<_>>();
	values.first()
		.is_some_and(|first| first.eq_ignore_ascii_case("Fallout - Invalidation.bsa"))
		&& values
			.iter()
			.filter(|item| item.eq_ignore_ascii_case("Fallout - Invalidation.bsa"))
			.count() == 1
}

fn archive_values(text: &str) -> HashMap<String, Vec<&str>> {
	section_values(text, "Archive", &MANAGED_ARCHIVE_KEYS)
}

fn general_values<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
	section_values(text, "General", &[key])
		.remove(&key.to_ascii_lowercase())
		.unwrap_or_default()
}

fn section_name(line: &str) -> Option<&str> {
	let line = line.trim();
	line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn contains_keys(text: &str, keys: &[&str]) -> bool {
	text.lines().any(|line| {
		line.split_once('=')
			.is_some_and(|(key, _)| keys.iter().any(|wanted| wanted.eq_ignore_ascii_case(key.trim())))
	})
}

fn contains_keys_outside_section(text: &str, section: &str, keys: &[&str]) -> bool {
	let mut current = "";
	for line in text.lines() {
		if let Some(found) = section_name(line) {
			current = found;
			continue;
		}
		if !current.eq_ignore_ascii_case(section)
			&& line.split_once('=').is_some_and(|(key, _)| {
				keys.iter().any(|wanted| wanted.eq_ignore_ascii_case(key.trim()))
			}) {
			return true;
		}
	}
	false
}

fn section_values<'a>(text: &'a str, wanted_section: &str, wanted_keys: &[&str]) -> HashMap<String, Vec<&'a str>> {
	let mut result = HashMap::new();
	let mut current = "";
	for line in text.lines() {
		if let Some(section) = line.trim().strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
			current = section.trim();
			continue;
		}
		if !current.eq_ignore_ascii_case(wanted_section) {
			continue;
		}
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};
		if wanted_keys.iter().any(|wanted| wanted.eq_ignore_ascii_case(key.trim())) {
			result.entry(key.trim().to_ascii_lowercase())
				.or_insert_with(Vec::new)
				.push(value.trim());
		}
	}
	result
}
