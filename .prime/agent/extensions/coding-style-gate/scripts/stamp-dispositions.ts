// Records the current content hash on each disposition whose file exists.
// Run it only after re-reviewing those files: a stamped disposition accepts the file's current content.
//
// Usage: bun .prime/agent/extensions/coding-style-gate/scripts/stamp-dispositions.ts [--file <dispositions.json>]
import { lstat, readFile, writeFile } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";
import { parseArgs } from "node:util";

import { loadConfig } from "../config";
import { contentSha256, repositoryPath } from "../dispositions";

const { values } = parseArgs({ options: { file: { type: "string" } } });
const root = (await Bun.$`git rev-parse --show-toplevel`.text()).trim();
const dispositionsPath = values.file === undefined ? join(root, (await loadConfig(root)).dispositionsFile) : resolve(values.file);

const document = JSON.parse(await readFile(dispositionsPath, "utf8")) as { dispositions?: unknown };
if (!Array.isArray(document?.dispositions)) {
	throw new Error(`${dispositionsPath} must contain a "dispositions" array`);
}

let changed = 0;
for (const [index, entry] of document.dispositions.entries()) {
	if (typeof entry !== "object" || entry === null || typeof entry.file !== "string" || !entry.file.trim()) {
		console.log(`skipped entry ${index}: no file`);
		continue;
	}
	const file = repositoryPath(entry.file);
	if (isAbsolute(entry.file) || file === ".." || file.startsWith("../")) {
		console.log(`skipped entry ${index} ${entry.file}: not repository-relative`);
		continue;
	}
	const path = join(root, file);
	// The gate reviews only regular files, so a missing file or symlink has no content to stamp.
	const exists = await lstat(path).then((stats) => stats.isFile(), () => false);
	if (!exists) {
		console.log(`skipped entry ${index} ${file}: file does not exist`);
		continue;
	}
	const sha256 = contentSha256(await readFile(path, "utf8"));
	if (entry.sha256 === sha256) {
		continue;
	}
	console.log(`${entry.sha256 === undefined ? "stamped" : "refreshed"} entry ${index} ${file} (rule ${String(entry.rule)}): ${sha256}`);
	entry.sha256 = sha256;
	changed += 1;
}

if (changed === 0) {
	console.log(`No dispositions changed in ${dispositionsPath}.`);
} else {
	await writeFile(dispositionsPath, `${JSON.stringify(document, null, 2)}\n`);
	console.log(`Updated ${changed} ${changed === 1 ? "disposition" : "dispositions"} in ${dispositionsPath}.`);
}
