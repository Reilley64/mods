use crate::ErrorMarker;
use rootcause::Result;
use rootcause::report;

pub(super) fn shortcut_name(value: &str, explicit: bool) -> Result<String, ErrorMarker> {
	let forbidden = |character: char| character < ' ' || r#"<>:"/\|?*"#.contains(character);
	let mut name = if explicit {
		value.to_owned()
	} else {
		value.chars()
			.map(|character| if forbidden(character) { '_' } else { character })
			.collect::<String>()
			.trim_end_matches([' ', '.'])
			.to_owned()
	};

	if !explicit {
		while name.encode_utf16().count() > 251 {
			name.pop();
		}
		name = name.trim_end_matches([' ', '.']).to_owned();
		if name.is_empty() {
			name = "Launch Shortcut".into();
		}
	}

	let stem = name.split('.').next().unwrap_or_default().trim_end().to_uppercase();
	let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
		|| ["COM", "LPT"].iter().any(|prefix| {
			stem.strip_prefix(prefix).is_some_and(|suffix| {
				matches!(
					suffix,
					"1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
				)
			})
		});
	if reserved && !explicit {
		name.insert(0, '_');
	}

	if name.is_empty()
		|| name.chars().any(forbidden)
		|| name.ends_with([' ', '.'])
		|| name.encode_utf16().count() > 251
		|| (reserved && explicit)
	{
		return Err(report!(ErrorMarker::shortcut_name_invalid()));
	}

	Ok(name)
}
