import { readFile, realpath } from "node:fs/promises";
import { isAbsolute, join, relative } from "node:path";

import { extractStyleRules, type StyleRule } from "./rules";

export async function loadStyleRules(root: string, styleFiles: readonly string[]): Promise<StyleRule[]> {
	if (styleFiles.length === 0) {
		throw new Error("coding-style-gate: styleFiles lists no rubric files");
	}
	const canonicalRoot = await realpath(root);
	const rules: StyleRule[] = [];
	for (const styleFile of styleFiles) {
		const policyPath = await realpath(join(root, styleFile));
		const pathFromRoot = relative(canonicalRoot, policyPath);
		if (!pathFromRoot || pathFromRoot === ".." || pathFromRoot.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) || isAbsolute(pathFromRoot)) {
			throw new Error(`coding-style-gate: style file ${styleFile} must resolve to a file within the project`);
		}
		const fileRules = extractStyleRules(await readFile(policyPath, "utf8"));
		if (fileRules.length === 0) {
			throw new Error(`coding-style-gate: no semantic rules found in ${styleFile}`);
		}
		for (const rule of fileRules) {
			if (rules.some((existing) => existing.id === rule.id)) {
				throw new Error(`coding-style-gate: duplicate rubric rule id ${rule.id} in ${styleFile}`);
			}
			rules.push(rule);
		}
	}
	return rules;
}
