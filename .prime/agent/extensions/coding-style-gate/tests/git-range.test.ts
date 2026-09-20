import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { loadGitRange } from "../calibration/git-range";
import { findModuleReferencingFiles } from "../snapshot";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});

describe("retroactive Git range loading", () => {
	test("uses the merge base and includes unchanged Rust callers as reference evidence", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-range-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await Bun.$`git -C ${root} config user.email test@example.com`;
		await Bun.$`git -C ${root} config user.name Test`;
		await writeFile(join(root, "caller.rs"), "mod helper;\nfn call() { helper::run(); }\n");
		await writeFile(join(root, "helper.rs"), "pub fn run() { let _version = 0; }\n");
		await Bun.$`git -C ${root} add caller.rs helper.rs`;
		await Bun.$`git -C ${root} commit -qm base`;
		const mergeBase = (await Bun.$`git -C ${root} rev-parse HEAD`.text()).trim();

		await Bun.$`git -C ${root} checkout -qb head-branch`;
		await writeFile(join(root, "helper.rs"), "pub fn run() { let _version = 1; }\n");
		await Bun.$`git -C ${root} commit -qam head`;
		const head = (await Bun.$`git -C ${root} rev-parse HEAD`.text()).trim();

		await Bun.$`git -C ${root} checkout -q ${mergeBase} -b other-branch`;
		await writeFile(join(root, "helper.rs"), "pub fn run() { let _version = 2; }\n");
		await Bun.$`git -C ${root} commit -qam other`;
		const requestedBase = (await Bun.$`git -C ${root} rev-parse HEAD`.text()).trim();

		const loaded = await loadGitRange(root, requestedBase, head);

		expect(loaded.effectiveBase).toBe(mergeBase);
		expect(loaded.changes).toHaveLength(1);
		expect(loaded.changes[0]?.before).toContain("_version = 0");
		expect(loaded.changes[0]?.after).toContain("_version = 1");
		expect([...loaded.completeAfter.keys()].sort()).toEqual(["caller.rs", "helper.rs"]);
		expect(findModuleReferencingFiles(loaded.completeAfter, "helper.rs")).toEqual(["caller.rs"]);
	});
});
