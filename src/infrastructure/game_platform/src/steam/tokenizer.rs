use super::model::SteamMetadataParseError;
use super::model::Token;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;

pub(super) fn tokenize(text: &str) -> Result<Vec<Token>, SteamMetadataParseError> {
	let bytes = text.as_bytes();
	let mut tokens = Vec::new();
	let mut index = 0;
	while index < bytes.len() {
		match bytes[index] {
			byte if byte.is_ascii_whitespace() => index += 1,
			b'/' if bytes.get(index + 1) == Some(&b'/') => {
				index += 2;
				while index < bytes.len() && bytes[index] != b'\n' {
					index += 1;
				}
			}
			b'{' => {
				tokens.push(Token::Open);
				index += 1;
			}
			b'}' => {
				tokens.push(Token::Close);
				index += 1;
			}
			b'"' => {
				let (value, next) = quoted_token(bytes, index + 1)?;
				tokens.push(Token::Text(value));
				index = next;
			}
			_ => {
				let start = index;
				while index < bytes.len()
					&& !bytes[index].is_ascii_whitespace()
					&& !matches!(bytes[index], b'{' | b'}')
				{
					index += 1;
				}
				tokens.push(Token::Text(
					text.get(start..index)
						.ok_or_else(|| report!(SteamMetadataParseError))?
						.to_owned(),
				));
			}
		}
	}
	Ok(tokens)
}

fn quoted_token(bytes: &[u8], mut index: usize) -> Result<(String, usize), SteamMetadataParseError> {
	let mut value = Vec::new();
	while index < bytes.len() {
		match bytes[index] {
			b'"' => return Ok((String::from_utf8(value).context(SteamMetadataParseError)?, index + 1)),
			b'\\' => {
				index += 1;
				let escaped = *bytes.get(index).ok_or_else(|| report!(SteamMetadataParseError))?;
				match escaped {
					b'"' => value.push(b'"'),
					b'\\' => value.push(b'\\'),
					other => {
						value.push(b'\\');
						value.push(other);
					}
				}
				index += 1;
			}
			byte => {
				value.push(byte);
				index += 1;
			}
		}
	}
	Err(report!(SteamMetadataParseError))
}
