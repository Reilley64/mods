use super::model::Entry;
use super::model::SteamMetadataParseError;
use super::model::Token;
use super::model::Value;
use super::tokenizer::tokenize;
use rootcause::Result;
use rootcause::report;

pub(super) fn parse_key_values(text: &str) -> Result<Vec<Entry>, SteamMetadataParseError> {
	let tokens = tokenize(text)?;
	let mut index = 0;
	let entries = parse_entries(&tokens, &mut index, false)?;
	if index == tokens.len() {
		Ok(entries)
	} else {
		Err(report!(SteamMetadataParseError))
	}
}

fn parse_entries(tokens: &[Token], index: &mut usize, nested: bool) -> Result<Vec<Entry>, SteamMetadataParseError> {
	let mut entries = Vec::new();
	loop {
		match tokens.get(*index) {
			Some(Token::Close) if nested => {
				*index += 1;
				return Ok(entries);
			}
			None if !nested => return Ok(entries),
			Some(Token::Text(key)) => {
				*index += 1;
				let value = match tokens.get(*index) {
					Some(Token::Text(value)) => {
						*index += 1;
						Value::Text(value.clone())
					}
					Some(Token::Open) => {
						*index += 1;
						Value::Object(parse_entries(tokens, index, true)?)
					}
					_ => return Err(report!(SteamMetadataParseError)),
				};
				entries.push(Entry {
					key: key.clone(),
					value,
				});
			}
			_ => return Err(report!(SteamMetadataParseError)),
		}
	}
}
