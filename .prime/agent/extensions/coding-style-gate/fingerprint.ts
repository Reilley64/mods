import { createHash } from "node:crypto";

import { backendIdentity } from "./provider";
import { REQUEST_VERSION } from "./cache";
import { declaresFileNamedEntryPoint } from "./snapshot";

import type { GateConfig } from "./config";
import type { StyleRule } from "./rules";
import type { RustChange, RustSnapshot } from "./snapshot";

function hash(parts: readonly string[]): string {
	const digest = createHash("sha256");
	for (const part of parts) {
		digest.update(String(part.length));
		digest.update(":");
		digest.update(part);
	}
	return digest.digest("hex");
}

export function snapshotFingerprint(snapshot: RustSnapshot): string {
	return hash([...snapshot.entries()].sort(([left], [right]) => left.localeCompare(right)).flatMap(([path, content]) => [path, content]));
}

export function reviewFingerprint(
	changes: readonly RustChange[],
	rules: readonly StyleRule[],
	config: GateConfig,
	moduleReferences: ReadonlyMap<string, readonly string[]> = new Map(),
): string {
	return hash([
		config.model,
		backendIdentity(),
		String(REQUEST_VERSION),
		...Object.entries(config.ruleThresholds)
			.sort(([left], [right]) => left.localeCompare(right))
			.flatMap(([id, threshold]) => [id, String(threshold)]),
		...rules.flatMap((rule) => [rule.id, rule.title, rule.text, rule.violation, rule.compliant, ...rule.badExamples, ...rule.goodExamples]),
		...changes.flatMap((change) => [
			change.path,
			change.before === undefined ? "added" : change.after === undefined ? "deleted" : "modified",
			String(declaresFileNamedEntryPoint(change)),
			change.patch,
			change.after ?? "",
			...(moduleReferences.get(change.path) ?? []),
		]),
	]);
}
