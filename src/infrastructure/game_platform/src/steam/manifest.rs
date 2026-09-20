use super::model::SteamMetadataParseError;
use super::model::exactly_one_object;
use super::model::exactly_one_text;
use super::parser::parse_key_values;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::Component;
use std::path::Path;

pub(super) fn fields(text: &str) -> Result<(u32, String, u64), SteamMetadataParseError> {
	let root = parse_key_values(text)?;
	let app_state = exactly_one_object(&root, "AppState").ok_or_else(|| report!(SteamMetadataParseError))?;
	let app_id = exactly_one_text(app_state, "appid")
		.ok_or_else(|| report!(SteamMetadataParseError))?
		.parse()
		.context(SteamMetadataParseError)?;
	let install_dir = exactly_one_text(app_state, "installdir")
		.filter(|value| !value.is_empty())
		.ok_or_else(|| report!(SteamMetadataParseError))?
		.to_owned();
	let build = exactly_one_text(app_state, "buildid")
		.ok_or_else(|| report!(SteamMetadataParseError))?
		.parse()
		.context(SteamMetadataParseError)?;
	Ok((app_id, install_dir, build))
}

pub(super) fn is_install_directory_name(value: &str) -> bool {
	!matches!(value, "" | "." | "..")
		&& !value.contains(['/', '\\'])
		&& matches!(
			Path::new(value).components().collect::<Vec<_>>().as_slice(),
			[Component::Normal(_)]
		)
}
