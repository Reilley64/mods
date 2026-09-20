import { execFile } from "node:child_process";
import { lstat, readFile, realpath } from "node:fs/promises";
import { basename, join } from "node:path";
import { promisify } from "node:util";
import { createTwoFilesPatch } from "diff";

const execFileAsync = promisify(execFile);

export type RustSnapshot = ReadonlyMap<string, string>;
export interface RustWorkspaceSnapshot {
	primaryRoot: string;
	snapshots: ReadonlyMap<string, RustSnapshot>;
}

export interface RustWorkspaceOptions {
	includeRegisteredWorktrees: boolean;
	additionalRoots: readonly string[];
}

export interface RustChange {
	path: string;
	before: string | undefined;
	after: string | undefined;
	patch: string;
}

async function availableRegisteredRoot(path: string): Promise<string | undefined> {
	try {
		return await validateGitRoot(path);
	} catch {
		// `git worktree list` retains prunable entries whose directories or `.git`
		// links disappeared. A stale sibling must not disable the live session root.
		return undefined;
	}
}

export async function discoverRegisteredWorktrees(root: string): Promise<string[]> {
	const canonicalRoot = await realpath(root);
	const { stdout } = await execFileAsync("git", ["worktree", "list", "--porcelain", "-z"], {
		cwd: canonicalRoot,
		encoding: "buffer",
		maxBuffer: 1024 * 1024,
	});
	const worktreePaths = stdout
		.toString("utf8")
		.split("\0\0")
		.map((record) => record.split("\0"))
		.filter((fields) => !fields.some((field) => field.startsWith("prunable")))
		.map((fields) => fields.find((field) => field.startsWith("worktree ")))
		.filter((field): field is string => field !== undefined)
		.map((field) => field.slice("worktree ".length));
	const discovered = await Promise.all(worktreePaths.map(availableRegisteredRoot));
	return [
		canonicalRoot,
		...discovered.filter((path): path is string => path !== undefined && path !== canonicalRoot).sort(),
	];
}

async function validateGitRoot(root: string): Promise<string> {
	const canonicalRoot = await realpath(root);
	const { stdout } = await execFileAsync("git", ["rev-parse", "--show-toplevel"], {
		cwd: canonicalRoot,
		encoding: "utf8",
		maxBuffer: 1024 * 1024,
	});
	const repositoryRoot = await realpath(stdout.trim());
	if (repositoryRoot !== canonicalRoot) {
		throw new Error(`coding-style-gate: watched root is not a Git worktree root: ${root}`);
	}
	return canonicalRoot;
}

export async function discoverWatchedRoots(root: string, options: RustWorkspaceOptions): Promise<string[]> {
	const canonicalRoot = await validateGitRoot(root);
	const registered = options.includeRegisteredWorktrees ? await discoverRegisteredWorktrees(canonicalRoot) : [canonicalRoot];
	const additional = await Promise.all(options.additionalRoots.map(validateGitRoot));
	const externalRoots = new Set([...registered, ...additional]);
	externalRoots.delete(canonicalRoot);

	return [canonicalRoot, ...[...externalRoots].sort()];
}

export async function captureRustIndexSnapshot(root: string): Promise<RustSnapshot> {
	const { stdout } = await execFileAsync("git", ["ls-files", "--cached", "--stage", "-z", "--", "*.rs"], {
		cwd: root,
		encoding: "buffer",
		maxBuffer: 16 * 1024 * 1024,
	});
	const entries = stdout
		.toString("utf8")
		.split("\0")
		.filter(Boolean)
		.map((entry) => {
			const separator = entry.indexOf("\t");
			const [mode, object, stage] = entry.slice(0, separator).split(" ");
			return { mode, object: object!, path: entry.slice(separator + 1), stage };
		})
		.filter(({ mode, stage }) => mode !== "120000" && stage === "0");
	const files = new Map<string, string>();
	for (const { object, path } of entries) {
		const { stdout: content } = await execFileAsync("git", ["cat-file", "blob", object], {
			cwd: root,
			encoding: "buffer",
			maxBuffer: 16 * 1024 * 1024,
		});
		files.set(path, content.toString("utf8"));
	}
	return files;
}

export async function captureRustWorkspaceSnapshot(
	root: string,
	options: RustWorkspaceOptions,
): Promise<RustWorkspaceSnapshot> {
	const worktrees = await discoverWatchedRoots(root, options);
	const snapshots = new Map(
		await Promise.all(worktrees.map(async (worktree) => [worktree, await captureRustSnapshot(worktree)] as const)),
	);
	return { primaryRoot: worktrees[0]!, snapshots };
}

export async function captureRustSnapshot(root: string): Promise<RustSnapshot> {
	const { stdout } = await execFileAsync(
		"git",
		["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.rs"],
		{ cwd: root, encoding: "buffer", maxBuffer: 16 * 1024 * 1024 },
	);
	const snapshot = new Map<string, string>();

	for (const path of stdout.toString("utf8").split("\0").filter(Boolean).sort()) {
		try {
			const absolutePath = join(root, path);
			if (!(await lstat(absolutePath)).isFile()) {
				continue;
			}
			snapshot.set(path, await readFile(absolutePath, "utf8"));
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
				throw error;
			}
		}
	}

	return snapshot;
}

export function diffRustSnapshots(before: RustSnapshot, after: RustSnapshot): RustChange[] {
	const paths = [...new Set([...before.keys(), ...after.keys()])].sort();
	const changes: RustChange[] = [];

	for (const path of paths) {
		const previous = before.get(path);
		const current = after.get(path);
		if (previous === current) {
			continue;
		}

		changes.push({
			path,
			before: previous,
			after: current,
			patch: createTwoFilesPatch(
				previous === undefined ? "/dev/null" : `a/${path}`,
				current === undefined ? "/dev/null" : `b/${path}`,
				previous ?? "",
				current ?? "",
				undefined,
				undefined,
				{ context: 20 },
			),
		});
	}

	return changes;
}

export function findModuleReferencingFiles(snapshot: RustSnapshot, path: string): string[] {
	if (!snapshot.has(path)) {
		return [];
	}
	const moduleName = basename(path, ".rs");
	if (["lib", "main", "mod"].includes(moduleName)) {
		return [];
	}
	const sourceMarker = path.indexOf("/src/");
	const sourceRoot = sourceMarker === -1 ? "" : path.slice(0, sourceMarker + "/src/".length);
	const escapedName = moduleName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	const reference = new RegExp(
		String.raw`(?:\bmod\s+${escapedName}\b|\b${escapedName}::|::${escapedName}(?:::|\b)|\buse\s+[^;\n]*\b${escapedName}\b)`,
	);
	return [...snapshot.entries()]
		.filter(
			([candidatePath, content]) =>
				candidatePath !== path && (!sourceRoot || candidatePath.startsWith(sourceRoot)) && reference.test(content),
		)
		.map(([candidatePath]) => candidatePath)
		.sort();
}

export function declaresFileNamedEntryPoint(change: RustChange): boolean {
	if (change.after === undefined) {
		return false;
	}
	const functionName = basename(change.path, ".rs");
	if (["lib", "main", "mod"].includes(functionName)) {
		return false;
	}
	const escapedName = functionName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
	return new RegExp(String.raw`\bpub(?:\([^)]*\))?\s+(?:async\s+)?fn\s+${escapedName}\b`).test(change.after);
}
