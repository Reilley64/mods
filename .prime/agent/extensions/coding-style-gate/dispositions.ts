import { readFile, realpath } from "node:fs/promises";
import { isAbsolute, join, posix, relative } from "node:path";

import type { StyleFinding } from "./reviewer";

export interface Disposition {
	file: string;
	rule: string;
	reason: string;
}

export interface DispositionSet {
	dispositions: Disposition[];
	problems: string[];
}

export interface DispositionOutcome {
	accepted: StyleFinding[];
	unaccepted: StyleFinding[];
}

// Git reports repository paths with forward slashes; dispositions may be written on Windows.
function repositoryPath(path: string): string {
	return posix.normalize(path.replace(/\\/g, "/")).replace(/^(\.\/)+/, "");
}

export async function loadDispositions(root: string, dispositionsFile: string): Promise<DispositionSet> {
	let text: string;
	try {
		const canonicalRoot = await realpath(root);
		const path = await realpath(join(root, dispositionsFile));
		const pathFromRoot = relative(canonicalRoot, path);
		if (!pathFromRoot || pathFromRoot === ".." || pathFromRoot.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) || isAbsolute(pathFromRoot)) {
			return { dispositions: [], problems: [`${dispositionsFile} must resolve to a file within the project; no findings were accepted from it.`] };
		}
		text = await readFile(path, "utf8");
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") {
			return { dispositions: [], problems: [] };
		}
		return { dispositions: [], problems: [`${dispositionsFile} could not be read (${error instanceof Error ? error.message : String(error)}); no findings were accepted from it.`] };
	}

	let value: unknown;
	try {
		value = JSON.parse(text);
	} catch (error) {
		return { dispositions: [], problems: [`${dispositionsFile} is not valid JSON (${error instanceof Error ? error.message : String(error)}); no findings were accepted from it.`] };
	}
	const entries = (value as { dispositions?: unknown } | null)?.dispositions;
	if (typeof value !== "object" || value === null || Array.isArray(value) || !Array.isArray(entries)) {
		return { dispositions: [], problems: [`${dispositionsFile} must contain a "dispositions" array; no findings were accepted from it.`] };
	}

	const dispositions: Disposition[] = [];
	const problems: string[] = [];
	for (const [index, entry] of entries.entries()) {
		const { file, rule, reason } = (typeof entry === "object" && entry !== null ? entry : {}) as Record<string, unknown>;
		const missing = [["file", file], ["rule", rule], ["reason", reason]]
			.filter(([, field]) => typeof field !== "string" || !field.trim())
			.map(([name]) => name);
		if (missing.length > 0) {
			problems.push(`${dispositionsFile} entry ${index} has no non-empty ${missing.join(", ")}; it accepts no findings.`);
			continue;
		}
		const path = repositoryPath(file as string);
		if (isAbsolute(file as string) || path === ".." || path.startsWith("../")) {
			problems.push(`${dispositionsFile} entry ${index} file must be repository-relative; it accepts no findings.`);
			continue;
		}
		dispositions.push({ file: path, rule: (rule as string).trim(), reason: (reason as string).trim() });
	}
	return { dispositions, problems };
}

export function applyDispositions(findings: readonly StyleFinding[], dispositions: readonly Disposition[]): DispositionOutcome {
	const accepted: StyleFinding[] = [];
	const unaccepted: StyleFinding[] = [];
	for (const finding of findings) {
		const file = repositoryPath(finding.file);
		const matches = dispositions.some(
			(disposition) =>
				disposition.file === file &&
				(disposition.rule === finding.rule.id || disposition.rule === `${finding.rule.section} / ${finding.rule.title}`),
		);
		(matches ? accepted : unaccepted).push(finding);
	}
	return { accepted, unaccepted };
}
