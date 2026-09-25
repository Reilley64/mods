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
	use super::profile_test_file_slots;

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
