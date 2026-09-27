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

	test("rejects relative additional roots", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await mkdir(join(root, ".prime", "agent"), { recursive: true });
		await writeFile(
			join(root, ".prime", "agent", "coding-style-gate.json"),
			JSON.stringify({ additionalRoots: ["../other"] }),
		);

		await expect(loadConfig(root)).rejects.toThrow("absolute paths");
	});

	test("limits the number of additional roots", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await mkdir(join(root, ".prime", "agent"), { recursive: true });
		await writeFile(
			join(root, ".prime", "agent", "coding-style-gate.json"),
			JSON.stringify({ additionalRoots: Array.from({ length: 17 }, (_, index) => join(root, String(index))) }),
		);

		await expect(loadConfig(root)).rejects.toThrow("at most 16");
	});

	test("has no implicit thresholds", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);

		const config = await loadConfig(root);
		expect(config.ruleThresholds).toEqual({});
		expect(config.worktreeScope).toBe("registered");
		expect(config.additionalRoots).toEqual([]);
	});

	test("rejects legacy scalar thresholds with migration instructions", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await mkdir(join(root, ".prime", "agent"), { recursive: true });
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ threshold: 0.8 }));
		await expect(loadConfig(root)).rejects.toThrow("ruleThresholds");
	});

	test("rejects invalid per-rule config thresholds", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await mkdir(join(root, ".prime", "agent"), { recursive: true });
		for (const threshold of [null, "0.8", -0.1, 1.1]) {
			await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds: { rule: threshold } }));
			await expect(loadConfig(root)).rejects.toThrow("threshold");
		}
	});

});

test("migrates legacy session models, rejects unknown session IDs, and ignores local model before validation", async () => {
	const root = await mkdtemp(join(tmpdir(), "coding-style-model-")); temporaryDirectories.push(root);
	await mkdir(join(root, ".prime/agent"), { recursive: true });
	const path = join(root, ".prime/agent/coding-style-gate.json");
	await writeFile(path, JSON.stringify({ provider: "typesafe", model: "jev-1.13.0" }));
	expect((await loadConfig(root)).model).toBe("typesafe/jev-1.13");
	await writeFile(path, JSON.stringify({ provider: "typesafe", model: { invalid: true } }));
	await expect(loadConfig(root)).rejects.toThrow("unsupported OpenRouter model");
	expect((await loadConfig(root, "typesafe/jev-1.13")).model).toBe("typesafe/jev-1.13");
});
