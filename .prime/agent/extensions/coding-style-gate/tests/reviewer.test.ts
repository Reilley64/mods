import { afterAll, afterEach, beforeAll, describe, expect, test } from "bun:test";
import { TypeSafeClient } from "@typesafe-ai/sdk";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";

import type { RustChange } from "../snapshot";
import type { StyleRule } from "../rules";
import { reviewChanges } from "../reviewer";

const requests: unknown[] = [];
const originalApiKey = process.env.TYPESAFE_API_KEY;
const server = setupServer(
	http.post("https://api.typesafe.ai/v1/systemone", async ({ request }) => {
		const body = (await request.clone().json()) as {
			questions: Record<string, unknown>;
			state: unknown;
		};
		requests.push(body);
		const answers = Object.fromEntries(
			Object.keys(body.questions).map((key) => [key, { type: "noul", noul: key === "comments-and-documentation-narrative-comments" ? 0.91 : 0.08 }]),
		);
		return HttpResponse.json({
			model: "jev-test",
			answers,
			usage: { input_tokens: 123, output_tokens: 4 },
		});
	}),
);

beforeAll(() => {
	process.env.TYPESAFE_API_KEY = "test-key";
	server.listen({ onUnhandledRequest: "error" });
});
afterEach(() => {
	requests.length = 0;
	server.resetHandlers();
});
afterAll(() => {
	server.close();
	if (originalApiKey === undefined) delete process.env.TYPESAFE_API_KEY;
	else process.env.TYPESAFE_API_KEY = originalApiKey;
});

const changes: RustChange[] = [
	{
		path: "application/src/example.rs",
		before: "fn run() {}\n",
		after: "// Run the function.\nfn run() {}\n",
		patch: "@@ -1 +1,2 @@\n+// Run the function.\n fn run() {}\n",
	},
];

const rules: StyleRule[] = [
	{
		id: "comments-and-documentation-narrative-comments",
		section: "Comments and documentation",
		title: "Narrative comments",
		text: "Comments explain why a constraint exists rather than narrating the code.",
		violation: "An added comment merely restates the adjacent operation.",
		compliant: "The comment explains a necessary constraint or surprising decision.",
		badExamples: ["// Add one to value.\nvalue += 1;"],
		goodExamples: ["// Saturation preserves counters imported from legacy saves.\nvalue = value.saturating_add(1);"],
	},
	{
		id: "control-flow-guard-clauses",
		section: "Control flow",
		title: "Guard clauses",
		text: "Prefer guard clauses.",
		violation: "The successful path is nested under a condition whose opposite branch exits.",
		compliant: "The exiting case appears first and the successful path remains unnested.",
		badExamples: ["if ready { publish(); } else { return Err(NotReady); }"],
		goodExamples: ["if !ready { return Err(NotReady); } publish();"],
	},
];

describe("Jev coding style review", () => {
	test("sends changed hunks through the official SDK and reports threshold violations", async () => {
		const client = new TypeSafeClient({
			apiKey: "test-key",
			defaultModel: "jev-test",
			retry: { maxRetries: 0 },
		});

		const report = await reviewChanges(client, changes, rules, {
			model: "jev-test",
			threshold: 0.8,
			moduleReferences: new Map([["application/src/example.rs", ["application/src/caller.rs"]]]),
		});

		expect(requests).toHaveLength(1);
		expect(requests[0]).toMatchObject({
			state: {
				file: "application/src/example.rs",
				patch: changes[0]?.patch,
				change_kind: "modified",
				declares_file_named_entry_point: false,
				module_referencing_files: ["application/src/caller.rs"],
			},
		});
		expect((requests[0] as { state: Record<string, unknown> }).state).not.toHaveProperty("current_source");
		expect(requests[0]).toMatchObject({
			questions: {
				"comments-and-documentation-narrative-comments": {
					instructions: {
						question: expect.stringContaining("added comment"),
						inspect: ["file", "patch", "change_kind", "declares_file_named_entry_point", "module_referencing_files"],
					},
					criteria: {
						true: { what: rules[0]!.violation, examples: rules[0]!.badExamples },
						false: {
							what: `${rules[0]!.compliant} If the patch lacks direct evidence or the rule does not apply, answer false.`,
							examples: rules[0]!.goodExamples,
						},
					},
				},
			},
		});
		expect(report).toMatchObject({
			model: "jev-test",
			filesReviewed: 1,
			findings: [
				{
					file: "application/src/example.rs",
					probability: 0.91,
					rule: rules[0],
				},
			],
		});
	});

	test("sends more than 32 rubric questions in one request", async () => {
		const client = new TypeSafeClient({ apiKey: "test-key", retry: { maxRetries: 0 } });
		const manyRules: StyleRule[] = Array.from({ length: 33 }, (_, index) => ({
			id: `rule-${index}`,
			section: "Test",
			title: `Rule ${index}`,
			text: `Follow rule ${index}.`,
			violation: `Rule ${index} is violated.`,
			compliant: `Rule ${index} is followed.`,
			badExamples: [`bad ${index}`],
			goodExamples: [`good ${index}`],
		}));

		const report = await reviewChanges(client, changes, manyRules, { model: "jev-test", threshold: 0.8 });

		expect(requests).toHaveLength(1);
		expect(Object.keys((requests[0] as { questions: Record<string, unknown> }).questions)).toHaveLength(33);
		expect(report.filesReviewed).toBe(1);
	});


	test("rejects malformed TypeSafe answers instead of treating them as compliance", async () => {
		let answer: unknown = undefined;
		server.use(
			http.post("https://api.typesafe.ai/v1/systemone", async ({ request }) => {
				const body = (await request.clone().json()) as { questions: Record<string, unknown> };
				return HttpResponse.json({
					model: "jev-test",
					answers: Object.fromEntries(Object.keys(body.questions).map((key) => [key, answer])),
					usage: { input_tokens: 1, output_tokens: 1 },
				});
			}),
		);
		const client = new TypeSafeClient({ apiKey: "test-key", retry: { maxRetries: 0 } });
		for (const malformed of [undefined, { type: "choice", choice: "yes", confidence: 1 }, { type: "noul", noul: null }, { type: "noul", noul: -0.1 }, { type: "noul", noul: 1.1 }]) {
			answer = malformed;
			await expect(reviewChanges(client, changes, rules, { model: "jev-test", threshold: 0.8 })).rejects.toThrow(
				"no valid Noul answer",
			);
		}
	});

	test("rejects an oversized patch before sending it", async () => {
		const client = new TypeSafeClient({ apiKey: "test-key", retry: { maxRetries: 0 } });
		const oversized = [{ ...changes[0]!, patch: "x".repeat(100_001) }];

		await expect(reviewChanges(client, oversized, rules, { model: "jev-test", threshold: 0.8 })).rejects.toThrow(
			"review limit",
		);
	});


	test("applies a calibrated threshold for an individual rubric rule", async () => {
		const client = new TypeSafeClient({ apiKey: "test-key", retry: { maxRetries: 0 } });

		const report = await reviewChanges(client, changes, rules, {
			model: "jev-test",
			threshold: 0.9,
			ruleThresholds: { "control-flow-guard-clauses": 0.05 },
		});

		expect(report.findings.map((finding) => finding.rule.id)).toEqual([
			"comments-and-documentation-narrative-comments",
			"control-flow-guard-clauses",
		]);
	});


	test("rejects a response from a model other than the pinned model", async () => {
		server.use(
			http.post("https://api.typesafe.ai/v1/systemone", async ({ request }) => {
				const body = (await request.clone().json()) as { questions: Record<string, unknown> };
				return HttpResponse.json({
					model: "jev-substituted",
					answers: Object.fromEntries(
						Object.keys(body.questions).map((key) => [key, { type: "noul", noul: 0.02 }]),
					),
					usage: { input_tokens: 1, output_tokens: 1 },
				});
			}),
		);
		const client = new TypeSafeClient({ apiKey: "test-key", retry: { maxRetries: 0 } });

		expect(
			reviewChanges(client, changes, rules, { model: "jev-test", threshold: 0.8 }),
		).rejects.toThrow("unexpected model");
	});

});
