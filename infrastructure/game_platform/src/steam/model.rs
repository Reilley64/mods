use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, Copy)]
pub(super) struct SteamMetadataParseError;

impl fmt::Display for SteamMetadataParseError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("Steam metadata is malformed")
	}
}

impl Error for SteamMetadataParseError {}

#[derive(Debug)]
pub(super) struct Entry {
	pub(super) key: String,
	pub(super) value: Value,
}

#[derive(Debug)]
pub(super) enum Value {
	Text(String),
	Object(Vec<Entry>),
}

#[derive(Debug)]
pub(super) enum Token {
	Text(String),
	Open,
	Close,
}

pub(super) fn exactly_one_object<'a>(entries: &'a [Entry], key: &str) -> Option<&'a [Entry]> {
	let mut matches = entries.iter().filter(|entry| entry.key.eq_ignore_ascii_case(key));
	let Value::Object(value) = &matches.next()?.value else {
		return None;
	};
	if matches.next().is_some() { None } else { Some(value) }
}

pub(super) fn exactly_one_text<'a>(entries: &'a [Entry], key: &str) -> Option<&'a str> {
	let mut matches = entries.iter().filter(|entry| entry.key.eq_ignore_ascii_case(key));
	let Value::Text(value) = &matches.next()?.value else {
		return None;
	};
	if matches.next().is_some() { None } else { Some(value) }
}
