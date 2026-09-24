import { describe, expect, test } from "bun:test";
import { readFile } from "node:fs/promises";

import { calibrationCases } from "../calibration/cases";
import { commentCalibrationCases } from "../calibration/comment-cases";
import { fitRuleThresholds } from "../calibration/fit";
import { perRuleCases } from "../calibration/per-rule-cases";
import { calibrationInput } from "../calibration/input";
import { loadConfig, validateRuleThresholds } from "../config";
import { extractStyleRules } from "../rules";

describe("coding style calibration corpus", () => {

	test("calibration evidence matches added and deleted production snapshots", () => {
		const added = calibrationInput({ name: "added", before: "", after: "fn run() {}" });
		expect(added.change.before).toBeUndefined();
		expect(added.change.after).toBe("fn run() {}");
		const deleted = calibrationInput({ name: "deleted", before: "fn run() {}", after: "", referencingFiles: ["caller.rs"] });
		expect(deleted.change.after).toBeUndefined();
		expect(deleted.moduleReferences.get(deleted.change.path)).toEqual([]);
	});


	test("preserves separation for nearby probability scores", () => {
		const result = fitRuleThresholds(["a"], [
			{ ruleId: "a", split: "train", expectedViolation: false, probability: 0.5000001 },
			{ ruleId: "a", split: "train", expectedViolation: true, probability: 0.5000002 },
		]).a!;
		expect(result.threshold).toBeGreaterThan(0.5000001);
		expect(result.threshold).toBeLessThan(0.5000002);
		expect(result.training.falsePositive + result.training.falseNegative).toBe(0);
	});

	test("repository configuration explicitly covers exactly the current rubric", async () => {
		const config = await loadConfig(process.cwd());
		const rules = extractStyleRules(await readFile(config.styleFile, "utf8"));
		validateRuleThresholds(config.ruleThresholds, rules);
		expect(new Set(Object.keys(config.ruleThresholds))).toEqual(new Set(rules.map(rule => rule.id)));
		expect(Object.hasOwn(config, "threshold")).toBeFalse();
	});

	test("covers every authoritative rule with separate training and validation labels", async () => {
		const rules = extractStyleRules(await readFile("CODING_STYLE.md", "utf8"));
		expect(new Set(perRuleCases.map(sample => sample.name)).size).toBe(perRuleCases.length);
		expect(new Set(perRuleCases.map(sample => sample.ruleId))).toEqual(new Set(rules.map(rule => rule.id)));
		for (const rule of rules) {
			for (const split of ["train", "validation"] as const) {
				for (const expectedViolation of [true, false]) {
					expect(perRuleCases.filter(sample => sample.ruleId === rule.id && sample.split === split && sample.expectedViolation === expectedViolation)).toHaveLength(split === "train" ? 2 : 1);
				}
			}
		}
		const training = new Set(perRuleCases.filter(sample => sample.split === "train").map(sample => `${sample.before}\n${sample.after}`));
		for (const sample of perRuleCases) {
			expect(sample.before).not.toBe(sample.after);
			if (sample.split === "validation") expect(training.has(`${sample.before}\n${sample.after}`)).toBeFalse();
		}
	});


	test("reports overlapping score distributions and rejects invalid scores", () => {
		const overlapping = [
			{ ruleId: "a", split: "train" as const, expectedViolation: false, probability: 0.8 },
			{ ruleId: "a", split: "train" as const, expectedViolation: true, probability: 0.4 },
		];
		const result = fitRuleThresholds(["a"], overlapping).a!;
		expect(result.separable).toBeFalse();
		expect(result.training.falsePositive).toBe(0);
		expect(result.training.falseNegative).toBe(1);
		expect(() => fitRuleThresholds(["a"], [...overlapping, { ...overlapping[0]!, probability: NaN }])).toThrow("Invalid calibration probability");
	});


	test("fits each rule from training rows without using validation labels", () => {
		const rows = [
			{ ruleId: "a", split: "train" as const, expectedViolation: false, probability: 0.2 },
			{ ruleId: "a", split: "train" as const, expectedViolation: true, probability: 0.8 },
			{ ruleId: "a", split: "validation" as const, expectedViolation: true, probability: 0.1 },
			{ ruleId: "b", split: "train" as const, expectedViolation: false, probability: 0.6 },
			{ ruleId: "b", split: "train" as const, expectedViolation: true, probability: 0.9 },
		];
		const fitted = fitRuleThresholds(["a", "b"], rows);
		expect(fitted.a?.threshold).toBe(0.5);
		expect(fitted.b?.threshold).toBe(0.75);
		expect(fitted.a?.separable).toBeTrue();
		expect(() => fitRuleThresholds(["missing"], rows)).toThrow("both labels");
	});

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
