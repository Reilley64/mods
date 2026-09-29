use crate::active_code_page::decode as decode_active_code_page;
use crate::active_code_page::encode as encode_active_code_page;
use crate::files::read_optional;
use crate::snapshot::visible_plugins;
use application::ErrorMarker;
use application::ports::InitializationProfileSources;
use application::ports::ProfileFileDisposition;
use application::ports::ProfileFileRecord;
use domain::GameBinding;
use domain::ProfileIniPurpose;
use domain::canonical_profile_routing_valid;
use domain::case_fold_key;
use domain::derive_profile_ini;
use encoding_rs::WINDOWS_1252;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::str::from_utf8;
use tokio::fs::create_dir;
use tokio::fs::metadata;
use tokio::fs::read;
use tokio::fs::read_dir;
use tokio::fs::write;
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

pub(crate) async fn write_initial_profile(
	profile: &Path,
	sources: &InitializationProfileSources,
	cancellation: &CancellationToken,
) -> Result<Vec<ProfileFileRecord>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	create_dir(profile.join("saves"))
		.await
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

	let mut records = Vec::with_capacity(PROFILE_FILES.len());
	for name in PROFILE_FILES {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let contents = source_map.get(name).copied().flatten();
		let (output, disposition) = match (name, contents) {
			("Fallout.ini", Some(contents)) => {
				(Some(canonical_ini(name, contents)?), ProfileFileDisposition::Imported)
			}
			("Fallout.ini", None) => (
				Some(canonical_ini(name, &sources.fallout_default_ini)?),
				ProfileFileDisposition::SeededFromGame,
			),
			("FalloutPrefs.ini", Some(contents)) => {
				(Some(canonical_ini(name, contents)?), ProfileFileDisposition::Imported)
			}
			("FalloutCustom.ini", Some(contents)) => {
				(Some(canonical_ini(name, contents)?), ProfileFileDisposition::Imported)
			}
			("plugins.txt" | "loadorder.txt", None) => {
				(Some(Vec::new()), ProfileFileDisposition::CreatedEmpty)
			}
			(_, Some(contents)) => (Some(contents.to_vec()), ProfileFileDisposition::Imported),
			(_, None) => (None, ProfileFileDisposition::Absent),
		};
		if let Some(output) = output {
			write(profile.join(name), &output)
				.await
				.context(ErrorMarker::environment_root_unsafe())?;
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
		}
		records.push(ProfileFileRecord { name, disposition });
	}
	write(profile.join("modlist.txt"), b"")
		.await
		.context(ErrorMarker::environment_root_unsafe())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	validate_profile(profile, cancellation).await?;
	Ok(records)
}

pub(crate) async fn validate_profile(profile: &Path, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	validate_profile_files(profile, true, cancellation).await
}

pub(crate) async fn validate_profile_files(
	profile: &Path,
	require_empty: bool,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	validate_profile_mode(profile, require_empty, cancellation).await
}

async fn validate_profile_mode(
	profile: &Path,
	require_empty: bool,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let allowed = PROFILE_FILES.iter().copied().chain(["modlist.txt", "saves"]);
	let mut expected = allowed.collect::<HashSet<_>>();
	let mut entries = read_dir(profile)
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let name = entry.file_name();
		let name = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if !expected.remove(name) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}

		let metadata = metadata(entry.path())
			.await
			.context(ErrorMarker::environment_invalid(None))?;
		if (name == "saves" && !metadata.is_dir()) || (name != "saves" && !metadata.is_file()) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	for required in ["Fallout.ini", "plugins.txt", "loadorder.txt", "modlist.txt", "saves"] {
		if expected.contains(required) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}

	let saves = profile.join("saves");
	if require_empty
		&& read_dir(&saves)
			.await
			.context(ErrorMarker::environment_invalid(None))?
			.next_entry()
			.await
			.context(ErrorMarker::environment_invalid(None))?
			.is_some()
	{
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	validate_saves(&saves, cancellation).await?;

	let fallout = read_regular_file(profile, "Fallout.ini", cancellation).await?;
	let text = decode(&fallout)?.0;
	if !canonical_profile_routing_valid("Fallout.ini", &text) {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	for name in ["FalloutPrefs.ini", "FalloutCustom.ini"] {
		let Some(bytes) = read_optional(&profile.join(name))
			.await
			.context(ErrorMarker::environment_invalid(None))?
		else {
			continue;
		};
		let text = decode(&bytes)?.0;
		if !canonical_profile_routing_valid(name, &text) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}

	for (name, utf8) in [("plugins.txt", false), ("loadorder.txt", true)] {
		let Some(bytes) = read_optional(&profile.join(name))
			.await
			.context(ErrorMarker::environment_invalid(None))?
		else {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		};
		validate_plugin_list(&bytes, utf8, true)?;
	}

	let modlist = read_regular_file(profile, "modlist.txt", cancellation).await?;
	if require_empty && !modlist.is_empty() {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}
	Ok(())
}

/// Requires every save entry to be a directory or regular file.
async fn validate_saves(directory: &Path, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let mut entries = read_dir(directory)
		.await
		.context(ErrorMarker::environment_invalid(None))?;
	while let Some(entry) = entries
		.next_entry()
		.await
		.context(ErrorMarker::environment_invalid(None))?
	{
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let metadata = metadata(entry.path())
			.await
			.context(ErrorMarker::environment_invalid(None))?;
		if metadata.is_dir() {
			Box::pin(validate_saves(&entry.path(), cancellation)).await?;
		} else if !metadata.is_file() {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
	}
	Ok(())
}

/// Updates `plugins.txt` and `loadorder.txt` in place after an enabled replacement.
///
/// `before` holds the plugins that were visible before the old mod files were removed.
pub(crate) async fn update_plugin_lists(
	root: &Path,
	binding: &GameBinding,
	before: &HashMap<String, String>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let after = visible_plugins(root, binding, cancellation).await?;
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
	let profile = root.join("profile");

	let plugins_bytes = read_regular_file(&profile, "plugins.txt", cancellation).await?;
	let plugins_text = decode_active_code_page(&plugins_bytes)?;
	let plugins_output = remove_unavailable_lines(&plugins_text, &unavailable);

	if plugins_output != plugins_text {
		let bytes = encode_active_code_page(&plugins_output)?;
		write(profile.join("plugins.txt"), &bytes)
			.await
			.context(ErrorMarker::transaction_failure())?;
	}

	let loadorder_bytes = read_regular_file(&profile, "loadorder.txt", cancellation).await?;
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
		write(profile.join("loadorder.txt"), loadorder_output.as_bytes())
			.await
			.context(ErrorMarker::transaction_failure())?;
	}
	Ok(())
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

fn validate_plugin_list(bytes: &[u8], utf8: bool, allow_light_plugins: bool) -> Result<(), ErrorMarker> {
	let text = if utf8 {
		from_utf8(bytes)
			.context(ErrorMarker::environment_invalid(None))?
			.to_owned()
	} else {
		decode_active_code_page(bytes)?
	};
	validate_plugin_text(&text, allow_light_plugins)
}

pub(crate) fn validate_plugin_text(text: &str, allow_light_plugins: bool) -> Result<(), ErrorMarker> {
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

async fn read_regular_file(
	directory: &Path,
	name: &str,
	cancellation: &CancellationToken,
) -> Result<Vec<u8>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	read(directory.join(name))
		.await
		.context(ErrorMarker::environment_invalid(None))
}

fn canonical_ini(name: &str, bytes: &[u8]) -> Result<Vec<u8>, ErrorMarker> {
	let (text, encoding) = decode(bytes)?;
	encode(
		&derive_profile_ini(name, &text, ProfileIniPurpose::Canonical, ""),
		encoding,
	)
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

pub(crate) fn encode(text: &str, encoding: Encoding) -> Result<Vec<u8>, ErrorMarker> {
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

#[cfg(test)]
mod tests {
	use super::PROFILE_FILES;
	use super::is_activatable_plugin_name;
	use super::remove_unavailable_lines;
	use super::validate_saves;
	use super::write_initial_profile;
	use application::ErrorCode;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileFileDisposition;
	use application::ports::ProfileSource;
	use domain::canonical_profile_routing_valid;
	use domain::case_fold_key;
	use std::collections::HashSet;
	use std::error::Error;
	use std::fs;
	use std::result::Result as StdResult;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn large_save_validation_opens_without_buffering_contents() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let save = fs::File::create(temp.path().join("large.fos"))?;
		save.set_len(256 * 1024 * 1024)?;
		let saves = temp.path().canonicalize()?;

		validate_saves(&saves, &CancellationToken::new())
			.await
			.map_err(|_| "large save validation failed")?;
		assert_eq!(fs::metadata(temp.path().join("large.fos"))?.len(), 256 * 1024 * 1024);
		Ok(())
	}

	#[tokio::test]
	async fn cancelled_save_validation_is_typed_and_non_mutating() -> StdResult<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		fs::create_dir_all(temp.path().join("nested/deeper"))?;
		let save_path = temp.path().join("nested/deeper/save.fos");
		fs::write(&save_path, b"unchanged save")?;
		let saves = temp.path().canonicalize()?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let result = validate_saves(&saves, &cancellation).await;

		assert!(matches!(
			result,
			Err(report) if report.current_context().code() == ErrorCode::OperationCancelled
		));
		assert_eq!(fs::read(save_path)?, b"unchanged save");
		Ok(())
	}

	#[tokio::test]
	async fn stages_import_seed_empty_absent_and_never_saves() -> StdResult<(), Box<dyn Error>> {
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
		let profile = temp.path().canonicalize()?;
		let records = write_initial_profile(&profile, &sources, &CancellationToken::new())
			.await
			.map_err(|_| "stage failed")?;
		assert_eq!(records[0].disposition, ProfileFileDisposition::SeededFromGame);
		assert_eq!(records[2].disposition, ProfileFileDisposition::Imported);
		assert_eq!(records[5].disposition, ProfileFileDisposition::Imported);
		assert_eq!(records[6].disposition, ProfileFileDisposition::CreatedEmpty);
		assert!(fs::read_dir(temp.path().join("saves"))?.next().is_none());
		let fallout = fs::read_to_string(temp.path().join("Fallout.ini"))?;
		assert!(fallout.contains("sArchiveList=Default.bsa"));
		assert!(!fallout.contains("bInvalidateOlderFiles"));
		assert!(fallout.contains("SLocalSavePath=Saves\\"));
		let custom = fs::read_to_string(temp.path().join("FalloutCustom.ini"))?;
		assert!(custom.contains("sArchiveList=Custom.bsa"));
		assert!(!custom.to_ascii_lowercase().contains("slocalsavepath"));
		Ok(())
	}

	#[tokio::test]
	async fn stages_exact_save_routing_and_removes_conflicts_from_imports() -> StdResult<(), Box<dyn Error>> {
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
		let profile = temp.path().canonicalize()?;
		write_initial_profile(&profile, &sources, &CancellationToken::new())
			.await
			.map_err(|_| "stage failed")?;

		let fallout = fs::read_to_string(temp.path().join("Fallout.ini"))?;
		assert!(canonical_profile_routing_valid("Fallout.ini", &fallout));
		assert!(fallout.contains("; keep fallout comment"));
		assert!(fallout.contains("bOther=keep"));
		assert_eq!(fallout.to_ascii_lowercase().matches("busemygamesdirectory").count(), 1);
		assert_eq!(fallout.to_ascii_lowercase().matches("slocalsavepath").count(), 1);
		assert_eq!(fallout.matches("bInvalidateOlderFiles=0").count(), 2);
		assert!(fallout.contains("sArchiveList=Misplaced.bsa"));
		for name in ["FalloutPrefs.ini", "FalloutCustom.ini"] {
			let text = fs::read_to_string(temp.path().join(name))?;
			assert!(canonical_profile_routing_valid(name, &text));
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

	#[tokio::test]
	async fn seeds_exact_save_routing_from_conflicting_fallout_ini() -> StdResult<(), Box<dyn Error>> {
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
		let profile = temp.path().canonicalize()?;
		write_initial_profile(&profile, &sources, &CancellationToken::new())
			.await
			.map_err(|_| "stage failed")?;

		let fallout = fs::read_to_string(temp.path().join("Fallout.ini"))?;
		assert!(canonical_profile_routing_valid("Fallout.ini", &fallout));
		Ok(())
	}

	#[tokio::test]
	async fn unavailable_plugin_names_use_simple_unicode_case_folded_keys() {
		let unavailable = HashSet::from([case_fold_key("ÉΣ.ESP")]);

		assert_eq!(
			remove_unavailable_lines("éς.esp\r\nKeep.esm\r\n", &unavailable),
			"Keep.esm\r\n"
		);
	}

	#[tokio::test]
	async fn plugin_extensions_are_activatable_case_insensitively() {
		for name in ["Example.esp", "Example.ESM", "Example.EsL", "Example.eſp"] {
			assert!(is_activatable_plugin_name(name));
		}
	}

	#[tokio::test]
	async fn invalid_plugin_entries_block_publication() -> StdResult<(), Box<dyn Error>> {
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
			let profile = temp.path().canonicalize()?;
			assert!(write_initial_profile(&profile, &sources, &CancellationToken::new())
				.await
				.is_err());
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
