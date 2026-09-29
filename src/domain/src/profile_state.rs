use crate::case_fold_key;

/// Returns the last assigned value for each General sTestFile1 through sTestFile10 slot.
///
/// Each INI must be interpreted independently: a slot in one file does not
/// suppress activation from another. Values are trimmed but not validated as
/// plugin names; callers retain their own eligibility policies.
pub fn profile_test_file_slots(text: &str) -> [Option<&str>; 10] {
	let mut slots = [None; 10];
	let mut section = "";
	for line in text.lines() {
		let line = line.trim();
		if let Some(header) = line.strip_prefix('[').and_then(|value| value.strip_suffix(']')) {
			section = header.trim();
			continue;
		}
		if !section.eq_ignore_ascii_case("General") {
			continue;
		}
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};
		let key = key.trim();
		let Some(slot) = (0..10).find(|slot| key.eq_ignore_ascii_case(&format!("sTestFile{}", slot + 1)))
		else {
			continue;
		};

		slots[slot] = Some(value.trim());
	}

	slots
}

#[cfg(test)]
mod tests {
	use super::ProfileIniPurpose;
	use super::canonical_profile_routing_valid;
	use super::derive_profile_ini;
	use super::preserve_profile_ini_keys;
	use super::profile_archive_list;
	use super::profile_ini_valid;
	use super::profile_test_file_slots;

	const MASTER_MISMATCH_WARNING: &str = concat!(
		"SMasterMismatchWarning=One of the files that \"%s\" is dependent on has changed since the last save.\r\n",
		"This may result in errors. Saving again will clear this message\r\n",
		"but not necessarily fix any errors.\r\n"
	);

	#[test]
	fn accepts_the_game_multi_line_warning_and_keeps_it_in_every_copy() {
		let canonical = format!(
			"[General]\r\n{MASTER_MISMATCH_WARNING}bUseMyGamesDirectory=1\r\nSLocalSavePath=Saves\\\r\n[Archive]\r\nsArchiveList=User.bsa\r\n"
		);
		assert!(profile_ini_valid(&canonical));

		for purpose in [
			ProfileIniPurpose::Canonical,
			ProfileIniPurpose::Execution,
			ProfileIniPurpose::Export,
		] {
			for name in ["Fallout.ini", "FalloutPrefs.ini", "FalloutCustom.ini"] {
				let derived = derive_profile_ini(name, &canonical, purpose, "User.bsa");
				assert!(derived.contains(MASTER_MISMATCH_WARNING));
			}
		}

		let child = canonical.replace("User.bsa", "Child.bsa");
		assert!(preserve_profile_ini_keys(&canonical, &child).contains(MASTER_MISMATCH_WARNING));
	}

	#[test]
	fn still_rejects_control_characters_malformed_headers_and_empty_keys() {
		for text in [
			"[General]\r\ntext with a \u{1} control\r\n",
			"[General\r\n",
			"[ ]\r\n",
			"[General]\r\n=value\r\n",
		] {
			assert!(!profile_ini_valid(text), "{text:?}");
		}
	}

	#[test]
	fn canonical_and_derived_copies_keep_distinct_owned_keys() {
		let original =
			"[Archive]\r\nsArchiveList=User.bsa\r\nbInvalidateOlderFiles=0\r\n[Display]\r\nvalue=keep\r\n";
		let canonical = derive_profile_ini("Fallout.ini", original, ProfileIniPurpose::Canonical, "");
		assert!(canonical_profile_routing_valid("Fallout.ini", &canonical));
		assert!(canonical.contains("bInvalidateOlderFiles=0"));
		let execution = derive_profile_ini(
			"FalloutCustom.ini",
			&canonical,
			ProfileIniPurpose::Execution,
			"User.bsa, Fallout - Invalidation.bsa",
		);
		assert!(execution.contains("SLocalSavePath=__mods_saves\\"));
		assert_eq!(execution.matches("Fallout - Invalidation.bsa").count(), 1);
		let child = execution.replace("value=keep", "value=edited");
		let preserved = preserve_profile_ini_keys(&canonical, &child);
		assert!(canonical_profile_routing_valid("Fallout.ini", &preserved));
		assert_eq!(profile_archive_list(&preserved), Some("User.bsa"));
		assert!(preserved.contains("value=edited"));
		assert!(!preserved.contains("__mods_saves"));
		assert!(preserved.contains("bInvalidateOlderFiles=0"));
		assert!(!preserved.contains("SInvalidationFile="));
	}

	#[test]
	fn last_empty_archive_assignment_stops_fallback_and_custom_derivation_strips_overrides() {
		let custom = "[Archive]\nsArchiveList=First.bsa\n[ARCHIVE]\nSARCHIVELIST=\n[Other]\nSLocalSavePath=bad\nsetting=keep";
		assert_eq!(profile_archive_list(custom), Some(""));
		let derived = derive_profile_ini("FalloutCustom.ini", custom, ProfileIniPurpose::Export, "");
		assert!(!derived.to_ascii_lowercase().contains("sarchivelist"));
		assert!(!derived.contains("SLocalSavePath"));
		assert!(derived.ends_with("setting=keep"));
	}

	#[test]
	fn preservation_restores_duplicate_relocated_and_preamble_keys() {
		let canonical = "sArchiveList=preamble\n[Archive]\nsArchiveList=first\nsArchiveList=last\n[Other]\nSInvalidationFile=original\n";
		let child = "[Display]\nother=edit\nsArchiveList=injected\nSInvalidationFile=injected\n";
		let preserved = preserve_profile_ini_keys(canonical, child);
		assert!(preserved.starts_with("sArchiveList=preamble\n"));
		assert_eq!(profile_archive_list(&preserved), Some("last"));
		assert!(preserved.contains("[Other]\nSInvalidationFile=original"));
		assert!(preserved.contains("other=edit"));
		assert!(!preserved.contains("injected"));
	}

	#[test]
	fn settles_exact_numbered_slots_with_last_assignment_and_case_insensitive_keys() {
		let slots = profile_test_file_slots(concat!(
			"[ general ]\nsTestFile1=Old.esp\nsTESTfile1= New.ESP \n",
			"sTestFile2=Old.esp\nsTestFile2=\n",
			"sTestFile10=Ten.esl\nsTestFile01=Ignored.esp\nsTestFile11=Ignored.esp\n",
			";sTestFile1=Ignored.esp\n#sTestFile1=Ignored.esp\n",
			"[Other]\nsTestFile1=Ignored.esp\n"
		));
		assert_eq!(
			slots,
			[
				Some("New.ESP"),
				Some(""),
				None,
				None,
				None,
				None,
				None,
				None,
				None,
				Some("Ten.esl")
			]
		);
	}

	#[test]
	fn independent_inis_keep_each_effective_assignment() {
		let inis = [
			"[General]\nsTestFile1=First.esp\nsTestFile1=Final.esp",
			"[General]\nsTestFile1=Other.esm",
		];
		let values = inis
			.into_iter()
			.flat_map(profile_test_file_slots)
			.flatten()
			.collect::<Vec<_>>();
		assert_eq!(values, ["Final.esp", "Other.esm"]);
	}
}

const ARCHIVE_KEYS: [&str; 3] = ["bInvalidateOlderFiles", "SInvalidationFile", "sArchiveList"];
const ROUTING_KEYS: [&str; 2] = ["bUseMyGamesDirectory", "SLocalSavePath"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileIniPurpose {
	Canonical,
	Execution,
	Export,
}

/// Validates the line-oriented INI subset consumed by the game's profile files.
/// The editor below preserves text because general INI serializers discard
/// comments, duplicate assignments, spelling, and newline choices.
///
/// Text lines without an assignment are valid. The game's INI reader ignores
/// them, and the game's own `Fallout.ini` and `FalloutPrefs.ini` continue the
/// `SMasterMismatchWarning` value on such lines. Control characters, empty or
/// unterminated section headers, and assignments with an empty key stay invalid.
pub fn profile_ini_valid(text: &str) -> bool {
	text.lines().all(|line| {
		let line = line.trim();
		if line.is_empty() || line.starts_with([';', '#']) {
			return true;
		}
		if line.chars()
			.any(|character| character.is_control() && character != '\t')
		{
			return false;
		}
		if line.starts_with('[') {
			return ini_section(line).is_some_and(|section| !section.is_empty());
		}
		line.split_once('=').is_none_or(|(key, _)| !key.trim().is_empty())
	})
}

/// Returns the last Archive assignment, including an explicitly empty value.
pub fn profile_archive_list(text: &str) -> Option<&str> {
	let mut section = "";
	let mut value = None;
	for line in text.lines() {
		if let Some(header) = ini_section(line) {
			section = header;
		} else if section.eq_ignore_ascii_case("Archive")
			&& let Some((key, assigned)) = line.split_once('=')
			&& key.trim().eq_ignore_ascii_case("sArchiveList")
		{
			value = Some(assigned.trim());
		}
	}
	value
}

/// Canonical routing must not admit private or conflicting overrides. Archive
/// settings remain user-owned and do not participate in this validation.
pub fn canonical_profile_routing_valid(name: &str, text: &str) -> bool {
	let mut section = "";
	let mut counts = [0; 2];
	for line in text.lines() {
		if let Some(header) = ini_section(line) {
			section = header;
			continue;
		}
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};
		for (index, managed) in ROUTING_KEYS.iter().enumerate() {
			if !key.trim().eq_ignore_ascii_case(managed) {
				continue;
			}
			counts[index] += 1;
			if !name.eq_ignore_ascii_case("Fallout.ini")
				|| !section.eq_ignore_ascii_case("General")
				|| value.trim() != ["1", "Saves\\"][index]
			{
				return false;
			}
		}
	}

	if name.eq_ignore_ascii_case("Fallout.ini") {
		counts == [1; 2]
	} else {
		counts == [0; 2]
	}
}

/// The archive list must be selected from untouched canonical inputs before
/// deriving any copies. Execution callers materialize an absent FalloutCustom.ini.
/// Execution overrides require JIP LN NVSE; GECK consumption is not supported here.
pub fn derive_profile_ini(name: &str, text: &str, purpose: ProfileIniPurpose, archive_list: &str) -> String {
	if !["Fallout.ini", "FalloutPrefs.ini", "FalloutCustom.ini"]
		.iter()
		.any(|candidate| name.eq_ignore_ascii_case(candidate))
	{
		return text.to_owned();
	}

	if purpose == ProfileIniPurpose::Execution && !name.eq_ignore_ascii_case("FalloutCustom.ini") {
		return text.to_owned();
	}

	let fallout = name.eq_ignore_ascii_case("Fallout.ini") || purpose == ProfileIniPurpose::Execution;
	let archive =
		purpose != ProfileIniPurpose::Canonical && (fallout || name.eq_ignore_ascii_case("FalloutCustom.ini"));
	let mut output = filter_ini_keys(text, |key| {
		ROUTING_KEYS.iter().any(|item| key.eq_ignore_ascii_case(item))
			|| (archive && ARCHIVE_KEYS.iter().any(|item| key.eq_ignore_ascii_case(item)))
	});
	let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
	if !fallout {
		return output;
	}

	let route = if purpose == ProfileIniPurpose::Execution {
		"__mods_saves\\"
	} else {
		"Saves\\"
	};
	append_ini_section(
		&mut output,
		"General",
		&["bUseMyGamesDirectory=1".to_owned(), format!("SLocalSavePath={route}")],
		newline,
	);
	if archive {
		let mut archives: Vec<_> = archive_list
			.split(',')
			.map(str::trim)
			.filter(|value| !value.is_empty() && case_fold_key(value) != "fallout - invalidation.bsa")
			.collect();
		if purpose == ProfileIniPurpose::Execution {
			archives.push("Fallout - Invalidation.bsa");
		} else {
			archives.insert(0, "Fallout - Invalidation.bsa");
		}

		append_ini_section(
			&mut output,
			"Archive",
			&[
				"bInvalidateOlderFiles=1".to_owned(),
				"SInvalidationFile=".to_owned(),
				format!("sArchiveList={}", archives.join(", ")),
			],
			newline,
		);
	}
	output
}

/// Restores every canonical managed assignment, including duplicate and
/// relocated keys. Other child edits retain their original text and newlines.
pub fn preserve_profile_ini_keys(canonical: &str, child: &str) -> String {
	let managed = |key: &str| {
		ROUTING_KEYS
			.iter()
			.chain(ARCHIVE_KEYS.iter())
			.any(|item| key.eq_ignore_ascii_case(item))
	};
	let mut output = filter_ini_keys(child, managed);
	let mut preamble = String::new();
	let newline = if child.contains("\r\n") { "\r\n" } else { "\n" };
	let mut section = "";
	for line in canonical.lines() {
		if let Some(header) = ini_section(line) {
			section = header;
			continue;
		}
		if let Some((key, _)) = line.split_once('=')
			&& managed(key.trim())
		{
			if section.is_empty() {
				preamble.push_str(line);
				preamble.push_str(newline);
				continue;
			}
			append_ini_section(&mut output, section, &[line.to_owned()], newline);
		}
	}

	preamble.push_str(&output);
	preamble
}

fn ini_section(line: &str) -> Option<&str> {
	line.trim().strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

fn filter_ini_keys(text: &str, remove: impl Fn(&str) -> bool) -> String {
	text.split_inclusive('\n')
		.filter(|line| line.split_once('=').is_none_or(|(key, _)| !remove(key.trim())))
		.collect()
}

fn append_ini_section(output: &mut String, section: &str, values: &[String], newline: &str) {
	let mut offset = 0;
	let mut found = false;
	let mut insertion = output.len();
	for line in output.split_inclusive('\n') {
		if let Some(header) = ini_section(line) {
			if found {
				insertion = offset;
				break;
			}
			found = header.eq_ignore_ascii_case(section);
		}
		offset += line.len();
	}

	let mut added = String::new();
	if insertion > 0 && !output[..insertion].ends_with('\n') {
		added.push_str(newline);
	}
	if !found {
		added.push('[');
		added.push_str(section);
		added.push(']');
		added.push_str(newline);
	}
	for value in values {
		added.push_str(value);
		added.push_str(newline);
	}
	output.insert_str(insertion, &added);
}
