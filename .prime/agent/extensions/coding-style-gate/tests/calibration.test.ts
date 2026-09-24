import { describe, expect, test } from "bun:test";
import { readFile } from "node:fs/promises";

import { calibrationCases } from "../calibration/cases";
import { commentCalibrationCases } from "../calibration/comment-cases";
import { extractStyleRules } from "../rules";

describe("coding style calibration corpus", () => {
	test("keeps real comment provenance separate from manufactured violations", () => {
		expect(commentCalibrationCases).toHaveLength(15);
		for (const sample of commentCalibrationCases) {
			expect(sample.source?.commit).toMatch(/^[a-f0-9]{7}$/);
			expect(sample.source?.path.endsWith(".rs")).toBeTrue();
			expect(sample.source?.kind).toBe(sample.expectedViolation ? "controlled-mutation" : "excerpt");
			expect(sample.before).not.toBe(sample.after);
		}
	});
	test("pairs labeled good and bad patches for known rubric rules", async () => {
		const rules = extractStyleRules(await readFile("CODING_STYLE.md", "utf8"));
		const ruleIds = new Set(rules.map((rule) => rule.id));
		const names = new Set(calibrationCases.map((calibrationCase) => calibrationCase.name));

		expect(names.size).toBe(calibrationCases.length);
		expect(calibrationCases.every((calibrationCase) => ruleIds.has(calibrationCase.ruleId))).toBeTrue();
		expect(calibrationCases.filter((calibrationCase) => calibrationCase.expectedViolation)).toHaveLength(21);
		expect(calibrationCases.filter((calibrationCase) => !calibrationCase.expectedViolation)).toHaveLength(26);
		for (const ruleId of new Set(calibrationCases.map((calibrationCase) => calibrationCase.ruleId))) {
			const labels = calibrationCases.filter((calibrationCase) => calibrationCase.ruleId === ruleId);
			expect(labels.some((calibrationCase) => calibrationCase.expectedViolation)).toBeTrue();
			expect(labels.some((calibrationCase) => !calibrationCase.expectedViolation)).toBeTrue();
		}
	});
});
