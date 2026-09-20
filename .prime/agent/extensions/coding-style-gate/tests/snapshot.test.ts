import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { captureRustSnapshot, declaresFileNamedEntryPoint, diffRustSnapshots, findModuleReferencingFiles } from "../snapshot";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});

describe("Rust filesystem snapshots", () => {
	test("reports only Rust files changed between two snapshots", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeFile(join(root, "existing.rs"), "fn answer() -> u8 { 1 }\n");
		await writeFile(join(root, "notes.txt"), "before\n");
		await Bun.$`git -C ${root} add existing.rs notes.txt`;

		const before = await captureRustSnapshot(root);
		await writeFile(join(root, "existing.rs"), "fn answer() -> u8 { 2 }\n");
		await writeFile(join(root, "created.rs"), "fn created() {}\n");
		await writeFile(join(root, "notes.txt"), "after\n");
		const after = await captureRustSnapshot(root);

		const changes = diffRustSnapshots(before, after);

		expect(changes.map((change) => change.path)).toEqual(["created.rs", "existing.rs"]);
		expect(changes[0]).toMatchObject({ before: undefined, after: "fn created() {}\n" });
		expect(changes[1]?.patch).toContain("-fn answer() -> u8 { 1 }");
		expect(changes[1]?.patch).toContain("+fn answer() -> u8 { 2 }");
	});

	test("does not report dirty files that remain unchanged during the tool call", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeFile(join(root, "dirty.rs"), "fn dirty() {}\n");
		await Bun.$`git -C ${root} add dirty.rs`;
		await writeFile(join(root, "dirty.rs"), "fn already_dirty() {}\n");

		const before = await captureRustSnapshot(root);
		const after = await captureRustSnapshot(root);

		expect(diffRustSnapshots(before, after)).toEqual([]);
	});

	test("does not follow Rust symlinks outside the repository", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		const outside = await mkdtemp(join(tmpdir(), "coding-style-gate-outside-"));
		temporaryDirectories.push(root, outside);
		await Bun.$`git init -q ${root}`;
		await writeFile(join(outside, "secret.rs"), "const SECRET: &str = \"outside\";\n");
		await symlink(join(outside, "secret.rs"), join(root, "linked.rs"));
		await Bun.$`git -C ${root} add linked.rs`;

		const snapshot = await captureRustSnapshot(root);

		expect(snapshot.has("linked.rs")).toBeFalse();
	});


	test("finds repository files that reference a changed module name", () => {
		const snapshot = new Map([
			["application/src/installation/mod.rs", "mod fomod;\nmod install_archive;\n"],
			["application/src/installation/install_archive.rs", "use super::fomod;\nfn run() { fomod::evaluate(); }\n"],
			["application/src/installation/fomod.rs", "pub(super) fn evaluate() {}\n"],
			["application/src/settings/mod.rs", "mod types;\nstruct FomodInstaller;\n"],
			["infrastructure/archive/src/lib.rs", "mod fomod;\n"],
		]);

		expect(findModuleReferencingFiles(snapshot, "application/src/installation/fomod.rs")).toEqual([
			"application/src/installation/install_archive.rs",
			"application/src/installation/mod.rs",
		]);
	});


	test("does not infer current module references for a deleted path", () => {
		const snapshot = new Map([
			["application/src/installation/install_archive.rs", "mod fomod;\n"],
			["application/src/installation/install_archive/fomod.rs", "pub(super) fn evaluate() {}\n"],
		]);

		expect(findModuleReferencingFiles(snapshot, "application/src/installation/fomod.rs")).toEqual([]);
	});


	test("recognizes a public function matching its use-case file name", () => {
		expect(
			declaresFileNamedEntryPoint({
				path: "application/src/settings/get_setting.rs",
				before: "",
				after: "pub async fn get_setting() {}\n",
				patch: "",
			}),
		).toBeTrue();
		expect(
			declaresFileNamedEntryPoint({
				path: "application/src/installation/fomod.rs",
				before: "",
				after: "pub(super) fn evaluate() {}\n",
				patch: "",
			}),
		).toBeFalse();
	});

});
