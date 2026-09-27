import { readFile } from "node:fs/promises";
import { isAbsolute, join, normalize } from "node:path";

export type GateMode = "advisory" | "enforce";
export type WorktreeScope = "session" | "registered";

export interface GateConfig {
	enabled: boolean;
	mode: GateMode;
	model: string;
	ruleThresholds: Record<string, number>;
	styleFile: string;
	tools: string[];
	timeoutMs: number;
	maxConcurrency: number;
	maxFollowUps: number;
	worktreeScope: WorktreeScope;
	additionalRoots: string[];
}

export const DEFAULT_CONFIG: GateConfig = {
	enabled: true,
	mode: "advisory",
	model: "typesafe/jev-1.13",
	ruleThresholds: {},
	styleFile: "CODING_STYLE.md",
	tools: ["ipython", "edit", "bash"],
	timeoutMs: 10_000,
	maxConcurrency: 4,
	maxFollowUps: 2,
	worktreeScope: "registered",
	additionalRoots: [],
};

export async function loadConfig(root: string, sessionModel?: string): Promise<GateConfig> {
	const path = join(root, ".prime", "agent", "coding-style-gate.json");
	let value: Partial<GateConfig> = {};
	try {
		value = JSON.parse(await readFile(path, "utf8")) as Partial<GateConfig>;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
			throw error;
		}
	}

	if (Object.hasOwn(value, "threshold")) {
		throw new Error("coding-style-gate: legacy threshold is unsupported; migrate to explicit ruleThresholds for every style rule");
	}

	// Legacy provider fields have no routing authority. Override local models before validation.
	const { provider: _legacyProvider, ...local } = value as Partial<GateConfig> & { provider?: unknown };
	const config = { ...DEFAULT_CONFIG, ...local };
	config.model = sessionModel ?? config.model;
	if (config.model === "jev-1.13.0") config.model = DEFAULT_CONFIG.model;
	const normalizedStyleFile = typeof config.styleFile === "string" ? normalize(config.styleFile) : "";
	if (config.mode !== "advisory" && config.mode !== "enforce") {
		throw new Error(`coding-style-gate: invalid mode ${String(config.mode)}`);
	}
	validateRuleThresholds(config.ruleThresholds);
	if (typeof config.enabled !== "boolean") {
		throw new Error("coding-style-gate: enabled must be a boolean");
	}
	if (!["typesafe/jev-1.13", "typesafe/jev-1.13-20260917"].includes(config.model)) {
		throw new Error("coding-style-gate: unsupported OpenRouter model; use typesafe/jev-1.13 or typesafe/jev-1.13-20260917");
	}
	if (
		typeof config.styleFile !== "string" ||
		!config.styleFile ||
		isAbsolute(config.styleFile) ||
		(normalizedStyleFile === ".." || normalizedStyleFile.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`))
	) {
		throw new Error("coding-style-gate: styleFile must stay within the project");
	}
	if (!Number.isInteger(config.timeoutMs) || config.timeoutMs <= 0) {
		throw new Error("coding-style-gate: timeoutMs must be a positive integer");
	}
	if (!Number.isInteger(config.maxConcurrency) || config.maxConcurrency <= 0 || config.maxConcurrency > 16) {
		throw new Error("coding-style-gate: maxConcurrency must be an integer from 1 to 16");
	}
	if (!Number.isInteger(config.maxFollowUps) || config.maxFollowUps < 0 || config.maxFollowUps > 10) {
		throw new Error("coding-style-gate: maxFollowUps must be an integer from 0 to 10");
	}
	if (config.worktreeScope !== "session" && config.worktreeScope !== "registered") {
		throw new Error(`coding-style-gate: invalid worktreeScope ${String(config.worktreeScope)}`);
	}
	if (
		!Array.isArray(config.additionalRoots) ||
		config.additionalRoots.length > 16 ||
		config.additionalRoots.some((root) => typeof root !== "string" || !isAbsolute(root))
	) {
		throw new Error("coding-style-gate: additionalRoots must contain at most 16 absolute paths");
	}
	if (!Array.isArray(config.tools) || config.tools.some((tool) => typeof tool !== "string" || !tool)) {
		throw new Error("coding-style-gate: tools must be a list of tool names");
	}

	return config;
}

export function validateRuleThresholds(
	thresholds: unknown,
	rules: readonly { id: string }[] = [],
): asserts thresholds is Readonly<Record<string, number>> {
	if (typeof thresholds !== "object" || thresholds === null || Array.isArray(thresholds)) {
		throw new Error("coding-style-gate: ruleThresholds must map rule IDs to numeric thresholds between 0 and 1");
	}
	for (const [id, threshold] of Object.entries(thresholds)) {
		if (!id || typeof threshold !== "number" || !Number.isFinite(threshold) || threshold < 0 || threshold > 1) {
			throw new Error(`coding-style-gate: invalid threshold for rule ${id}; expected a finite number between 0 and 1`);
		}
	}
	for (const rule of rules) {
		if (!Object.hasOwn(thresholds, rule.id)) {
			throw new Error(`coding-style-gate: missing threshold for rule ${rule.id}; add an explicit ruleThresholds entry`);
		}
	}
}
