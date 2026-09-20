import { describe, expect, test } from "bun:test";
import { readFile } from "node:fs/promises";

import { calibrationCases } from "../calibration/cases";
import { extractStyleRules } from "../rules";

describe("coding style calibration corpus", () => {
	test("pairs labeled good and bad patches for known rubric rules", async () => {
		const rules = extractStyleRules(await readFile("CODING_STYLE.md", "utf8"));
		const ruleIds = new Set(rules.map((rule) => rule.id));
		const names = new Set(calibrationCases.map((calibrationCase) => calibrationCase.name));

		expect(names.size).toBe(calibrationCases.length);
		expect(calibrationCases.every((calibrationCase) => ruleIds.has(calibrationCase.ruleId))).toBeTrue();
		expect(calibrationCases.filter((calibrationCase) => calibrationCase.expectedViolation)).toHaveLength(15);
		expect(calibrationCases.filter((calibrationCase) => !calibrationCase.expectedViolation)).toHaveLength(15);
		for (const ruleId of new Set(calibrationCases.map((calibrationCase) => calibrationCase.ruleId))) {
			const labels = calibrationCases.filter((calibrationCase) => calibrationCase.ruleId === ruleId);
			expect(labels.some((calibrationCase) => calibrationCase.expectedViolation)).toBeTrue();
			expect(labels.some((calibrationCase) => !calibrationCase.expectedViolation)).toBeTrue();
		}
	});
});
