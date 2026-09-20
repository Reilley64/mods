import { readFile } from "node:fs/promises";
import { TypeSafeClient } from "@typesafe-ai/sdk";
import { createTwoFilesPatch } from "diff";

import { loadConfig } from "../config";
import { reviewChanges } from "../reviewer";
import { extractStyleRules } from "../rules";
import { calibrationCases } from "./cases";

const root = process.cwd();
const config = await loadConfig(root);
const rules = extractStyleRules(await readFile(config.styleFile, "utf8"));
const changes = calibrationCases.map((calibrationCase) => {
	const path = calibrationCase.path ?? `calibration/${calibrationCase.name}.rs`;
	return {
		path,
		before: calibrationCase.before,
		after: calibrationCase.after,
		patch: createTwoFilesPatch(
			calibrationCase.before ? `a/${path}` : "/dev/null",
			`b/${path}`,
			calibrationCase.before,
			calibrationCase.after,
			undefined,
			undefined,
			{ context: 20 },
		),
	};
});
const moduleReferences = new Map(
	calibrationCases.map((calibrationCase) => [
		calibrationCase.path ?? `calibration/${calibrationCase.name}.rs`,
		calibrationCase.referencingFiles ?? [],
	]),
);
const report = await reviewChanges(
	new TypeSafeClient({
		defaultModel: config.model,
		logLevel: "warn",
		retry: { maxRetries: 1 },
		timeout: 20_000,
	}),
	changes,
	rules,
	{ maxConcurrency: config.maxConcurrency, moduleReferences, model: config.model, threshold: 0 },
);
const score = new Map(report.findings.map((finding) => [`${finding.file}:${finding.rule.id}`, finding.probability]));
const rows = calibrationCases.map((calibrationCase) => {
	const path = calibrationCase.path ?? `calibration/${calibrationCase.name}.rs`;
	const all = rules.map((rule) => ({ id: rule.id, probability: score.get(`${path}:${rule.id}`) ?? 0 }));
	const unrelated = all.filter((result) => result.id !== calibrationCase.ruleId).sort((left, right) => right.probability - left.probability)[0]!;
	const configuredFindings = all.filter(
		(result) => result.probability >= (config.ruleThresholds[result.id] ?? config.threshold),
	);
	return {
		name: calibrationCase.name,
		expectedViolation: calibrationCase.expectedViolation,
		ruleId: calibrationCase.ruleId,
		target: score.get(`${path}:${calibrationCase.ruleId}`) ?? 0,
		configuredTargetThreshold: config.ruleThresholds[calibrationCase.ruleId] ?? config.threshold,
		configuredTargetDetected: configuredFindings.some((finding) => finding.id === calibrationCase.ruleId),
		configuredFindings,
		maxUnrelated: unrelated,
		maxAny: all.sort((left, right) => right.probability - left.probability)[0]!,
	};
});
const configuredSummary = {
	truePositive: rows.filter((row) => row.expectedViolation && row.configuredTargetDetected).length,
	falseNegative: rows.filter((row) => row.expectedViolation && !row.configuredTargetDetected).length,
	trueNegative: rows.filter((row) => !row.expectedViolation && row.configuredFindings.length === 0).length,
	falsePositive: rows.filter((row) => !row.expectedViolation && row.configuredFindings.length > 0).length,
};
const badFloor = Math.min(...rows.filter((row) => row.expectedViolation).map((row) => row.target));
const goodCeiling = Math.max(...rows.filter((row) => !row.expectedViolation).map((row) => row.maxAny.probability));
const recommendedThreshold = Number(((badFloor + goodCeiling) / 2).toFixed(2));
const thresholds = Array.from({ length: 14 }, (_, index) => 0.3 + index * 0.05).map((threshold) => {
	let truePositive = 0;
	let falseNegative = 0;
	let trueNegative = 0;
	let falsePositive = 0;
	for (const row of rows) {
		if (row.expectedViolation) {
			if (row.target >= threshold) truePositive += 1;
			else falseNegative += 1;
		} else if (row.maxAny.probability >= threshold) falsePositive += 1;
		else trueNegative += 1;
	}
	const precision = truePositive / Math.max(1, truePositive + falsePositive);
	const recall = truePositive / Math.max(1, truePositive + falseNegative);
	const f1 = (2 * precision * recall) / Math.max(Number.EPSILON, precision + recall);
	return { threshold: Number(threshold.toFixed(2)), truePositive, falseNegative, trueNegative, falsePositive, f1: Number(f1.toFixed(3)) };
});
console.log(
	JSON.stringify(
		{
			model: report.model,
			rules: rules.length,
			observedRange: { badFloor, goodCeiling, recommendedThreshold },
			configuredThresholds: { default: config.threshold, rules: config.ruleThresholds },
			configuredSummary,
			rows,
			thresholds,
			usage: { inputTokens: report.inputTokens, outputTokens: report.outputTokens },
		},
		null,
		2,
	),
);
