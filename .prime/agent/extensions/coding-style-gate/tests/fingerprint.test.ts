import { describe, expect, test } from "bun:test";

import { DEFAULT_CONFIG } from "../config";
import { reviewFingerprint } from "../fingerprint";
import type { StyleRule } from "../rules";

const rule: StyleRule = {
	id: "application-use-cases-and-ports-use-case-parameters",
	section: "Application use cases and ports",
	title: "Use-case parameters",
	text: "Pass dependencies first.",
	violation: "Dependencies are not first.",
	compliant: "Dependencies are first.",
	badExamples: ["fn run(path: PathBuf, dependencies: Dependencies) {}"],
	goodExamples: ["fn run(dependencies: Dependencies, path: PathBuf) {}"],
};
const changes = [{ path: "src/application/src/run.rs", after: "fn run() {}\n", patch: "+fn run() {}" }];

describe("review fingerprint", () => {
	test("changes when a rule's Applies to scope changes", () => {
		const unscoped = reviewFingerprint(changes, [rule], DEFAULT_CONFIG);
		const scoped = reviewFingerprint(changes, [{ ...rule, appliesTo: ["src/application/**"] }], DEFAULT_CONFIG);
		const rescoped = reviewFingerprint(changes, [{ ...rule, appliesTo: ["src/domain/**"] }], DEFAULT_CONFIG);

		expect(new Set([unscoped, scoped, rescoped]).size).toBe(3);
	});
});
