import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { diffRustSnapshots, type RustChange, type RustSnapshot } from "../snapshot";

const execFileAsync = promisify(execFile);

export interface LoadedGitRange {
	changes: RustChange[];
	completeAfter: RustSnapshot;
	effectiveBase: string;
	head: string;
	requestedBase: string;
}

async function contentAt(root: string, revision: string, path: string): Promise<string | undefined> {
	try {
		const result = await execFileAsync("git", ["show", `${revision}:${path}`], {
			cwd: root,
			encoding: "buffer",
			maxBuffer: 16 * 1024 * 1024,
		});
		return result.stdout.toString("utf8");
	} catch (error) {
		const stderr = String((error as { stderr?: unknown }).stderr ?? "");
		if (stderr.includes("does not exist in") || stderr.includes("exists on disk, but not in")) return undefined;
		throw error;
	}
}

export async function loadGitRange(
	root: string,
	requestedBase: string,
	head: string,
	pathFilters: readonly string[] = [],
): Promise<LoadedGitRange> {
	const mergeBaseResult = await execFileAsync("git", ["merge-base", requestedBase, head], {
		cwd: root,
		encoding: "utf8",
	});
	const effectiveBase = mergeBaseResult.stdout.trim();
	if (!/^[0-9a-f]{40}$/.test(effectiveBase)) {
		throw new Error("git merge-base did not return a full commit OID");
	}
	const { stdout } = await execFileAsync(
		"git",
		["diff", "--name-only", "--no-renames", "-z", `${effectiveBase}..${head}`, "--", "*.rs"],
		{ cwd: root, encoding: "buffer", maxBuffer: 16 * 1024 * 1024 },
	);
	const changedPaths = stdout.toString("utf8").split("\0").filter(Boolean).sort();
	const paths = pathFilters.length > 0 ? changedPaths.filter((path) => pathFilters.includes(path)) : changedPaths;
	if (pathFilters.length > 0 && paths.length !== pathFilters.length) {
		throw new Error("one or more requested paths are not Rust changes in the supplied range");
	}

	const before = new Map<string, string>();
	const changedAfter = new Map<string, string>();
	await Promise.all(
		paths.flatMap((path) => [
			contentAt(root, effectiveBase, path).then((content) => {
				if (content !== undefined) before.set(path, content);
			}),
			contentAt(root, head, path).then((content) => {
				if (content !== undefined) changedAfter.set(path, content);
			}),
		]),
	);
	const changes = diffRustSnapshots(before, changedAfter);

	const treeResult = await execFileAsync("git", ["ls-tree", "-r", "-z", "--name-only", head], {
		cwd: root,
		encoding: "buffer",
		maxBuffer: 16 * 1024 * 1024,
	});
	const headRustPaths = treeResult.stdout
		.toString("utf8")
		.split("\0")
		.filter((path) => path.endsWith(".rs"))
		.sort();
	const completeAfter = new Map<string, string>();
	await Promise.all(
		headRustPaths.map(async (path) => {
			const content = await contentAt(root, head, path);
			if (content !== undefined) completeAfter.set(path, content);
		}),
	);

	return { changes, completeAfter, effectiveBase, head, requestedBase };
}
