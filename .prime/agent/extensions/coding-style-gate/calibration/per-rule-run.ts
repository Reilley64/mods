import { appendFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { createHash } from "node:crypto";
import { createReviewClient } from "../provider";
import { calibrationInput } from "./input";
import { reviewChanges } from "../reviewer";
import { extractStyleRules } from "../rules";
import { calibrationCases } from "./cases";
import { perRuleCases } from "./per-rule-cases";
import { fitRuleThresholds, measure, type ScoredCase } from "./fit";

const output = process.argv[2];
if (!output) throw new Error("Usage: bun calibration/per-rule-run.ts <new-output.json>");
await mkdir(dirname(output), { recursive: true });
await writeFile(`${output}.jsonl`, "", { flag: "wx" });
const config = JSON.parse(await readFile(".prime/agent/coding-style-gate.json", "utf8"));
const style = await readFile(config.styleFile, "utf8");
const rules = extractStyleRules(style);
const ruleIds = rules.map(rule => rule.id);
const fixtures = [
	...perRuleCases.map(sample => ({ ...sample, cohort: "per-rule" })),
	...calibrationCases.map(sample => ({ ...sample, split: "validation" as const, cohort: "regression" })),
];
if (new Set(fixtures.map(sample => sample.name)).size !== fixtures.length) throw new Error("Duplicate fixture names");
for (const ruleId of ruleIds) {
	for (const split of ["train", "validation"] as const) {
		for (const label of [true, false]) {
			if (!perRuleCases.some(sample => sample.ruleId === ruleId && sample.split === split && sample.expectedViolation === label)) {
				throw new Error(`Missing ${split}/${label} fixture for ${ruleId}`);
			}
		}
	}
}
for (const sample of fixtures) {
	if (!ruleIds.includes(sample.ruleId)) throw new Error(`Unknown fixture rule ${sample.ruleId}`);
}
const client = createReviewClient({ ...config, timeoutMs: 20_000 });
const rows: Array<ScoredCase & { name: string; cohort: string; inputTokens: number; outputTokens: number }> = [];
async function scoreSplit(split: "train" | "validation") {
	const pending = fixtures.filter(sample => sample.split === split);
	let next = 0;
	await Promise.all(Array.from({ length: Math.min(config.maxConcurrency, pending.length) }, async () => {
		while (next < pending.length) {
			const sample = pending[next++]!;
			const input = calibrationInput(sample);
			const selected = split === "train" || sample.cohort === "regression"
				? rules.filter(rule => rule.id === sample.ruleId) : rules;
			const report = await reviewChanges(client, [input.change], selected, {
				model: config.model,
				ruleThresholds: Object.fromEntries(selected.map(rule => [rule.id, 0])),
				maxConcurrency: 1,
				moduleReferences: input.moduleReferences,
			});
			const probability = report.findings.find(finding => finding.rule.id === sample.ruleId)?.probability;
			if (probability === undefined) throw new Error(`Missing score for ${sample.name}`);
			const row = { name: sample.name, ruleId: sample.ruleId, split, cohort: sample.cohort, expectedViolation: sample.expectedViolation, probability, inputTokens: report.inputTokens, outputTokens: report.outputTokens };
			rows.push(row);
			await appendFile(`${output}.jsonl`, `${JSON.stringify(row)}\n`);
		}
	}));
}
await scoreSplit("train");
const fitted = fitRuleThresholds(ruleIds, rows);
await writeFile(`${output}.thresholds.json`, JSON.stringify(fitted, null, 2));
await scoreSplit("validation");
rows.sort((a, b) => a.name.localeCompare(b.name));
const results = Object.fromEntries(ruleIds.map(ruleId => [ruleId, {
	...fitted[ruleId],
	validation: measure(rows.filter(row => row.ruleId === ruleId && row.cohort === "per-rule" && row.split === "validation"), fitted[ruleId]!.threshold),
	regression: measure(rows.filter(row => row.ruleId === ruleId && row.cohort === "regression"), fitted[ruleId]!.threshold),
}]));
const result = {
	model: config.model,
		timestamp: new Date().toISOString(),
	rubricSha256: createHash("sha256").update(style).digest("hex"),
	reviewerSha256: createHash("sha256").update(await readFile(new URL("../reviewer.ts", import.meta.url))).digest("hex"),
	calibrationInputSha256: createHash("sha256").update(await readFile(new URL("./input.ts", import.meta.url))).digest("hex"),
	fittingSha256: createHash("sha256").update(await readFile(new URL("./fit.ts", import.meta.url))).digest("hex"),
	fixtureSha256: createHash("sha256").update(JSON.stringify(fixtures)).digest("hex"),
	method: "Training only; minimize 2*FP+FN, then fewer FP, then largest observed margin, then larger threshold. Per-rule held-out validation uses all production rubric questions. Training and legacy regression score only their labeled rule with the production question builder.",
	results, rows,
	usage: { inputTokens: rows.reduce((sum, row) => sum + row.inputTokens, 0), outputTokens: rows.reduce((sum, row) => sum + row.outputTokens, 0) },
};
await writeFile(output, JSON.stringify(result, null, 2));
console.log(JSON.stringify({ output, rules: ruleIds.length, cases: rows.length, usage: result.usage }));
