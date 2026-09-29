import { afterEach, describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { applyDispositions, loadDispositions } from "../dispositions";
import type { StyleFinding } from "../reviewer";
import type { StyleRule } from "../rules";

const temporaryDirectories: string[] = [];
const file = ".prime/agent/coding-style-dispositions.json";

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});

async function projectWith(content?: string): Promise<string> {
	const root = await mkdtemp(join(tmpdir(), "coding-style-dispositions-"));
	temporaryDirectories.push(root);
	await mkdir(join(root, ".prime", "agent"), { recursive: true });
	if (content !== undefined) {
		await writeFile(join(root, file), content);
	}
	return root;
}

const rule = (id: string, section: string, title: string): StyleRule => ({
	id,
	section,
	title,
	text: "",
	violation: "",
	compliant: "",
	badExamples: [],
	goodExamples: [],
});
const spacing = rule("formatting-and-imports-phase-spacing", "Formatting and imports", "Phase spacing");
const guards = rule("control-flow-guard-clauses", "Control flow", "Guard clauses");
const finding = (path: string, style: StyleRule): StyleFinding => ({ file: path, rule: style, probability: 0.9 });

describe("coding style gate dispositions", () => {
	test("an absent file accepts nothing and reports nothing", async () => {
		const root = await projectWith();

		expect(await loadDispositions(root, file)).toEqual({ dispositions: [], problems: [] });
	});

	test("a malformed file accepts nothing and reports the problem", async () => {
		for (const content of ["{not json", "[]", JSON.stringify({ dispositions: {} }), "null"]) {
			const root = await projectWith(content);

			const loaded = await loadDispositions(root, file);

			expect(loaded.dispositions).toEqual([]);
			expect(loaded.problems).toHaveLength(1);
			expect(loaded.problems[0]).toContain("no findings were accepted");
		}
	});

	test("an entry with an empty or missing reason accepts nothing", async () => {
		const root = await projectWith(
			JSON.stringify({
				dispositions: [
					{ file: "src/a.rs", rule: spacing.id, reason: "   " },
					{ file: "src/b.rs", rule: spacing.id },
					{ file: "src/c.rs", rule: spacing.id, reason: "Documented reason." },
				],
			}),
		);

		const loaded = await loadDispositions(root, file);
		const outcome = applyDispositions(
			[finding("src/a.rs", spacing), finding("src/b.rs", spacing), finding("src/c.rs", spacing)],
			loaded.dispositions,
		);

		expect(loaded.problems).toEqual([
			`${file} entry 0 has no non-empty reason; it accepts no findings.`,
			`${file} entry 1 has no non-empty reason; it accepts no findings.`,
		]);
		expect(outcome.accepted.map((accepted) => accepted.file)).toEqual(["src/c.rs"]);
		expect(outcome.unaccepted.map((unaccepted) => unaccepted.file)).toEqual(["src/a.rs", "src/b.rs"]);
	});

	test("rejects entries and files outside the repository", async () => {
		const root = await projectWith(
			JSON.stringify({
				dispositions: [
					{ file: "../other/src/a.rs", rule: spacing.id, reason: "Outside." },
					{ file: join(tmpdir(), "a.rs"), rule: spacing.id, reason: "Absolute." },
				],
			}),
		);
		const outside = await projectWith(JSON.stringify({ dispositions: [{ file: "a.rs", rule: spacing.id, reason: "Linked." }] }));
		await symlink(join(outside, file), join(root, ".prime", "agent", "linked.json"));

		const loaded = await loadDispositions(root, file);
		const linked = await loadDispositions(root, ".prime/agent/linked.json");

		expect(loaded.dispositions).toEqual([]);
		expect(loaded.problems).toHaveLength(2);
		expect(linked.dispositions).toEqual([]);
		expect(linked.problems[0]).toContain("within the project");
	});

	test("matches the repository-relative path with the rule ID or displayed rule", async () => {
		const root = await projectWith(
			JSON.stringify({
				dispositions: [
					{ file: "./src/a.rs", rule: spacing.id, reason: "Rule ID." },
					{ file: "src\\b.rs", rule: "Control flow / Guard clauses", reason: "Displayed rule." },
					{ file: "src/c.rs", rule: "Phase spacing", reason: "Title alone is ambiguous." },
				],
			}),
		);

		const loaded = await loadDispositions(root, file);
		const outcome = applyDispositions(
			[
				finding("src/a.rs", spacing),
				finding("src/a.rs", guards),
				finding("src/b.rs", guards),
				finding("src/b.rs", spacing),
				finding("src/c.rs", spacing),
				finding("other/src/a.rs", spacing),
			],
			loaded.dispositions,
		);

		expect(loaded.problems).toEqual([]);
		expect(outcome.accepted.map((accepted) => `${accepted.file} ${accepted.rule.id}`)).toEqual([
			"src/a.rs formatting-and-imports-phase-spacing",
			"src/b.rs control-flow-guard-clauses",
		]);
		expect(outcome.unaccepted.map((unaccepted) => `${unaccepted.file} ${unaccepted.rule.id}`)).toEqual([
			"src/a.rs control-flow-guard-clauses",
			"src/b.rs formatting-and-imports-phase-spacing",
			"src/c.rs formatting-and-imports-phase-spacing",
			"other/src/a.rs formatting-and-imports-phase-spacing",
		]);
	});
});
