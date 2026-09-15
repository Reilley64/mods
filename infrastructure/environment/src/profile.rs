use crate::check_cancelled;
use crate::safe_fs::SafeDir;
use crate::safe_fs::is_reparse;
use application::ErrorMarker;
use application::ports::InitializationProfileSources;
use application::ports::ProfileFileDisposition;
use application::ports::ProfileFileRecord;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::str::from_utf8;
use tokio_util::sync::CancellationToken;
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

pub(crate) const PROFILE_FILES: [&str; 8] = [
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

pub(crate) fn stage_profile(
	profile: &SafeDir,
	sources: &InitializationProfileSources,
	cancellation: &CancellationToken,
) -> Result<Vec<ProfileFileRecord>, ErrorMarker> {
	check_cancelled(cancellation)?;
	profile.create_dir("saves")
		.context(ErrorMarker::environment_root_unsafe())?;
	check_cancelled(cancellation)?;
	let source_map: HashMap<_, _> = sources
		.files
		.iter()
		.map(|source| (source.name, source.contents.as_deref()))
		.collect();
	if source_map.len() != PROFILE_FILES.len() || PROFILE_FILES.iter().any(|name| !source_map.contains_key(name)) {
		return Err(report!(ErrorMarker::game_install_invalid()));
	}
	let original_fallout = source_map.get("Fallout.ini").copied().flatten();
	let original_custom = source_map.get("FalloutCustom.ini").copied().flatten();
	let tail_source = original_custom
		.and_then(last_archive_list)
		.or_else(|| original_fallout.and_then(last_archive_list))
		.or_else(|| last_archive_list(&sources.fallout_default_ini))
		.unwrap_or_default();
	let archive_list = normalized_archive_list(&tail_source);

	let mut records = Vec::with_capacity(PROFILE_FILES.len());
	for name in PROFILE_FILES {
		check_cancelled(cancellation)?;
		let contents = source_map.get(name).copied().flatten();
		let (output, disposition) = match (name, contents) {
			("Fallout.ini", Some(contents)) => (
				Some(patch_fallout_ini(contents, &archive_list)?),
				ProfileFileDisposition::Imported,
			),
			("Fallout.ini", None) => (
				Some(patch_fallout_ini(&sources.fallout_default_ini, &archive_list)?),
				ProfileFileDisposition::SeededFromGame,
			),
			("FalloutPrefs.ini", Some(contents)) => (
				Some(strip_save_routing_keys(contents)?),
				ProfileFileDisposition::Imported,
			),
			("FalloutCustom.ini", Some(contents)) => (
				Some(strip_custom_routing_keys(contents)?),
				ProfileFileDisposition::Imported,
			),
			("plugins.txt" | "loadorder.txt", None) => {
				(Some(Vec::new()), ProfileFileDisposition::CreatedEmpty)
			}
			(_, Some(contents)) => (Some(contents.to_vec()), ProfileFileDisposition::Imported),
			(_, None) => (None, ProfileFileDisposition::Absent),
		};
		if let Some(output) = output {
			profile.write_new(name, &output)
				.context(ErrorMarker::environment_root_unsafe())?;
			check_cancelled(cancellation)?;
		}
		records.push(ProfileFileRecord { name, disposition });
	}
	profile.write_new("modlist.txt", b"")
		.context(ErrorMarker::environment_root_unsafe())?;
	check_cancelled(cancellation)?;
	validate_profile(profile)?;
	Ok(records)
}

pub(crate) fn validate_profile(profile: &SafeDir) -> Result<(), ErrorMarker> {
	let allowed = PROFILE_FILES.iter().copied().chain(["modlist.txt", "saves"]);
	let mut expected = allowed.collect::<HashSet<_>>();
	for name in profile.entries().context(ErrorMarker::environment_invalid(None))? {
		let name = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if !expected.remove(name) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let metadata = profile
			.symlink_metadata(name)
			.context(ErrorMarker::environment_invalid(None))?;
		if is_reparse(&metadata)
			|| (name == "saves" && !metadata.is_dir())
			|| (name != "saves" && !metadata.is_file())
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	for required in ["Fallout.ini", "plugins.txt", "loadorder.txt", "modlist.txt", "saves"] {
		if expected.contains(required) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	let saves = profile
		.open_dir("saves")
		.context(ErrorMarker::environment_invalid(None))?;
	if !saves
		.entries()
		.context(ErrorMarker::environment_invalid(None))?
		.is_empty()
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let fallout = read_regular_file(profile, "Fallout.ini")?;
	let text = decode(&fallout)?.0;
	let keys = archive_values(&text);
	if keys.get("binvalidateolderfiles")
		.is_none_or(|values| values.as_slice() != ["1"])
		|| keys.get("sinvalidationfile")
			.is_none_or(|values| values.as_slice() != [""])
		|| keys.get("sarchivelist")
			.is_none_or(|values| values.len() != 1 || !archive_list_valid(values[0]))
		|| contains_keys_outside_section(&text, "Archive", &MANAGED_ARCHIVE_KEYS)
		|| general_values(&text, "bUseMyGamesDirectory").as_slice() != ["1"]
		|| general_values(&text, "SLocalSavePath").as_slice() != ["__mods_saves\\"]
		|| contains_keys_outside_section(&text, "General", &MANAGED_GENERAL_KEYS)
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	for name in ["FalloutPrefs.ini", "FalloutCustom.ini"] {
		if profile.exists(name).context(ErrorMarker::environment_invalid(None))? {
			let bytes = read_regular_file(profile, name)?;
			let text = decode(&bytes)?.0;
			if contains_keys(&text, &MANAGED_GENERAL_KEYS)
				|| (name == "FalloutCustom.ini" && contains_keys(&text, &MANAGED_ARCHIVE_KEYS))
			{
				return Err(report!(ErrorMarker::environment_invalid(None)));
			}
		}
	}
	validate_plugin_list(profile, "plugins.txt", false)?;
	validate_plugin_list(profile, "loadorder.txt", true)?;
	let modlist = read_regular_file(profile, "modlist.txt")?;
	if !modlist.is_empty() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

fn validate_plugin_list(profile: &SafeDir, name: &str, utf8: bool) -> Result<(), ErrorMarker> {
	let bytes = read_regular_file(profile, name)?;
	let text = if utf8 {
		from_utf8(&bytes)
			.context(ErrorMarker::environment_invalid(None))?
			.to_owned()
	} else {
		decode_active_code_page(&bytes)?
	};
	let without_crlf = text.replace("\r\n", "");
	if without_crlf.contains(['\r', '\n']) {
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
			|| !(line.get(line.len().saturating_sub(4)..).is_some_and(|extension| {
				extension.eq_ignore_ascii_case(".esm") || extension.eq_ignore_ascii_case(".esp")
			})) {
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
	// SAFETY: GetACP takes no arguments, accesses no caller-owned memory, and retains no pointers.
	let code_page = unsafe { GetACP() };
	if code_page == 65_001 {
		return String::from_utf8(bytes.to_vec()).context(ErrorMarker::environment_invalid(None));
	}
	// SAFETY: `bytes` is initialized and remains live for the call. `code_page` came from GetACP,
	// zero is a valid flag set, and None is the documented size query. The API retains no pointers.
	let length = unsafe { MultiByteToWideChar(code_page, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, None) };
	if length <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	let mut wide = vec![0_u16; usize::try_from(length).context(ErrorMarker::environment_invalid(None))?];
	let written = {
		// SAFETY: `wide` has the exact queried length and is exclusively borrowed. Both slices remain live,
		// the binding supplies their bounds to Windows, and the API retains no pointers.
		unsafe { MultiByteToWideChar(code_page, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), bytes, Some(&mut wide)) }
	};
	if written <= 0 {
		return Err(report!(WindowsError::from_thread()).context(ErrorMarker::environment_invalid(None)));
	}
	if written != length {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	let text = String::from_utf16(&wide).context(ErrorMarker::environment_invalid(None))?;
	let mut used_default = BOOL(0);
	// SAFETY: `wide` contains initialized UTF-16 validated above and remains live. None is the documented
	// size query, `used_default` is exclusively borrowed, and a null default character is allowed. The API
	// retains no pointers.
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
	// SAFETY: `encoded` has the exact queried length and is exclusively borrowed. `wide` and `used_default`
	// remain live and disjoint, a null default character is allowed, and the API retains no pointers.
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
	if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned())
}

fn is_reserved_name(name: &str) -> bool {
	let base = name.split('.').next().unwrap_or("");
	matches!(
		base.to_ascii_uppercase().as_str(),
		"CON" | "PRN" | "AUX" | "NUL" | "CLOCK$"
	) || (base.len() == 4
		&& (base.get(..3).is_some_and(|prefix| {
			prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT")
		})) && base.as_bytes()[3].is_ascii_digit()
		&& base.as_bytes()[3] != b'0')
}

fn read_regular_file(directory: &SafeDir, name: &str) -> Result<Vec<u8>, ErrorMarker> {
	directory
		.read_regular(name)
		.context(ErrorMarker::environment_invalid(None))
}

fn patch_fallout_ini(bytes: &[u8], archive_list: &str) -> Result<Vec<u8>, ErrorMarker> {
	let (text, encoding) = decode(bytes)?;
	let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
	let mut lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
	remove_keys_any_section(&mut lines, &MANAGED_ARCHIVE_KEYS);
	remove_keys_any_section(&mut lines, &MANAGED_GENERAL_KEYS);
	insert_section_values(
		&mut lines,
		"General",
		&[
			"bUseMyGamesDirectory=1".to_owned(),
			"SLocalSavePath=__mods_saves\\".to_owned(),
		],
	);
	insert_section_values(
		&mut lines,
		"Archive",
		&[
			"bInvalidateOlderFiles=1".to_owned(),
			"SInvalidationFile=".to_owned(),
			format!("sArchiveList={archive_list}"),
		],
	);
	encode(&(lines.join(newline) + newline), encoding)
}

fn strip_save_routing_keys(bytes: &[u8]) -> Result<Vec<u8>, ErrorMarker> {
	strip_routing_keys(bytes, false)
}

fn strip_custom_routing_keys(bytes: &[u8]) -> Result<Vec<u8>, ErrorMarker> {
	strip_routing_keys(bytes, true)
}

fn strip_routing_keys(bytes: &[u8], strip_archive: bool) -> Result<Vec<u8>, ErrorMarker> {
	let (text, encoding) = decode(bytes)?;
	let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
	let trailing = text.ends_with('\n');
	let mut lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
	if strip_archive {
		remove_keys_any_section(&mut lines, &MANAGED_ARCHIVE_KEYS);
	}
	remove_keys_any_section(&mut lines, &MANAGED_GENERAL_KEYS);
	let mut result = lines.join(newline);
	if trailing {
		result.push_str(newline);
	}
	encode(&result, encoding)
}

fn remove_keys_any_section(lines: &mut Vec<String>, keys: &[&str]) {
	lines.retain(|line| {
		let Some((key, _)) = line.split_once('=') else {
			return true;
		};
		!keys.iter().any(|managed| managed.eq_ignore_ascii_case(key.trim()))
	});
}

fn insert_section_values(lines: &mut Vec<String>, section: &str, values: &[String]) {
	let header = format!("[{section}]");
	let start = lines
		.iter()
		.position(|line| section_name(line).is_some_and(|name| name.eq_ignore_ascii_case(section)));
	if let Some(start) = start {
		let end = lines
			.iter()
			.enumerate()
			.skip(start + 1)
			.find(|(_, line)| section_name(line).is_some())
			.map_or(lines.len(), |(index, _)| index);
		for (offset, value) in values.iter().enumerate() {
			lines.insert(end + offset, value.clone());
		}
	} else {
		if lines.last().is_some_and(|line| !line.is_empty()) {
			lines.push(String::new());
		}
		lines.push(header);
		lines.extend(values.iter().cloned());
	}
}

fn last_archive_list(bytes: &[u8]) -> Option<String> {
	let (text, _) = decode(bytes).ok()?;
	let mut current = "";
	let mut found = None;
	for line in text.lines() {
		if let Some(section) = section_name(line) {
			current = section;
			continue;
		}
		if current.eq_ignore_ascii_case("Archive")
			&& let Some((key, value)) = line.split_once('=')
			&& key.trim().eq_ignore_ascii_case("sArchiveList")
		{
			found = Some(value.trim().to_owned());
		}
	}
	found
}

fn normalized_archive_list(source: &str) -> String {
	let mut values = vec!["Fallout - Invalidation.bsa".to_owned()];
	values.extend(source
		.split(',')
		.map(str::trim)
		.filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("Fallout - Invalidation.bsa"))
		.map(ToOwned::to_owned));
	values.join(", ")
}

fn archive_list_valid(value: &str) -> bool {
	let values: Vec<_> = value
		.split(',')
		.map(str::trim)
		.filter(|item| !item.is_empty())
		.collect();
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
	let mut result: HashMap<String, Vec<&str>> = HashMap::new();
	let mut current = "";
	for line in text.lines() {
		if let Some(section) = section_name(line) {
			current = section;
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
				.or_default()
				.push(value.trim());
		}
	}
	result
}

fn section_name(line: &str) -> Option<&str> {
	let line = line.trim();
	line.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

#[derive(Clone, Copy)]
enum Encoding {
	Utf8,
	Utf8Bom,
	Utf16Le,
	Windows1252,
}
fn decode(bytes: &[u8]) -> Result<(String, Encoding), ErrorMarker> {
	if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
		return String::from_utf8(bytes.to_vec())
			.map(|text| (text, Encoding::Utf8Bom))
			.context(ErrorMarker::game_install_invalid());
	}
	if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
		if bytes.len() % 2 != 0 {
			return Err(report!(ErrorMarker::game_install_invalid()));
		}
		let mut values = Vec::with_capacity(bytes.len() / 2);
		let mut index = 0;
		while index < bytes.len() {
			values.push(u16::from_le_bytes([bytes[index], bytes[index + 1]]));
			index += 2;
		}
		return String::from_utf16(&values)
			.map(|text| (text, Encoding::Utf16Le))
			.context(ErrorMarker::game_install_invalid());
	}
	match String::from_utf8(bytes.to_vec()) {
		Ok(text) => Ok((text, Encoding::Utf8)),
		Err(_) => {
			let (text, _, _) = encoding_rs::WINDOWS_1252.decode(bytes);
			Ok((text.into_owned(), Encoding::Windows1252))
		}
	}
}

fn encode(text: &str, encoding: Encoding) -> Result<Vec<u8>, ErrorMarker> {
	match encoding {
		Encoding::Utf8 => Ok(text.as_bytes().to_vec()),
		Encoding::Utf8Bom => Ok([&[0xef, 0xbb, 0xbf], text.as_bytes()].concat()),
		Encoding::Utf16Le => {
			let mut output = vec![0xff, 0xfe];
			for value in text.encode_utf16() {
				output.extend_from_slice(&value.to_le_bytes());
			}
			Ok(output)
		}
		Encoding::Windows1252 => {
			let (bytes, _, had_errors) = encoding_rs::WINDOWS_1252.encode(text);
			if had_errors {
				Err(report!(ErrorMarker::game_install_invalid()))
			} else {
				Ok(bytes.into_owned())
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use application::ports::ProfileSource;
	use std::error::Error;
	use std::fs;
	use std::result::Result as StdResult;
	use tempfile::TempDir;

	#[test]
	fn stages_import_seed_empty_absent_and_never_saves() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let files = PROFILE_FILES
			.into_iter()
			.map(|name| {
				ProfileSourceFixture::source(
					name,
					match name {
						"FalloutCustom.ini" => Some(concat!(
							"[Archive]\r\nsArchiveList=Custom.bsa, ",
							"Fallout - Invalidation.bsa\r\n",
						)
						.as_bytes()
						.to_vec()),
						"plugins.txt" => Some(b"Example.esm\r\n".to_vec()),
						_ => None,
					},
				)
			})
			.collect();
		let sources = InitializationProfileSources {
			files,
			fallout_default_ini: b"[Archive]\r\nsArchiveList=Default.bsa\r\n".to_vec(),
		};
		let profile =
			SafeDir::open_absolute(&temp.path().canonicalize()?).map_err(|_| "safe dir open failed")?;
		let records =
			stage_profile(&profile, &sources, &CancellationToken::new()).map_err(|_| "stage failed")?;
		assert_eq!(records[0].disposition, ProfileFileDisposition::SeededFromGame);
		assert_eq!(records[2].disposition, ProfileFileDisposition::Imported);
		assert_eq!(records[5].disposition, ProfileFileDisposition::Imported);
		assert_eq!(records[6].disposition, ProfileFileDisposition::CreatedEmpty);
		assert!(fs::read_dir(temp.path().join("saves"))?.next().is_none());
		let fallout = fs::read_to_string(temp.path().join("Fallout.ini"))?;
		assert!(fallout.contains("sArchiveList=Fallout - Invalidation.bsa, Custom.bsa"));
		assert!(fallout.contains("SLocalSavePath=__mods_saves\\"));
		let custom = fs::read_to_string(temp.path().join("FalloutCustom.ini"))?;
		assert!(!custom.to_ascii_lowercase().contains("sarchivelist"));
		assert!(!custom.to_ascii_lowercase().contains("slocalsavepath"));
		Ok(())
	}

	#[test]
	fn stages_exact_save_routing_and_removes_conflicts_from_imports() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let fallout = concat!(
			"[gEnErAl]\r\n",
			"; keep fallout comment\r\n",
			"bOther=keep\r\n",
			"bInvalidateOlderFiles=0\r\n",
			"bInvalidateOlderFiles=0\r\n",
			"bUseMyGamesDirectory=0\r\n",
			"SLocalSavePath=elsewhere\r\n",
			"busemygamesdirectory=0\r\n",
			"slocalsavepath=other\r\n",
			"[General]\r\n",
			"bUseMyGamesDirectory=0\r\n",
			"[Display]\r\n",
			"SInvalidationFile=misplaced\r\n",
			"sArchiveList=Misplaced.bsa\r\n",
			"SInvalidationFile=misplaced\r\n",
			"sArchiveList=Misplaced.bsa\r\n",
			"bUseMyGamesDirectory=0\r\n",
			"SLocalSavePath=elsewhere\r\n",
		);
		let prefs = concat!(
			"[General]\r\n",
			"; keep nonfallout comment\r\n",
			"bOther=keep\r\n",
			"bUseMyGamesDirectory=0\r\n",
			"SLocalSavePath=elsewhere\r\n",
			"[general]\r\n",
			"busemygamesdirectory=0\r\n",
			"slocalsavepath=other\r\n",
			"[Display]\r\n",
			"bUseMyGamesDirectory=0\r\n",
			"SLocalSavePath=elsewhere\r\n",
			"[Archive]\r\n",
			"sArchiveList=Prefs.bsa\r\n",
		);
		let custom = concat!(
			"[General]\r\n",
			"; keep nonfallout comment\r\n",
			"bOther=keep\r\n",
			"bUseMyGamesDirectory=0\r\n",
			"SLocalSavePath=elsewhere\r\n",
			"[general]\r\n",
			"busemygamesdirectory=0\r\n",
			"slocalsavepath=other\r\n",
			"[Display]\r\n",
			"bUseMyGamesDirectory=0\r\n",
			"SLocalSavePath=elsewhere\r\n",
		);
		let files = PROFILE_FILES
			.into_iter()
			.map(|name| {
				ProfileSourceFixture::source(
					name,
					match name {
						"Fallout.ini" => Some(fallout.as_bytes().to_vec()),
						"FalloutPrefs.ini" => Some(prefs.as_bytes().to_vec()),
						"FalloutCustom.ini" => Some(custom.as_bytes().to_vec()),
						_ => None,
					},
				)
			})
			.collect();
		let sources = InitializationProfileSources {
			files,
			fallout_default_ini: b"[General]\r\nbUseMyGamesDirectory=0\r\nSLocalSavePath=elsewhere\r\n"
				.to_vec(),
		};
		let original_sources = sources.clone();
		let profile =
			SafeDir::open_absolute(&temp.path().canonicalize()?).map_err(|_| "safe dir open failed")?;
		stage_profile(&profile, &sources, &CancellationToken::new()).map_err(|_| "stage failed")?;

		let fallout = fs::read_to_string(temp.path().join("Fallout.ini"))?;
		assert_eq!(general_values(&fallout, "bUseMyGamesDirectory"), ["1"]);
		assert_eq!(general_values(&fallout, "SLocalSavePath"), ["__mods_saves\\"]);
		assert!(fallout.contains("; keep fallout comment"));
		assert!(fallout.contains("bOther=keep"));
		assert_eq!(fallout.to_ascii_lowercase().matches("busemygamesdirectory").count(), 1);
		assert_eq!(fallout.to_ascii_lowercase().matches("slocalsavepath").count(), 1);
		for key in ["binvalidateolderfiles", "sinvalidationfile", "sarchivelist"] {
			assert_eq!(fallout.to_ascii_lowercase().matches(key).count(), 1);
		}
		for name in ["FalloutPrefs.ini", "FalloutCustom.ini"] {
			let text = fs::read_to_string(temp.path().join(name))?;
			assert!(general_values(&text, "bUseMyGamesDirectory").is_empty());
			assert!(general_values(&text, "SLocalSavePath").is_empty());
			assert!(!text.to_ascii_lowercase().contains("busemygamesdirectory"));
			assert!(!text.to_ascii_lowercase().contains("slocalsavepath"));
			assert!(text.contains("; keep nonfallout comment"));
			assert!(text.contains("bOther=keep"));
			if name == "FalloutPrefs.ini" {
				assert!(text.contains("sArchiveList=Prefs.bsa"));
			} else {
				for key in ["binvalidateolderfiles", "sinvalidationfile", "sarchivelist"] {
					assert!(!text.to_ascii_lowercase().contains(key));
				}
			}
		}
		assert_eq!(sources.files, original_sources.files);
		Ok(())
	}

	#[test]
	fn seeds_exact_save_routing_from_conflicting_fallout_ini() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let files = PROFILE_FILES
			.into_iter()
			.map(|name| ProfileSourceFixture::source(name, None))
			.collect();
		let sources = InitializationProfileSources {
			files,
			fallout_default_ini: concat!(
				"[General]\n",
				"bUseMyGamesDirectory=0\n",
				"SLocalSavePath=elsewhere\n",
				"[general]\n",
				"busemygamesdirectory=0\n",
				"slocalsavepath=other\n",
			)
			.as_bytes()
			.to_vec(),
		};
		let profile =
			SafeDir::open_absolute(&temp.path().canonicalize()?).map_err(|_| "safe dir open failed")?;
		stage_profile(&profile, &sources, &CancellationToken::new()).map_err(|_| "stage failed")?;

		let fallout = fs::read_to_string(temp.path().join("Fallout.ini"))?;
		assert_eq!(general_values(&fallout, "bUseMyGamesDirectory"), ["1"]);
		assert_eq!(general_values(&fallout, "SLocalSavePath"), ["__mods_saves\\"]);
		Ok(())
	}

	#[test]
	fn invalid_plugin_entries_block_publication() -> StdResult<(), Box<dyn Error>> {
		for value in [b"*Active.esm\r\n".as_slice(), b"folder\\Bad.esp\r\n", b"CON.esm\r\n"] {
			let temp = TempDir::new()?;
			let files = PROFILE_FILES
				.into_iter()
				.map(|name| {
					ProfileSourceFixture::source(
						name,
						if name == "plugins.txt" {
							Some(value.to_vec())
						} else {
							None
						},
					)
				})
				.collect();
			let sources = InitializationProfileSources {
				files,
				fallout_default_ini: b"[Archive]\r\n".to_vec(),
			};
			let profile = SafeDir::open_absolute(&temp.path().canonicalize()?)
				.map_err(|_| "safe dir open failed")?;
			assert!(stage_profile(&profile, &sources, &CancellationToken::new()).is_err());
		}
		Ok(())
	}

	struct ProfileSourceFixture;
	impl ProfileSourceFixture {
		fn source(name: &'static str, contents: Option<Vec<u8>>) -> ProfileSource {
			ProfileSource { name, contents }
		}
	}
}
