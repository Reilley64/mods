import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { loadStyleRules } from "../policy";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});

function area(section: string, title: string): string {
	return `# Coding style: ${section}\n\n## ${section}\n\n### ${title}\n\n#### Rule\n\nRule.\n\n#### Violation\n\nViolation.\n\n#### Compliant\n\nCompliant.\n\n#### Bad example\n\n\`\`\`rust\nbad();\n\`\`\`\n\n#### Good example\n\n\`\`\`rust\ngood();\n\`\`\`\n`;
}

async function project(): Promise<string> {
	const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
	temporaryDirectories.push(root);
	return root;
}

describe("coding style policy", () => {
	test("loads every configured area file in order", async () => {
		const root = await project();
		await writeFile(join(root, "errors.md"), area("Errors", "Context propagation"));
		await writeFile(join(root, "control-flow.md"), area("Control flow", "Guard clauses"));

		const rules = await loadStyleRules(root, ["errors.md", "control-flow.md"]);

		expect(rules.map((rule) => rule.id)).toEqual(["errors-context-propagation", "control-flow-guard-clauses"]);
	});

	test("rejects a policy symlink that resolves outside the project", async () => {
		const root = await project();
		const outside = await mkdtemp(join(tmpdir(), "coding-style-gate-outside-"));
		temporaryDirectories.push(outside);
		await writeFile(join(outside, "POLICY.md"), "## Rule\n\nDo not leak this.\n");
		await writeFile(join(root, "errors.md"), area("Errors", "Context propagation"));
		await symlink(join(outside, "POLICY.md"), join(root, "CODING_STYLE.md"));

		await expect(loadStyleRules(root, ["errors.md", "CODING_STYLE.md"])).rejects.toThrow("CODING_STYLE.md must resolve to a file within the project");
	});

	test("rejects an empty list, a file without rules, and a rule ID repeated across files", async () => {
		const root = await project();
		await writeFile(join(root, "index.md"), "# Coding style\n\n## Authority\n\n1. Rust correctness.\n");
		await writeFile(join(root, "errors.md"), area("Errors", "Context propagation"));
		await writeFile(join(root, "errors-copy.md"), area("Errors", "Context propagation"));

		await expect(loadStyleRules(root, [])).rejects.toThrow("styleFiles lists no rubric files");
		await expect(loadStyleRules(root, ["errors.md", "index.md"])).rejects.toThrow("no semantic rules found in index.md");
		await expect(loadStyleRules(root, ["errors.md", "errors-copy.md"])).rejects.toThrow(
			"duplicate rubric rule id errors-context-propagation in errors-copy.md",
		);
	});
});
