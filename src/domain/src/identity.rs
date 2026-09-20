use unicode_case_mapping::case_folded;

// Path and mod-name identity must be stable across host platforms and locales. Simple Unicode case folding keeps
// compatibility characters and normalization forms distinct because it maps each scalar to at most one scalar.
pub fn case_fold_key(value: &str) -> String {
	value.chars()
		.map(|character| {
			case_folded(character)
				.and_then(|folded| char::from_u32(folded.get()))
				.unwrap_or(character)
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::case_fold_key;

	#[test]
	fn simple_case_folding_handles_unicode_without_expansion_or_normalization() {
		assert_eq!(case_fold_key("ÉΣς"), "éσσ");
		assert_ne!(case_fold_key("ﬃ"), case_fold_key("ffi"));
		assert_ne!(case_fold_key("ß"), case_fold_key("ss"));
		assert_ne!(case_fold_key("é"), case_fold_key("e\u{301}"));
	}
}
