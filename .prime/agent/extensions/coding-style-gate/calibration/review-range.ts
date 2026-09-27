import { createReviewClient } from "../provider";

import { loadConfig, validateRuleThresholds } from "../config";
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
if (process.env.CODING_STYLE_GATE_THRESHOLD !== undefined) {
	throw new Error("CODING_STYLE_GATE_THRESHOLD is unsupported; configure explicit ruleThresholds instead");
}
const allRules = await loadStyleRules(root, config.styleFile);
const rules = ruleFilter ? allRules.filter((rule) => rule.id === ruleFilter) : allRules;
if (rules.length === 0) {
	throw new Error(`unknown rubric rule ${ruleFilter}`);
}
validateRuleThresholds(config.ruleThresholds, rules);
const report = await reviewChanges(
	createReviewClient(config),
	changes,
	rules,
	{
		maxConcurrency: config.maxConcurrency,
		moduleReferences,
		model: config.model,
		ruleThresholds: config.ruleThresholds,
	},
);
console.log(
	JSON.stringify(
		{
			range: { requestedBase: base, effectiveBase, head },
			ruleFilter: ruleFilter ?? null,
			model: report.model,
		ruleThresholds: config.ruleThresholds,
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
