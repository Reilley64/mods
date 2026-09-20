import { afterEach, describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { loadConfig } from "../config";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});

describe("coding style gate configuration", () => {
	test("rejects a style file outside the project", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await mkdir(join(root, ".prime", "agent"), { recursive: true });
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ styleFile: "../SECRET.md" }));

		await expect(loadConfig(root)).rejects.toThrow("within the project");
	});

	test("uses the calibrated threshold by default", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);

		const config = await loadConfig(root);
		expect(config.threshold).toBe(0.86);
		expect(config.ruleThresholds).toEqual({
			"application-use-cases-and-ports-use-case-local-implementation-modules": 0.45,
		});
	});

});
