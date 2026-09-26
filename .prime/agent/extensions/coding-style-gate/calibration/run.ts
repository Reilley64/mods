import { readFile } from "node:fs/promises";
import { createReviewClient } from "../provider";
import { calibrationInput } from "./input";

import { loadConfig, validateRuleThresholds } from "../config";
import { reviewChanges } from "../reviewer";
import { extractStyleRules } from "../rules";
import { calibrationCases, type CalibrationCase } from "./cases";

const config = await loadConfig(process.cwd());
const rules = extractStyleRules(await readFile(config.styleFile, "utf8"));
validateRuleThresholds(config.ruleThresholds, rules);
const client = createReviewClient({ ...config, timeoutMs: 20_000 });

async function scoreCase(sample: CalibrationCase) {
	const input = calibrationInput(sample);
	const report = await reviewChanges(client, [input.change], rules, {
		model: config.model,
		ruleThresholds: Object.fromEntries(rules.map(rule => [rule.id, 0])),
		maxConcurrency: 1,
		moduleReferences: input.moduleReferences,
	});
	const all = report.findings.map(finding => ({ id: finding.rule.id, probability: finding.probability }));
	const target = all.find(result => result.id === sample.ruleId);
	if (target === undefined) throw new Error(`Missing calibration score for ${sample.name}`);
	const configuredFindings = all.filter(result => result.probability >= config.ruleThresholds[result.id]!);
	return {
		row: {
			name: sample.name,
			expectedViolation: sample.expectedViolation,
			ruleId: sample.ruleId,
			target: target.probability,
			configuredTargetThreshold: config.ruleThresholds[sample.ruleId]!,
			configuredTargetDetected: configuredFindings.some(finding => finding.id === sample.ruleId),
			configuredFindings,
			maxUnrelated: all.find(result => result.id !== sample.ruleId),
			maxAny: all[0],
		},
		inputTokens: report.inputTokens,
		outputTokens: report.outputTokens,
	};
}

const results: Array<Awaited<ReturnType<typeof scoreCase>>> = [];
let next = 0;
await Promise.all(Array.from({ length: Math.min(config.maxConcurrency, calibrationCases.length) }, async () => {
	while (next < calibrationCases.length) {
		results.push(await scoreCase(calibrationCases[next++]!));
	}
}));
const rows = results.map(result => result.row).sort((a, b) => a.name.localeCompare(b.name));
const configuredSummary = {
	truePositive: rows.filter(row => row.expectedViolation && row.configuredTargetDetected).length,
	falseNegative: rows.filter(row => row.expectedViolation && !row.configuredTargetDetected).length,
	trueNegative: rows.filter(row => !row.expectedViolation && row.configuredFindings.length === 0).length,
	falsePositive: rows.filter(row => !row.expectedViolation && row.configuredFindings.length > 0).length,
};
console.log(JSON.stringify({
	model: config.model,
		rules: rules.length,
	configuredThresholds: config.ruleThresholds,
	configuredSummary,
	rows,
	usage: { inputTokens: results.reduce((sum, result) => sum + result.inputTokens, 0), outputTokens: results.reduce((sum, result) => sum + result.outputTokens, 0) },
}, null, 2));
