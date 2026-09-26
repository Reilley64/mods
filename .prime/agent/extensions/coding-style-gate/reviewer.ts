import { type NoulResponse, TypeSafeClient, noul } from "@typesafe-ai/sdk";

import { cachedInference, requestKey } from "./cache";
import { backendIdentity, responseModelMatches } from "./provider";

import { validateRuleThresholds } from "./config";
import type { StyleRule } from "./rules";
import { declaresFileNamedEntryPoint, type RustChange } from "./snapshot";

const MAX_PATCH_CHARS = 100_000;

export interface ReviewOptions {
	model: string;
	cacheDirectory?: string;
	ruleThresholds: Readonly<Record<string, number>>;
	maxConcurrency?: number;
	moduleReferences?: ReadonlyMap<string, readonly string[]>;
	signal?: AbortSignal;
}

export interface StyleFinding {
	file: string;
	probability: number;
	rule: StyleRule;
}

export interface StyleReviewReport {
	model: string;
	filesReviewed: number;
	cachedFiles?: number;
	findings: StyleFinding[];
	inputTokens: number;
	outputTokens: number;
}

function questionFor(rule: StyleRule) {
	return noul(
		{
			question: `Does the supplied review state directly show this coding-style violation: ${rule.violation}`,
			inspect: ["file", "patch", "change_kind", "declares_file_named_entry_point", "module_referencing_files"],
			repository_rule: rule.text,
			focus:
				"Judge added and removed lines. Use unchanged lines only as context. Use change_kind: a deletion violates a rule only when it removes something required. Use file and module_referencing_files when the rubric concerns module placement or reuse.",
		},
		{
			true: { what: rule.violation, examples: rule.badExamples },
			false: {
				what: `${rule.compliant} If the patch lacks direct evidence or the rule does not apply, answer false.`,
				examples: rule.goodExamples,
			},
		},
	);
}

function validateAnswer(answer: unknown, rule: StyleRule): NoulResponse {
	if (
		typeof answer !== "object" ||
		answer === null ||
		(answer as { type?: unknown }).type !== "noul" ||
		typeof (answer as { noul?: unknown }).noul !== "number" ||
		!Number.isFinite((answer as { noul: number }).noul) ||
		(answer as { noul: number }).noul < 0 ||
		(answer as { noul: number }).noul > 1
	) {
		throw new Error(`coding-style-gate: OpenRouter returned no valid Noul answer for rule ${rule.id}`);
	}
	return answer as NoulResponse;
}

function validateUsage(value: unknown, field: string): number {
	if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
		throw new Error(`coding-style-gate: OpenRouter returned invalid ${field}`);
	}
	return value;
}

export async function reviewChanges(
	client: TypeSafeClient,
	changes: readonly RustChange[],
	rules: readonly StyleRule[],
	options: ReviewOptions,
): Promise<StyleReviewReport> {
	validateRuleThresholds(options.ruleThresholds, rules);

	for (const change of changes) {
		if (change.patch.length > MAX_PATCH_CHARS) {
			throw new Error(
				`coding-style-gate: ${change.path} patch is ${change.patch.length} characters; the review limit is ${MAX_PATCH_CHARS}`,
			);
		}
	}

	interface Scores { model: string; probabilities: number[]; }
	const validateScores = (value: unknown): Scores => {
		const scores = value as Scores;
		if (!scores || !responseModelMatches(options.model, scores.model)
			|| !Array.isArray(scores.probabilities) || scores.probabilities.length !== rules.length
			|| scores.probabilities.some((score) => typeof score !== "number" || !Number.isFinite(score) || score < 0 || score > 1)) {
			throw new Error("coding-style-gate: invalid cached scores");
		}
		return scores;
	};
	const results = new Array<{ value: Scores; cached: boolean; inputTokens: number; outputTokens: number }>(changes.length);
	let nextIndex = 0;
	const worker = async () => {
		while (nextIndex < changes.length) {
			const index = nextIndex++;
			const change = changes[index]!;
			const questions = Object.fromEntries(rules.map((rule) => [rule.id, questionFor(rule)]));
			const request = {
				model: options.model,
				state: {
					purpose: "Review this task-local Rust patch against repository coding-style rules.",
					file: change.path,
					patch: change.patch,
					change_kind: change.before === undefined ? "added" : change.after === undefined ? "deleted" : "modified",
					declares_file_named_entry_point: declaresFileNamedEntryPoint(change),
					module_referencing_files: options.moduleReferences?.get(change.path) ?? [],
				},
				questions,
			};
			let inputTokens = 0;
			let outputTokens = 0;
			const result = await cachedInference(options.cacheDirectory,
				requestKey(request, backendIdentity()), validateScores, async () => {
					const response = await client.systemOne(request, { signal: options.signal });
					if (!responseModelMatches(options.model, response.model)) {
						throw new Error("coding-style-gate: OpenRouter returned a response from an unexpected model");
					}
					inputTokens = validateUsage(response.usage?.input_tokens, "input token usage");
					outputTokens = validateUsage(response.usage?.output_tokens, "output token usage");
					return { model: response.model, probabilities: rules.map((rule) =>
						validateAnswer((response.answers as Record<string, unknown>)?.[rule.id], rule).noul) };
				});
			results[index] = { ...result, inputTokens, outputTokens };
		}
	};
	const maxConcurrency = Math.max(1, Math.floor(options.maxConcurrency ?? 4));
	await Promise.all(Array.from({ length: Math.min(maxConcurrency, changes.length) }, worker));

	const findings: StyleFinding[] = [];
	let model = options.model;
	let inputTokens = 0;
	let outputTokens = 0;
	for (const [index, result] of results.entries()) {
		const change = changes[index]!;
		model = result.value.model;
		inputTokens += result.inputTokens;
		outputTokens += result.outputTokens;
		for (const [ruleIndex, rule] of rules.entries()) {
			const probability = result.value.probabilities[ruleIndex]!;
			if (probability >= options.ruleThresholds[rule.id]!) {
				findings.push({ file: change.path, probability, rule });
			}
		}
	}

	return {
		model,
		filesReviewed: changes.length,
		cachedFiles: results.filter((result) => result.cached).length,
		findings: findings.sort((left, right) => right.probability - left.probability),
		inputTokens,
		outputTokens,
	};
}
