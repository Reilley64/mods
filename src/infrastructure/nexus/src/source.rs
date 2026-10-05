use crate::types::NexusRequest;
use application::ErrorMarker;
use reqwest::Url;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub(crate) fn parse(source: &str, explicit_file: Option<u64>) -> Result<NexusRequest, ErrorMarker> {
	let url = Url::parse(source).context(ErrorMarker::nexus_source_invalid())?;
	if url.scheme() != "https"
		|| !matches!(url.host_str(), Some("www.nexusmods.com" | "nexusmods.com"))
		|| url.port().is_some()
		|| !url.username().is_empty()
		|| url.password().is_some()
		|| url.fragment().is_some()
	{
		return Err(report!(ErrorMarker::nexus_source_invalid()));
	}

	let segments = url.path().trim_end_matches('/').split('/').collect::<Vec<_>>();
	if segments.len() != 4 || segments[1] != "newvegas" || segments[2] != "mods" {
		return Err(report!(ErrorMarker::nexus_source_invalid()));
	}

	let mod_id = segments[3]
		.parse::<u64>()
		.context(ErrorMarker::nexus_source_invalid())?;
	let mut file_id = None;
	for (key, value) in url.query_pairs() {
		match key.as_ref() {
			"file_id" => {
				if file_id.is_some() {
					return Err(report!(ErrorMarker::nexus_source_invalid()));
				}
				file_id = Some(value.parse::<u64>().context(ErrorMarker::nexus_source_invalid())?);
			}
			"tab" if matches!(value.as_ref(), "files" | "description") => {}
			"nmm" if value == "1" => {}
			_ => return Err(report!(ErrorMarker::nexus_source_invalid())),
		}
	}
	if mod_id == 0
		|| file_id == Some(0)
		|| explicit_file == Some(0)
		|| matches!((file_id, explicit_file), (Some(left), Some(right)) if left != right)
	{
		return Err(report!(ErrorMarker::nexus_source_invalid()));
	}

	Ok(NexusRequest {
		game_domain: "newvegas".into(),
		mod_id,
		file_id: explicit_file.or(file_id),
	})
}

#[cfg(test)]
mod tests {
	use super::parse;
	#[test]
	fn accepts_pages_and_explicit_files_without_guessing_conflicts() {
		assert_eq!(
			parse("https://www.nexusmods.com/newvegas/mods/42?tab=files&file_id=7", None)
				.ok()
				.and_then(|r| r.file_id),
			Some(7)
		);
		assert_eq!(
			parse("https://nexusmods.com/newvegas/mods/42", Some(8))
				.ok()
				.and_then(|r| r.file_id),
			Some(8)
		);
		for source in [
			"https://nexusmods.com/skyrim/mods/42",
			"https://evil.test/newvegas/mods/42",
			"http://nexusmods.com/newvegas/mods/42",
			"https://nexusmods.com/newvegas/mods/0",
			"https://nexusmods.com/newvegas/mods/42?file_id=7&file_id=8",
			"https://nexusmods.com/newvegas/mods/42?key=secret",
		] {
			assert!(parse(source, None).is_err());
		}
		assert!(parse("https://nexusmods.com/newvegas/mods/42?file_id=7", Some(8)).is_err());
	}
}
