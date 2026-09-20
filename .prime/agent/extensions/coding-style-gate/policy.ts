import { readFile, realpath } from "node:fs/promises";
import { isAbsolute, join, relative } from "node:path";

import { extractStyleRules, type StyleRule } from "./rules";

export async function loadStyleRules(root: string, styleFile: string): Promise<StyleRule[]> {
	const canonicalRoot = await realpath(root);
	const policyPath = await realpath(join(root, styleFile));
	const pathFromRoot = relative(canonicalRoot, policyPath);
	if (!pathFromRoot || pathFromRoot === ".." || pathFromRoot.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) || isAbsolute(pathFromRoot)) {
		throw new Error("coding-style-gate: styleFile must resolve to a file within the project");
	}
	const rules = extractStyleRules(await readFile(policyPath, "utf8"));
	if (rules.length === 0) {
		throw new Error(`coding-style-gate: no semantic rules found in ${styleFile}`);
	}
	return rules;
}
