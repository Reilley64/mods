import { TypeSafeClient } from "@typesafe-ai/sdk";

import { loadConfig } from "../config";
import { loadStyleRules } from "../policy";
import { reviewChanges } from "../reviewer";
import { findModuleReferencingFiles } from "../snapshot";
import { loadGitRange } from "./git-range";

const [base, head, ruleFilter, ...pathFilters] = process.argv.slice(2);
if (!base || !head || !/^[0-9a-f]{40}$/.test(base) || !/^[0-9a-f]{40}$/.test(head)) {
	throw new Error(
		"usage: bun review-range.ts <40-character-base-oid> <40-character-head-oid> [rule-id] [exact-path ...]",
	);
}
const root = process.cwd();
const { changes, completeAfter, effectiveBase } = await loadGitRange(root, base, head, pathFilters);
const moduleReferences = new Map(
	changes.map((change) => [change.path, findModuleReferencingFiles(completeAfter, change.path)]),
);
const config = await loadConfig(root);
const thresholdOverride = process.env.CODING_STYLE_GATE_THRESHOLD;
const threshold = thresholdOverride === undefined ? config.threshold : Number(thresholdOverride);
if (!Number.isFinite(threshold) || threshold < 0 || threshold > 1) {
	throw new Error("CODING_STYLE_GATE_THRESHOLD must be between 0 and 1");
}
const allRules = await loadStyleRules(root, config.styleFile);
const rules = ruleFilter ? allRules.filter((rule) => rule.id === ruleFilter) : allRules;
if (rules.length === 0) {
	throw new Error(`unknown rubric rule ${ruleFilter}`);
}
const report = await reviewChanges(
	new TypeSafeClient({
		defaultModel: config.model,
		logLevel: "warn",
		retry: { maxRetries: 1 },
		timeout: config.timeoutMs,
	}),
	changes,
	rules,
	{
		maxConcurrency: config.maxConcurrency,
		moduleReferences,
		model: config.model,
		threshold,
		ruleThresholds: thresholdOverride === undefined ? config.ruleThresholds : {},
	},
);
console.log(
	JSON.stringify(
		{
			range: { requestedBase: base, effectiveBase, head },
			ruleFilter: ruleFilter ?? null,
			model: report.model,
			threshold,
			ruleThresholds: thresholdOverride === undefined ? config.ruleThresholds : {},
			filesReviewed: report.filesReviewed,
			findings: report.findings.map((finding) => ({
				file: finding.file,
				ruleId: finding.rule.id,
				section: finding.rule.section,
				title: finding.rule.title,
				probability: finding.probability,
				violation: finding.rule.violation,
			})),
			usage: { inputTokens: report.inputTokens, outputTokens: report.outputTokens },
		},
		null,
		2,
	),
);
