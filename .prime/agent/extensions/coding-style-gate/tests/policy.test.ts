import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { loadStyleRules } from "../policy";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});

describe("coding style policy", () => {
	test("rejects a policy symlink that resolves outside the project", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		const outside = await mkdtemp(join(tmpdir(), "coding-style-gate-outside-"));
		temporaryDirectories.push(root, outside);
		await writeFile(join(outside, "POLICY.md"), "## Rule\n\nDo not leak this.\n");
		await symlink(join(outside, "POLICY.md"), join(root, "CODING_STYLE.md"));

		await expect(loadStyleRules(root, "CODING_STYLE.md")).rejects.toThrow("within the project");
	});
});
