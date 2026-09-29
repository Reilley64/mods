import { afterEach, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
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
const sha256 = (content: string) => createHash("sha256").update(content).digest("hex");
const reviewedSource = "// Run the function.\nfn run() {}\n";
const reviewedHash = sha256(reviewedSource);
const reviewed = (...paths: string[]) => new Map(paths.map((path) => [path, reviewedSource]));

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
					{ file: "src/a.rs", rule: spacing.id, reason: "   ", sha256: reviewedHash },
					{ file: "src/b.rs", rule: spacing.id, sha256: reviewedHash },
					{ file: "src/c.rs", rule: spacing.id, reason: "Documented reason.", sha256: reviewedHash },
				],
			}),
		);

		const loaded = await loadDispositions(root, file);
		const outcome = applyDispositions(
			[finding("src/a.rs", spacing), finding("src/b.rs", spacing), finding("src/c.rs", spacing)],
			loaded.dispositions,
			reviewed("src/a.rs", "src/b.rs", "src/c.rs"),
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

	test("matches the repository-relative path, including Windows separators, with the rule ID or displayed rule", async () => {
		const root = await projectWith(
			JSON.stringify({
				dispositions: [
					{ file: "./src/a.rs", rule: spacing.id, reason: "Rule ID.", sha256: reviewedHash },
					{ file: "src\\b.rs", rule: "Control flow / Guard clauses", reason: "Displayed rule.", sha256: reviewedHash },
					{ file: "src/c.rs", rule: "Phase spacing", reason: "Title alone is ambiguous.", sha256: reviewedHash },
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
			reviewed("src/a.rs", "src/b.rs", "src/c.rs", "other/src/a.rs"),
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

	test("an entry with a missing or invalid sha256 accepts nothing", async () => {
		const root = await projectWith(
			JSON.stringify({
				dispositions: [
					{ file: "src/a.rs", rule: spacing.id, reason: "No hash." },
					{ file: "src/b.rs", rule: spacing.id, reason: "Uppercase hash.", sha256: reviewedHash.toUpperCase() },
					{ file: "src/c.rs", rule: spacing.id, reason: "Short hash.", sha256: reviewedHash.slice(1) },
				],
			}),
		);

		const loaded = await loadDispositions(root, file);
		const outcome = applyDispositions(
			[finding("src/a.rs", spacing), finding("src/b.rs", spacing), finding("src/c.rs", spacing)],
			loaded.dispositions,
			reviewed("src/a.rs", "src/b.rs", "src/c.rs"),
		);

		expect(loaded.dispositions).toEqual([]);
		expect(loaded.problems).toEqual(
			["src/a.rs", "src/b.rs", "src/c.rs"].map(
				(path, index) =>
					`${file} entry ${index} has no valid sha256 (the lowercase hex SHA-256 of ${path}); it accepts no findings. Re-review the file and record its current hash.`,
			),
		);
		expect(outcome.accepted).toEqual([]);
		expect(outcome.unaccepted).toHaveLength(3);
	});

	test("a matching hash accepts the finding and a changed file makes the disposition stale", async () => {
		const root = await projectWith(
			JSON.stringify({
				dispositions: [
					{ file: "src/a.rs", rule: spacing.id, reason: "Accepted once.", sha256: reviewedHash },
					{ file: "src/b.rs", rule: spacing.id, reason: "Accepted once.", sha256: reviewedHash },
					{ file: "src/c.rs", rule: spacing.id, reason: "Accepted once.", sha256: reviewedHash },
				],
			}),
		);
		const stacked = `${reviewedSource}// Run it again.\nfn again() {}\n`;

		const loaded = await loadDispositions(root, file);
		const outcome = applyDispositions(
			[finding("src/a.rs", spacing), finding("src/b.rs", spacing), finding("src/c.rs", spacing)],
			loaded.dispositions,
			new Map([
				["src/a.rs", reviewedSource],
				["src/b.rs", stacked],
			]),
		);

		expect(loaded.problems).toEqual([]);
		expect(outcome.accepted.map((accepted) => accepted.file)).toEqual(["src/a.rs"]);
		expect(outcome.unaccepted.map((unaccepted) => unaccepted.file)).toEqual(["src/b.rs", "src/c.rs"]);
		expect(outcome.problems).toEqual([
			`src/b.rs changed since this disposition was recorded (rule ${spacing.id}); re-review and update its sha256.`,
			`src/c.rs no longer exists (rule ${spacing.id}); its disposition accepts no findings.`,
		]);
	});

	test("the stamp script fills and refreshes hashes for existing files only", async () => {
		const root = await projectWith(
			JSON.stringify({
				dispositions: [
					{ file: "src/a.rs", rule: spacing.id, reason: "New." },
					{ file: "src\\b.rs", rule: spacing.id, reason: "Stale.", sha256: sha256("fn old() {}\n") },
					{ file: "src/c.rs", rule: spacing.id, reason: "Current.", sha256: reviewedHash },
					{ file: "src/deleted.rs", rule: spacing.id, reason: "Deleted.", sha256: reviewedHash },
				],
			}),
		);
		await Bun.$`git init -q ${root}`;
		await mkdir(join(root, "src"));
		for (const name of ["a.rs", "b.rs", "c.rs"]) {
			await writeFile(join(root, "src", name), reviewedSource);
		}

		const output = await Bun.$`bun ${join(import.meta.dir, "..", "scripts", "stamp-dispositions.ts")} --file ${file}`.cwd(root).text();
		const stamped = JSON.parse(await readFile(join(root, file), "utf8"));

		expect(output).toContain(`stamped entry 0 src/a.rs (rule ${spacing.id}): ${reviewedHash}`);
		expect(output).toContain(`refreshed entry 1 src/b.rs (rule ${spacing.id}): ${reviewedHash}`);
		expect(output).not.toContain("src/c.rs");
		expect(output).toContain("skipped entry 3 src/deleted.rs: file does not exist");
		expect(output).toContain("Updated 2 dispositions");
		expect(stamped.dispositions.map((entry: { sha256: string }) => entry.sha256)).toEqual([
			reviewedHash,
			reviewedHash,
			reviewedHash,
			reviewedHash,
		]);
		expect((await loadDispositions(root, file)).problems).toEqual([]);
	});
});
