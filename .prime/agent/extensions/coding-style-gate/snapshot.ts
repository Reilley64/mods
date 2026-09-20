import { execFile } from "node:child_process";
import { lstat, readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { promisify } from "node:util";
import { createTwoFilesPatch } from "diff";

const execFileAsync = promisify(execFile);

export type RustSnapshot = ReadonlyMap<string, string>;

export interface RustChange {
	path: string;
	before: string | undefined;
	after: string | undefined;
	patch: string;
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
