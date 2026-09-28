use crate::active_code_page::decode as decode_active_code_page;
use crate::active_code_page::encode as encode_active_code_page;
use crate::safe_fs::EntryBudget;
use crate::safe_fs::MAX_TRAVERSAL_DEPTH;
use crate::safe_fs::SafeDir;
use crate::safe_fs::is_reparse;
use crate::safe_fs::read_bounded;
use crate::snapshot::visible_plugins;
use application::ErrorMarker;
use application::ports::InitializationProfileSources;
use application::ports::ProfileFileDisposition;
use application::ports::ProfileFileRecord;
use domain::ModName;
use domain::case_fold_key;
use encoding_rs::WINDOWS_1252;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::str::from_utf8;
use tokio_util::sync::CancellationToken;

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
pub(crate) const MAX_PROFILE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SAVE_ENTRIES: usize = 100_000;

pub(crate) fn stage_profile(
	profile: &SafeDir,
	sources: &InitializationProfileSources,
	cancellation: &CancellationToken,
) -> Result<Vec<ProfileFileRecord>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	profile.create_dir("saves")
		.context(ErrorMarker::environment_root_unsafe())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

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
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

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
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
		}
		records.push(ProfileFileRecord { name, disposition });
	}
	profile.write_new("modlist.txt", b"")
		.context(ErrorMarker::environment_root_unsafe())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	validate_profile(profile, cancellation)?;
	Ok(records)
}

pub(crate) fn validate_profile(profile: &SafeDir, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	validate_profile_files(profile, true, cancellation)
}

pub(crate) fn validate_profile_files(
	profile: &SafeDir,
	require_empty: bool,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	validate_profile_mode(profile, require_empty, false, cancellation)
}

pub(crate) fn validate_execution_profile(
	profile: &SafeDir,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	validate_profile_mode(profile, false, true, cancellation)
}

fn validate_profile_mode(
	profile: &SafeDir,
	require_empty: bool,
	execution: bool,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let allowed = PROFILE_FILES.iter().copied().chain(["modlist.txt", "saves"]);
	let mut expected = allowed.collect::<HashSet<_>>();
	let allowed_count = expected.len();
	let opened = profile.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
	let mut observed = 0_usize;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
		observed = observed.saturating_add(1);
		if observed > allowed_count {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let name = entry.file_name();
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
		if expected.contains(required) && !(execution && matches!(required, "plugins.txt" | "loadorder.txt")) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	let saves = profile
		.open_dir("saves")
		.context(ErrorMarker::environment_invalid(None))?;
	if require_empty {
		let opened = saves.entries();
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
		if let Some(entry) = entries.next() {
			entry.into_report().context(ErrorMarker::environment_invalid(None))?;
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	if !execution {
		validate_saves(&saves, cancellation)?;
	}
	let fallout = read_regular_file(profile, "Fallout.ini", cancellation)?;
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
		if !profile.exists(name).context(ErrorMarker::environment_invalid(None))? {
			continue;
		}

		let bytes = read_regular_file(profile, name, cancellation)?;
		let text = decode(&bytes)?.0;
		if contains_keys(&text, &MANAGED_GENERAL_KEYS)
			|| (name == "FalloutCustom.ini" && contains_keys(&text, &MANAGED_ARCHIVE_KEYS))
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	for (name, utf8) in [("plugins.txt", false), ("loadorder.txt", true)] {
		if !execution || profile.exists(name).context(ErrorMarker::environment_invalid(None))? {
			validate_plugin_list(profile, name, utf8, !execution, cancellation)?;
		}
	}
	let modlist = read_regular_file(profile, "modlist.txt", cancellation)?;
	if require_empty && !modlist.is_empty() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

fn validate_saves(directory: &SafeDir, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	let mut budget = EntryBudget::new(MAX_SAVE_ENTRIES);
	validate_saves_inner(directory, cancellation, &mut budget, MAX_TRAVERSAL_DEPTH)
}

fn validate_saves_inner(
	directory: &SafeDir,
	cancellation: &CancellationToken,
	budget: &mut EntryBudget,
	remaining_depth: usize,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
	let mut directory_entries = 0_usize;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
		budget.consume(&mut directory_entries)
			.context(ErrorMarker::environment_invalid(None))?;
		if remaining_depth == 0 {
			return Err(report!(io::Error::new(
				io::ErrorKind::InvalidData,
				"directory traversal depth limit exceeded",
			))
			.context(ErrorMarker::environment_invalid(None)));
		}
		let name = entry.file_name();
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let metadata = directory.symlink_metadata(&name);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let metadata = metadata.context(ErrorMarker::environment_invalid(None))?;
		if metadata.is_dir() {
			let child = directory.open_dir(&name);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			validate_saves_inner(
				&child.context(ErrorMarker::environment_invalid(None))?,
				cancellation,
				budget,
				remaining_depth - 1,
			)?;
		} else if metadata.is_file() {
			let opened = directory.open_regular(&name);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			opened.context(ErrorMarker::environment_invalid(None))?;
		} else {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

pub(crate) fn stage_plugin_maintenance(
	root: &SafeDir,
	staged_mod: &SafeDir,
	staged_profile: &SafeDir,
	mod_name: &ModName,
	mut profile_files: Vec<String>,
	cancellation: &CancellationToken,
) -> Result<Vec<String>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let before = visible_plugins(root, None, cancellation)?;
	let after = visible_plugins(root, Some((mod_name, staged_mod)), cancellation)?;
	let unavailable = before
		.keys()
		.filter(|name| !after.contains_key(*name))
		.cloned()
		.collect::<HashSet<_>>();
	let mut newly_visible = after
		.iter()
		.filter(|(name, _)| !before.contains_key(*name))
		.map(|(name, spelling)| (name.clone(), spelling.clone()))
		.collect::<Vec<_>>();
	newly_visible.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
	let profile = root
		.open_dir("profile")
		.context(ErrorMarker::environment_invalid(None))?;

	let plugins_bytes = read_bounded(
		&profile,
		"plugins.txt",
		MAX_PROFILE_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)?;
	let plugins_text = decode_active_code_page(&plugins_bytes)?;
	let plugins_output = remove_unavailable_lines(&plugins_text, &unavailable);
	if plugins_output != plugins_text {
		let bytes = encode_active_code_page(&plugins_output)?;
		staged_profile
			.write_new("plugins.txt", &bytes)
			.context(ErrorMarker::transaction_failure())?;
		profile_files.push("plugins.txt".to_owned());
	}

	let loadorder_bytes = read_bounded(
		&profile,
		"loadorder.txt",
		MAX_PROFILE_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)?;
	let loadorder_text = from_utf8(&loadorder_bytes).context(ErrorMarker::environment_invalid(None))?;
	let mut loadorder_output = remove_unavailable_lines(loadorder_text, &unavailable);
	let existing = loadorder_output
		.split_terminator("\r\n")
		.filter(|line| !line.is_empty() && !line.starts_with('#'))
		.map(case_fold_key)
		.collect::<HashSet<_>>();
	for (key, spelling) in newly_visible {
		if existing.contains(&key) {
			continue;
		}

		if !loadorder_output.is_empty() && !loadorder_output.ends_with("\r\n") {
			loadorder_output.push_str("\r\n");
		}
		loadorder_output.push_str(&spelling);
		loadorder_output.push_str("\r\n");
	}
	if loadorder_output != loadorder_text {
		staged_profile
			.write_new("loadorder.txt", loadorder_output.as_bytes())
			.context(ErrorMarker::transaction_failure())?;
		profile_files.push("loadorder.txt".to_owned());
	}
	Ok(profile_files)
}

fn remove_unavailable_lines(text: &str, unavailable: &HashSet<String>) -> String {
	let mut output = String::with_capacity(text.len());
	for line_with_separator in text.split_inclusive("\r\n") {
		let line = line_with_separator.strip_suffix("\r\n").unwrap_or(line_with_separator);
		if !line.is_empty() && !line.starts_with('#') && unavailable.contains(&case_fold_key(line)) {
			continue;
		}
		output.push_str(line_with_separator);
	}
	output
}

fn validate_plugin_list(
	profile: &SafeDir,
	name: &str,
	utf8: bool,
	allow_light_plugins: bool,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let bytes = read_regular_file(profile, name, cancellation)?;
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
			|| !is_activatable_plugin_name(line)
			|| (!allow_light_plugins && case_fold_key(line).ends_with(".esl"))
		{
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

pub(crate) fn is_activatable_plugin_name(name: &str) -> bool {
	let folded = case_fold_key(name);
	[".esp", ".esm", ".esl"]
		.iter()
		.any(|extension| folded.ends_with(extension))
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

fn read_regular_file(
	directory: &SafeDir,
	name: &str,
	cancellation: &CancellationToken,
) -> Result<Vec<u8>, ErrorMarker> {
	read_bounded(
		directory,
		name,
		MAX_PROFILE_BYTES,
		ErrorMarker::environment_invalid(None),
		cancellation,
	)
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
	let mut current_section = "";
	let mut last_value = None;
	for line in text.lines() {
		if let Some(section) = section_name(line) {
			current_section = section;
			continue;
		}
		if current_section.eq_ignore_ascii_case("Archive")
			&& let Some((key, value)) = line.split_once('=')
			&& key.trim().eq_ignore_ascii_case("sArchiveList")
		{
			last_value = Some(value.trim().to_owned());
		}
	}
	last_value
}

fn normalized_archive_list(source: &str) -> String {
	let mut values = vec!["Fallout - Invalidation.bsa".to_owned()];
	values.extend(source
		.split(',')
		.map(str::trim)
		.filter(|value| !value.is_empty() && !is_invalidation_archive(value))
		.map(ToOwned::to_owned));
	values.join(", ")
}

fn archive_list_valid(value: &str) -> bool {
	let values: Vec<_> = value
		.split(',')
		.map(str::trim)
		.filter(|item| !item.is_empty())
		.collect();
	values.first().is_some_and(|first| is_invalidation_archive(first))
		&& values.iter().filter(|item| is_invalidation_archive(item)).count() == 1
}

fn is_invalidation_archive(value: &str) -> bool {
	case_fold_key(value) == "fallout - invalidation.bsa"
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
	let mut current_section = "";
	for line in text.lines() {
		if let Some(found) = section_name(line) {
			current_section = found;
			continue;
		}
		if !current_section.eq_ignore_ascii_case(section)
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
	let mut current_section = "";
	for line in text.lines() {
		if let Some(section) = section_name(line) {
			current_section = section;
			continue;
		}
		if !current_section.eq_ignore_ascii_case(wanted_section) {
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
pub(crate) enum Encoding {
	Utf8,
	Utf8Bom,
	Utf16Le,
	Windows1252,
}
pub(crate) fn decode(bytes: &[u8]) -> Result<(String, Encoding), ErrorMarker> {
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
			let (text, _, _) = WINDOWS_1252.decode(bytes);
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
			let (bytes, _, had_errors) = WINDOWS_1252.encode(text);
			if had_errors {
				Err(report!(ErrorMarker::game_install_invalid()))
			} else {
				Ok(bytes.into_owned())
			}
		}
	}
}

/// Disposable #98 proof, not a native process or production export implementation.
#[cfg(test)]
mod export_prototype {
	use super::MANAGED_ARCHIVE_KEYS;
	use super::MANAGED_GENERAL_KEYS;
	use super::decode;
	use super::encode;
	use super::insert_section_values;
	use super::last_archive_list;
	use super::normalized_archive_list;
	use super::remove_keys_any_section;
	use super::section_name;
	use domain::DataRelativePath;
	use domain::EffectiveResult;
	use domain::ModName;
	use domain::ModPriority;
	use domain::ParticipationReason;
	use domain::ProviderClass;
	use domain::ProviderReference;
	use domain::Tombstone;
	use domain::TombstoneIndex;
	use domain::TombstoneScope;
	use domain::case_fold_key;
	use domain::resolve_effective_file;
	use rootcause::prelude::ResultExt;
	use rootcause::report;
	use std::collections::BTreeMap;
	use std::collections::HashMap;
	use std::collections::HashSet;
	use std::error::Error;
	use std::fmt;
	use std::fs::File;
	use std::fs::FileTimes;
	use std::fs::{self};
	use std::path::Path;
	use std::path::PathBuf;
	use std::time::SystemTime;
	use tempfile::TempDir;

	#[derive(Debug)]
	enum PrototypeError {
		Invalid(&'static str),
		Fixture,
	}

	impl fmt::Display for PrototypeError {
		fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
			match self {
				Self::Invalid(message) => formatter.write_str(message),
				Self::Fixture => formatter.write_str("prototype fixture operation failed"),
			}
		}
	}
	impl Error for PrototypeError {}

	type Trial<T> = rootcause::Result<T, PrototypeError>;

	fn invalid(message: &'static str) -> rootcause::Report<PrototypeError> {
		report!(PrototypeError::Invalid(message))
	}

	// The existing optional reader turns a decode error into absence. Decode first, including
	// the fallback only when selected, so malformed relevant input never silently falls through.
	fn archive_input(custom: Option<&[u8]>, fallout: &[u8], fallback: Option<&[u8]>) -> Trial<String> {
		for bytes in [custom, Some(fallout), fallback] {
			let Some(bytes) = bytes else { continue };
			decode(bytes).context(PrototypeError::Invalid("invalid archive INI encoding"))?;
			if let Some(value) = last_archive_list(bytes) {
				return Ok(normalized_archive_list(&value));
			}
		}
		if fallback.is_none() {
			return Err(invalid("required bound-game fallback missing"));
		}
		Ok(normalized_archive_list(""))
	}

	fn derived_ini(bytes: &[u8], archives: &str, execution: bool, fallout: bool) -> Trial<Vec<u8>> {
		let (text, encoding) = decode(bytes).context(PrototypeError::Invalid("invalid canonical INI"))?;
		let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
		let trailing = text.ends_with('\n');
		let mut lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
		if fallout {
			remove_keys_any_section(&mut lines, &MANAGED_ARCHIVE_KEYS);
			remove_keys_any_section(&mut lines, &MANAGED_GENERAL_KEYS);
			insert_section_values(
				&mut lines,
				"Archive",
				&[
					"bInvalidateOlderFiles=1".to_owned(),
					"SInvalidationFile=".to_owned(),
					format!("sArchiveList={archives}"),
				],
			);
			insert_section_values(
				&mut lines,
				"General",
				&[
					"bUseMyGamesDirectory=1".to_owned(),
					format!(
						"SLocalSavePath={}",
						if execution { "__mods_saves\\" } else { "Saves\\" }
					),
				],
			);
		} else {
			remove_keys_any_section(&mut lines, &MANAGED_GENERAL_KEYS);
			remove_keys_any_section(&mut lines, &MANAGED_ARCHIVE_KEYS);
		}
		let mut output = lines.join(newline);
		if trailing || fallout {
			output.push_str(newline);
		}
		encode(&output, encoding).context(PrototypeError::Invalid("derived INI cannot be encoded"))
	}

	fn project_profile(
		canonical: &BTreeMap<&str, Vec<u8>>,
		fallback: Option<&[u8]>,
		execution: bool,
	) -> Trial<BTreeMap<String, Vec<u8>>> {
		let fallout = canonical
			.get("Fallout.ini")
			.ok_or_else(|| invalid("Fallout.ini missing"))?;
		let archives = archive_input(canonical.get("FalloutCustom.ini").map(Vec::as_slice), fallout, fallback)?;
		let mut projected = BTreeMap::new();
		for (name, bytes) in canonical {
			let contents = match *name {
				"Fallout.ini" => derived_ini(bytes, &archives, execution, true)?,
				"FalloutCustom.ini" | "FalloutPrefs.ini" => {
					derived_ini(bytes, &archives, execution, false)?
				}
				_ => bytes.clone(),
			};
			projected.insert((*name).to_owned(), contents);
		}
		Ok(projected)
	}

	fn managed(section: &str, line: &str) -> bool {
		let Some((key, _)) = line.split_once('=') else {
			return false;
		};
		(section.eq_ignore_ascii_case("Archive")
			&& MANAGED_ARCHIVE_KEYS
				.iter()
				.any(|item| item.eq_ignore_ascii_case(key.trim())))
			|| (section.eq_ignore_ascii_case("General")
				&& MANAGED_GENERAL_KEYS
					.iter()
					.any(|item| item.eq_ignore_ascii_case(key.trim())))
	}

	fn controlled_lines(text: &str) -> Trial<HashMap<String, (String, String)>> {
		let mut found = HashMap::new();
		let mut section = "";
		for line in text.lines() {
			if let Some(name) = section_name(line) {
				section = name;
			}
			let Some((name, _)) = line.split_once('=') else {
				continue;
			};
			let key = name.trim().to_ascii_lowercase();
			let expected_section = if MANAGED_ARCHIVE_KEYS
				.iter()
				.any(|candidate| candidate.eq_ignore_ascii_case(&key))
			{
				"Archive"
			} else if MANAGED_GENERAL_KEYS
				.iter()
				.any(|candidate| candidate.eq_ignore_ascii_case(&key))
			{
				"General"
			} else {
				continue;
			};
			if !section.eq_ignore_ascii_case(expected_section)
				|| found.insert(key, (section.to_ascii_lowercase(), line.to_owned()))
					.is_some()
			{
				return Err(invalid("ambiguous or relocated controlled key"));
			}
		}
		Ok(found)
	}

	fn reconcile(original: &[u8], baseline: &[u8], child: &[u8], current: &[u8]) -> Trial<Vec<u8>> {
		if original != current {
			return Err(invalid("canonical changed concurrently"));
		}

		let (source, encoding) = decode(original).context(PrototypeError::Invalid("canonical invalid"))?;
		let (baseline, _) = decode(baseline).context(PrototypeError::Invalid("baseline invalid"))?;
		let (child, _) = decode(child).context(PrototypeError::Invalid("child invalid"))?;
		let originals = controlled_lines(&source)?;
		let baseline_keys = controlled_lines(&baseline)?;
		let child_keys = controlled_lines(&child)?;
		if baseline_keys.keys().any(|key| {
			child_keys.get(key).map(|(section, _)| section)
				!= baseline_keys.get(key).map(|(section, _)| section)
		}) || baseline_keys.len() != child_keys.len()
		{
			return Err(invalid("child controlled-key identity changed"));
		}

		let newline = if source.contains("\r\n") { "\r\n" } else { "\n" };
		let mut section = "";
		let mut output = Vec::new();
		let mut inserted = HashSet::new();
		for line in child.lines() {
			if let Some(name) = section_name(line) {
				section = name;
				output.push(line.to_owned());
				for (key, (original_section, value)) in &originals {
					if original_section.eq_ignore_ascii_case(section)
						&& !baseline_keys.contains_key(key)
					{
						output.push(value.clone());
						inserted.insert(key.clone());
					}
				}
				continue;
			}
			if managed(section, line) {
				let (key, _) = line.split_once('=').ok_or_else(|| invalid("controlled key invalid"))?;
				let key = key.trim().to_ascii_lowercase();
				if let Some((_, original)) = originals.get(&key) {
					output.push(original.clone());
				}
			} else {
				output.push(line.to_owned());
			}
		}
		if originals
			.keys()
			.any(|key| !baseline_keys.contains_key(key) && !inserted.contains(key))
		{
			return Err(invalid("original controlled section missing"));
		}
		let mut text = output.join(newline);
		if child.ends_with('\n') {
			text.push_str(newline);
		}
		encode(&text, encoding).context(PrototypeError::Invalid("reconciled INI cannot be encoded"))
	}

	struct Contribution {
		provider: ProviderReference,
		bytes: &'static [u8],
	}

	fn plan(entries: &[Contribution], tombstones: &TombstoneIndex) -> Trial<BTreeMap<String, Vec<u8>>> {
		let mut winners: HashMap<String, &Contribution> = HashMap::new();
		let mut directories: HashMap<String, (domain::ProviderRank, String)> = HashMap::new();
		for entry in entries {
			let provider = &entry.provider;
			if provider.original_path().comparison_key() == case_fold_key("meta.toml")
				&& provider.class() != ProviderClass::SteamData
			{
				continue;
			}
			if provider.participation_reason() == ParticipationReason::DisabledMod {
				continue;
			}
			let path = provider.original_path();
			let mut parent = String::new();
			for component in path
				.as_str()
				.split('/')
				.take(path.components().count().saturating_sub(1))
			{
				if !parent.is_empty() {
					parent.push('/');
				}
				parent.push_str(component);
				let key = case_fold_key(&parent);
				if directories.get(&key).is_none_or(|(rank, _)| *rank < provider.rank()) {
					directories.insert(key, (provider.rank(), component.to_owned()));
				}
			}
			let key = path.comparison_key().to_owned();
			if winners
				.get(&key)
				.is_none_or(|winner| winner.provider.rank() < provider.rank())
			{
				winners.insert(key, entry);
			}
		}
		let mut output = BTreeMap::new();
		for (key, winner) in winners {
			let provider = &winner.provider;
			if provider.class() == ProviderClass::SteamData {
				continue;
			}
			if !matches!(
				resolve_effective_file(Some(provider), tombstones.controlling(&key)),
				EffectiveResult::File(_)
			) {
				continue;
			}
			let components: Vec<_> = provider.original_path().as_str().split('/').collect();
			let mut parent = String::new();
			let mut spelled = Vec::new();
			for component in components.iter().take(components.len().saturating_sub(1)) {
				if !parent.is_empty() {
					parent.push('/');
				}
				parent.push_str(component);
				let (_, display) = directories
					.get(&case_fold_key(&parent))
					.ok_or_else(|| invalid("directory missing"))?;
				spelled.push(display.as_str());
			}
			spelled.push(components.last().ok_or_else(|| invalid("file missing"))?);
			output.insert(spelled.join("/"), winner.bytes.to_vec());
		}
		Ok(output)
	}

	fn write_with_mtime(source: &Path, destination: &Path, bytes: &[u8]) -> Trial<()> {
		if let Some(parent) = destination.parent() {
			fs::create_dir_all(parent).context(PrototypeError::Fixture)?;
		}
		fs::write(destination, bytes).context(PrototypeError::Fixture)?;
		let modified = fs::metadata(source)
			.context(PrototypeError::Fixture)?
			.modified()
			.context(PrototypeError::Fixture)?;
		File::options()
			.write(true)
			.open(destination)
			.context(PrototypeError::Fixture)?
			.set_times(FileTimes::new().set_modified(modified))
			.context(PrototypeError::Fixture)?;
		if fs::metadata(destination)
			.context(PrototypeError::Fixture)?
			.modified()
			.context(PrototypeError::Fixture)?
			!= modified
		{
			return Err(invalid("mtime not preserved"));
		}
		Ok(())
	}

	fn stage_once(
		root: &Path,
		final_path: &Path,
		files: &[(PathBuf, String, Vec<u8>)],
		stop_after: Option<usize>,
	) -> Trial<PathBuf> {
		if final_path.exists() {
			return Err(invalid("output exists"));
		}

		let staged = root.join("disposable-partial");
		fs::create_dir(&staged).context(PrototypeError::Fixture)?;
		for (index, (source, relative, bytes)) in files.iter().enumerate() {
			if stop_after == Some(index) {
				return Err(invalid("cancelled; partial retained"));
			}
			write_with_mtime(source, &staged.join(relative), bytes)?;
		}

		if final_path.exists() {
			return Err(invalid("output appeared before publication"));
		}
		fs::rename(&staged, final_path).context(PrototypeError::Fixture)?;

		Ok(final_path.to_owned())
	}

	fn profile_payload(source: &Path, include_saves: bool) -> Trial<BTreeMap<String, Vec<u8>>> {
		let mut files = BTreeMap::new();
		for name in super::PROFILE_FILES.into_iter().chain(["modlist.txt"]) {
			let path = source.join(name);
			if path.is_file() {
				files.insert(
					format!("profile/{name}"),
					fs::read(path).context(PrototypeError::Fixture)?,
				);
			}
		}
		if include_saves {
			let saves = source.join("saves");
			if saves.exists() {
				let mut pending = vec![(saves, "profile/saves".to_owned())];
				while let Some((directory, relative)) = pending.pop() {
					for entry in fs::read_dir(directory).context(PrototypeError::Fixture)? {
						let entry = entry.context(PrototypeError::Fixture)?;
						let name = entry
							.file_name()
							.into_string()
							.map_err(|_| invalid("non-Unicode save name"))?;
						let path = format!("{relative}/{name}");
						let kind = entry.file_type().context(PrototypeError::Fixture)?;
						if kind.is_dir() {
							pending.push((entry.path(), path));
						} else if kind.is_file() {
							files.insert(
								path,
								fs::read(entry.path())
									.context(PrototypeError::Fixture)?,
							);
						} else {
							return Err(invalid("unsupported save entry in prototype"));
						}
					}
				}
			}
		}
		Ok(files)
	}

	#[test]
	fn profile_payload_is_separate_from_data_and_saves_are_opt_in() -> Trial<()> {
		let scratch = TempDir::new().context(PrototypeError::Fixture)?;
		fs::create_dir(scratch.path().join("saves")).context(PrototypeError::Fixture)?;
		fs::write(scratch.path().join("Fallout.ini"), b"canonical").context(PrototypeError::Fixture)?;
		fs::write(scratch.path().join("modlist.txt"), b"+Example").context(PrototypeError::Fixture)?;
		fs::create_dir(scratch.path().join("saves/Chapter")).context(PrototypeError::Fixture)?;
		fs::write(scratch.path().join("saves/Chapter/Slot.fos"), b"saved").context(PrototypeError::Fixture)?;
		fs::write(scratch.path().join("private.db"), b"private").context(PrototypeError::Fixture)?;
		assert_eq!(
			profile_payload(scratch.path(), false)?,
			BTreeMap::from([
				("profile/Fallout.ini".to_owned(), b"canonical".to_vec()),
				("profile/modlist.txt".to_owned(), b"+Example".to_vec()),
			])
		);
		assert_eq!(
			profile_payload(scratch.path(), true)?,
			BTreeMap::from([
				("profile/Fallout.ini".to_owned(), b"canonical".to_vec()),
				("profile/modlist.txt".to_owned(), b"+Example".to_vec()),
				("profile/saves/Chapter/Slot.fos".to_owned(), b"saved".to_vec()),
			])
		);
		Ok(())
	}

	#[test]
	fn positive_plan_uses_rank_tombstones_and_one_directory_spelling() -> Trial<()> {
		let path = |name: &str| DataRelativePath::new(name.to_owned()).context(PrototypeError::Fixture);
		let modded = |name: &str, rank, enabled| -> Trial<ProviderReference> {
			Ok(ProviderReference::DataMod {
				mod_name: ModName::new(format!("Mod {rank}")).context(PrototypeError::Fixture)?,
				priority: ModPriority::new(rank),
				original_path: path(name)?,
				participation_reason: if enabled {
					ParticipationReason::EnabledMod
				} else {
					ParticipationReason::DisabledMod
				},
			})
		};
		let entries = vec![
			Contribution {
				provider: ProviderReference::SteamData {
					original_path: path("base.esm")?,
				},
				bytes: b"base",
			},
			Contribution {
				provider: modded("Textures/Shared.dds", 1, true)?,
				bytes: b"loser",
			},
			Contribution {
				provider: modded("textures/shared.dds", 3, true)?,
				bytes: b"winner",
			},
			Contribution {
				provider: modded("Textures/Only.dds", 1, true)?,
				bytes: b"unique",
			},
			Contribution {
				provider: modded("textures/Disabled.dds", 9, false)?,
				bytes: b"disabled",
			},
			Contribution {
				provider: modded("meshes/Chair.nif", 1, true)?,
				bytes: b"suppressed",
			},
			Contribution {
				provider: modded("meshes/Restored.nif", 5, true)?,
				bytes: b"restored",
			},
			Contribution {
				provider: modded("meta.toml", 2, true)?,
				bytes: b"private root metadata",
			},
			Contribution {
				provider: modded("nested/meta.toml", 2, true)?,
				bytes: b"ordinary metadata-like",
			},
			Contribution {
				provider: modded("Pack.bsa", 2, true)?,
				bytes: b"opaque BSA",
			},
			Contribution {
				provider: ProviderReference::Overwrite {
					original_path: path("textures/Overwrite.dds")?,
				},
				bytes: b"overwrite",
			},
		];
		let mut tombstones = TombstoneIndex::default();
		tombstones.insert(Tombstone {
			owner: modded("meshes", 4, true)?,
			scope: TombstoneScope::DirectorySubtree,
		});
		let selected = plan(&entries, &tombstones)?;
		assert_eq!(
			selected,
			BTreeMap::from([
				("Pack.bsa".to_owned(), b"opaque BSA".to_vec()),
				("meshes/Restored.nif".to_owned(), b"restored".to_vec()),
				("nested/meta.toml".to_owned(), b"ordinary metadata-like".to_vec()),
				("textures/Only.dds".to_owned(), b"unique".to_vec()),
				("textures/Overwrite.dds".to_owned(), b"overwrite".to_vec()),
				("textures/shared.dds".to_owned(), b"winner".to_vec()),
			])
		);
		Ok(())
	}

	#[test]
	fn canonical_archive_precedence_and_separate_derived_inis() -> Trial<()> {
		let original = b"[General]\r\nSLocalSavePath=Saves\\\r\nbUseMyGamesDirectory=1\r\n[Archive]\r\nsArchiveList=Original.bsa\r\n; untouched\r\n";
		let custom = b"[Archive]\r\nsArchiveList=First.bsa\r\nsARCHIVELIST=Custom.bsa, Fallout - Invalidation.bsa\r\n[Display]\r\nbOther=keep\r\n";
		let canonical = BTreeMap::from([
			("Fallout.ini", original.to_vec()),
			("FalloutCustom.ini", custom.to_vec()),
			("plugins.txt", b"*Example.esm\r\n".to_vec()),
		]);
		let export = project_profile(&canonical, Some(b"[Archive]\nsArchiveList=Default.bsa\n"), false)?;
		let exec = project_profile(&canonical, Some(b"[Archive]\nsArchiveList=Default.bsa\n"), true)?;
		let exported = String::from_utf8(export["Fallout.ini"].clone()).context(PrototypeError::Fixture)?;
		assert!(exported.contains("sArchiveList=Fallout - Invalidation.bsa, Custom.bsa\r\n"));
		assert!(exported.contains("SLocalSavePath=Saves\\\r\n"));
		assert!(exported.contains("; untouched\r\n"));
		assert!(String::from_utf8(exec["Fallout.ini"].clone())
			.context(PrototypeError::Fixture)?
			.contains("SLocalSavePath=__mods_saves\\\r\n"));
		assert_eq!(
			export["FalloutCustom.ini"],
			b"[Archive]\r\n[Display]\r\nbOther=keep\r\n"
		);
		assert_eq!(export["plugins.txt"], b"*Example.esm\r\n");
		assert_eq!(canonical["Fallout.ini"], original);
		assert_eq!(canonical["FalloutCustom.ini"], custom);
		assert_eq!(
			archive_input(Some(b"[Archive]\nsArchiveList=\n"), original, None)?,
			"Fallout - Invalidation.bsa"
		);
		assert_eq!(
			archive_input(None, b"[Archive]\nsArchiveList=Fallout.bsa\n", None)?,
			"Fallout - Invalidation.bsa, Fallout.bsa"
		);
		assert_eq!(
			archive_input(
				None,
				b"[General]\na=1\n",
				Some(b"[Archive]\nsArchiveList=Default.bsa\n")
			)?,
			"Fallout - Invalidation.bsa, Default.bsa"
		);
		assert!(archive_input(None, b"[General]\na=1\n", None).is_err());
		assert!(archive_input(Some(&[0xff, 0xfe, 0x00]), original, None).is_err());
		Ok(())
	}

	#[test]
	fn reconcile_keeps_original_managed_values_and_child_settings_or_refuses_conflict() -> Trial<()> {
		let canonical = b"[General]\r\nSLocalSavePath=Saves\\\r\nbUseMyGamesDirectory=0\r\n[Archive]\r\nsArchiveList=User.bsa\r\nbInvalidateOlderFiles=0\r\nSInvalidationFile=user.txt\r\n[Display]\r\nbOther=old\r\n";
		let baseline = derived_ini(canonical, "Fallout - Invalidation.bsa, User.bsa", true, true)?;
		let child = String::from_utf8(baseline.clone())
			.context(PrototypeError::Fixture)?
			.replace("bOther=old", "bOther=child")
			.replace("SLocalSavePath=__mods_saves\\", "SLocalSavePath=private-edited\\");
		let merged = reconcile(canonical, &baseline, child.as_bytes(), canonical)?;
		assert_eq!(merged, b"[General]\r\nbUseMyGamesDirectory=0\r\nSLocalSavePath=Saves\\\r\n[Archive]\r\nbInvalidateOlderFiles=0\r\nSInvalidationFile=user.txt\r\nsArchiveList=User.bsa\r\n[Display]\r\nbOther=child\r\n");
		assert!(reconcile(canonical, &baseline, child.as_bytes(), b"concurrent edit").is_err());
		Ok(())
	}

	#[test]
	fn custom_archive_canonical_keys_survive_projection_and_child_edits() -> Trial<()> {
		let original = b"[Archive]\r\nsArchiveList=User.bsa\r\n[Display]\r\nquality=old\r\n";
		let baseline = derived_ini(original, "Fallout - Invalidation.bsa, User.bsa", true, false)?;
		assert_eq!(baseline, b"[Archive]\r\n[Display]\r\nquality=old\r\n");
		let child = b"[Archive]\r\n[Display]\r\nquality=new\r\n";
		assert_eq!(
			reconcile(original, &baseline, child, original)?,
			b"[Archive]\r\nsArchiveList=User.bsa\r\n[Display]\r\nquality=new\r\n"
		);
		assert_eq!(
			reconcile(
				b"[Archive]\nsArchiveList=\n",
				b"[Archive]\n",
				b"[Archive]\n",
				b"[Archive]\nsArchiveList=\n"
			)?,
			b"[Archive]\nsArchiveList=\n"
		);
		Ok(())
	}

	#[test]
	fn relocated_or_duplicate_controlled_keys_are_refused_even_with_same_count() -> Trial<()> {
		let original = b"[General]\nSLocalSavePath=Saves\\\n[Archive]\nsArchiveList=User.bsa\n";
		let baseline = derived_ini(original, "Fallout - Invalidation.bsa, User.bsa", true, true)?;
		let baseline = String::from_utf8(baseline).context(PrototypeError::Fixture)?;
		let relocated = baseline
			.replace("SLocalSavePath=__mods_saves\\\n", "")
			.replace("[Archive]\n", "[Archive]\nSLocalSavePath=private\\\n");
		assert!(reconcile(original, baseline.as_bytes(), relocated.as_bytes(), original).is_err());
		let duplicate = baseline.replace(
			"sArchiveList=Fallout - Invalidation.bsa, User.bsa",
			"SLocalSavePath=private\\",
		);
		assert!(reconcile(original, baseline.as_bytes(), duplicate.as_bytes(), original).is_err());
		Ok(())
	}

	#[test]
	fn staged_publication_preserves_bytes_mtime_and_partial_on_stop() -> Trial<()> {
		let scratch = TempDir::new().context(PrototypeError::Fixture)?;
		let source = scratch.path().join("source.ini");
		fs::write(&source, b"canonical").context(PrototypeError::Fixture)?;
		let when = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
		File::options()
			.write(true)
			.open(&source)
			.context(PrototypeError::Fixture)?
			.set_times(FileTimes::new().set_modified(when))
			.context(PrototypeError::Fixture)?;
		let generated = crate::empty_bsa_bytes();
		let bsa_source = scratch.path().join("generated.bsa");
		fs::write(&bsa_source, &generated).context(PrototypeError::Fixture)?;
		let files = [
			(source.clone(), "profile/Fallout.ini".to_owned(), b"derived".to_vec()),
			(bsa_source, "Data/Fallout - Invalidation.bsa".to_owned(), generated),
		];
		let final_path = scratch.path().join("export");
		assert!(stage_once(scratch.path(), &final_path, &files, Some(1)).is_err());
		assert!(!final_path.exists());
		assert_eq!(
			fs::read(scratch.path().join("disposable-partial/profile/Fallout.ini"))
				.context(PrototypeError::Fixture)?,
			b"derived"
		);
		assert_eq!(
			fs::metadata(scratch.path().join("disposable-partial/profile/Fallout.ini"))
				.context(PrototypeError::Fixture)?
				.modified()
				.context(PrototypeError::Fixture)?,
			when
		);
		fs::remove_dir_all(scratch.path().join("disposable-partial")).context(PrototypeError::Fixture)?;
		stage_once(scratch.path(), &final_path, &files, None)?;
		assert_eq!(
			fs::read(final_path.join("Data/Fallout - Invalidation.bsa"))
				.context(PrototypeError::Fixture)?,
			[
				b"BSA\0".as_slice(),
				&0x68_u32.to_le_bytes(),
				&36_u32.to_le_bytes(),
				&3_u32.to_le_bytes(),
				&[0; 20]
			]
			.concat()
		);
		assert_eq!(
			fs::metadata(final_path.join("profile/Fallout.ini"))
				.context(PrototypeError::Fixture)?
				.modified()
				.context(PrototypeError::Fixture)?,
			when
		);
		assert!(stage_once(scratch.path(), &final_path, &files, None).is_err());
		assert_eq!(
			fs::read(final_path.join("profile/Fallout.ini")).context(PrototypeError::Fixture)?,
			b"derived"
		);
		Ok(())
	}

	#[test]
	fn missing_timestamp_source_retains_io_cause() -> Trial<()> {
		let scratch = TempDir::new().context(PrototypeError::Fixture)?;
		let result = write_with_mtime(
			&scratch.path().join("missing"),
			&scratch.path().join("copied"),
			b"copied",
		);
		let Err(error) = result else {
			return Err(invalid("missing source must fail"));
		};
		assert!(error.iter_reports().any(|report| report
			.downcast_current_context::<std::io::Error>()
			.is_some_and(|cause| cause.kind() == std::io::ErrorKind::NotFound)));
		Ok(())
	}

	#[test]
	fn model_keeps_temporary_ini_until_known_full_drain() -> Trial<()> {
		let scratch = TempDir::new().context(PrototypeError::Fixture)?;
		let temporary = scratch.path().join("execution.ini");
		fs::write(&temporary, b"temporary derived INI").context(PrototypeError::Fixture)?;
		let mut root_exited = true;
		let mut descendants_drained = false;
		if root_exited && descendants_drained {
			fs::remove_file(&temporary).context(PrototypeError::Fixture)?;
		}
		assert_eq!(
			fs::read(&temporary).context(PrototypeError::Fixture)?,
			b"temporary derived INI"
		);
		root_exited = true;
		descendants_drained = true;
		let mut preservation_succeeded = false;
		if root_exited && descendants_drained && preservation_succeeded {
			fs::remove_file(&temporary).context(PrototypeError::Fixture)?;
		}
		assert_eq!(
			fs::read(&temporary).context(PrototypeError::Fixture)?,
			b"temporary derived INI"
		);
		preservation_succeeded = true;
		if root_exited && descendants_drained && preservation_succeeded {
			fs::remove_file(&temporary).context(PrototypeError::Fixture)?;
		}
		assert!(!temporary.exists());
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::MAX_PROFILE_BYTES;
	use super::MAX_SAVE_ENTRIES;
	use super::PROFILE_FILES;
	use super::general_values;
	use super::is_activatable_plugin_name;
	use super::remove_unavailable_lines;
	use super::stage_profile;
	use super::validate_saves;
	use super::validate_saves_inner;
	use crate::safe_fs::EntryBudget;
	use crate::safe_fs::MAX_TRAVERSAL_DEPTH;
	use crate::safe_fs::SafeDir;
	use application::ErrorCode;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileFileDisposition;
	use application::ports::ProfileSource;
	use domain::case_fold_key;
	use std::collections::HashSet;
	use std::error::Error;
	use std::fs;
	use std::io;
	use std::result::Result as StdResult;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn profile_resource_caps_are_deliberate() {
		assert_eq!(MAX_PROFILE_BYTES, 16 * 1024 * 1024);
		assert_eq!(MAX_SAVE_ENTRIES, 100_000);
		assert_eq!(MAX_TRAVERSAL_DEPTH, 64);
	}

	#[test]
	fn save_validation_rejects_total_entry_cap() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("one.fos"), b"one")?;
		fs::write(temp.path().join("two.fos"), b"two")?;
		let saves = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe saves directory open failed")?;
		let mut budget = EntryBudget::new(1);
		let result = validate_saves_inner(&saves, &CancellationToken::new(), &mut budget, MAX_TRAVERSAL_DEPTH);

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::EnvironmentInvalid
		));
		Ok(())
	}

	#[test]
	fn save_validation_rejects_exhausted_depth_with_io_cause() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("save.fos"), b"contents")?;
		let saves = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe saves directory open failed")?;
		let mut budget = EntryBudget::new(MAX_SAVE_ENTRIES);

		let Err(error) = validate_saves_inner(&saves, &CancellationToken::new(), &mut budget, 0) else {
			return Err("exhausted depth must reject a save entry".into());
		};

		assert!(error.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
		}));
		Ok(())
	}

	#[test]
	fn large_save_validation_opens_without_buffering_contents() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let save = fs::File::create(temp.path().join("large.fos"))?;
		save.set_len(256 * 1024 * 1024)?;
		let saves = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe saves directory open failed")?;

		validate_saves(&saves, &CancellationToken::new()).map_err(|_| "large save validation failed")?;
		assert_eq!(fs::metadata(temp.path().join("large.fos"))?.len(), 256 * 1024 * 1024);
		Ok(())
	}

	#[test]
	fn cancelled_save_validation_is_typed_and_non_mutating() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::create_dir_all(temp.path().join("nested/deeper"))?;
		let save_path = temp.path().join("nested/deeper/save.fos");
		fs::write(&save_path, b"unchanged save")?;
		let saves = SafeDir::open_absolute(&temp.path().canonicalize()?)
			.map_err(|_| "safe saves directory open failed")?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = validate_saves(&saves, &cancellation);

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::OperationCancelled
		));
		assert_eq!(fs::read(save_path)?, b"unchanged save");
		Ok(())
	}

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
							"Fallout - Invalidation.bſa\r\n",
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
	fn unavailable_plugin_names_use_simple_unicode_case_folded_keys() {
		let unavailable = HashSet::from([case_fold_key("ÉΣ.ESP")]);

		assert_eq!(
			remove_unavailable_lines("éς.esp\r\nKeep.esm\r\n", &unavailable),
			"Keep.esm\r\n"
		);
	}

	#[test]
	fn plugin_extensions_are_activatable_case_insensitively() {
		for name in ["Example.esp", "Example.ESM", "Example.EsL", "Example.eſp"] {
			assert!(is_activatable_plugin_name(name));
		}
	}

	#[test]
	fn invalid_plugin_entries_block_publication() -> StdResult<(), Box<dyn Error>> {
		for (list, value) in [
			("plugins.txt", b"*Active.esm\r\n".as_slice()),
			("plugins.txt", b"folder\\Bad.esp\r\n"),
			("plugins.txt", b"CON.esm\r\n"),
			("plugins.txt", b"Unsupported.esx\r\n"),
			("loadorder.txt", b"Unsupported.esx\r\n"),
		] {
			let temp = TempDir::new()?;
			let files = PROFILE_FILES
				.into_iter()
				.map(|name| {
					ProfileSourceFixture::source(
						name,
						if name == list { Some(value.to_vec()) } else { None },
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
