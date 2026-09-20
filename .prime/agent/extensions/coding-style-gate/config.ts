import { readFile } from "node:fs/promises";
import { isAbsolute, join, normalize } from "node:path";

export type GateMode = "advisory" | "enforce";

export interface GateConfig {
	enabled: boolean;
	mode: GateMode;
	model: string;
	threshold: number;
	ruleThresholds: Record<string, number>;
	styleFile: string;
	tools: string[];
	timeoutMs: number;
	maxConcurrency: number;
	maxFollowUps: number;
}

export const DEFAULT_CONFIG: GateConfig = {
	enabled: true,
	mode: "advisory",
	model: "jev-1.13.0",
	threshold: 0.86,
	ruleThresholds: {
		"application-use-cases-and-ports-use-case-local-implementation-modules": 0.45,
	},
	styleFile: "CODING_STYLE.md",
	tools: ["ipython", "edit", "bash"],
	timeoutMs: 10_000,
	maxConcurrency: 4,
	maxFollowUps: 2,
};

export async function loadConfig(root: string): Promise<GateConfig> {
	const path = join(root, ".prime", "agent", "coding-style-gate.json");
	let value: Partial<GateConfig> = {};
	try {
		value = JSON.parse(await readFile(path, "utf8")) as Partial<GateConfig>;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
			throw error;
		}
	}

	const config = { ...DEFAULT_CONFIG, ...value };
	const normalizedStyleFile = typeof config.styleFile === "string" ? normalize(config.styleFile) : "";
	if (config.mode !== "advisory" && config.mode !== "enforce") {
		throw new Error(`coding-style-gate: invalid mode ${String(config.mode)}`);
	}
	if (!Number.isFinite(config.threshold) || config.threshold < 0 || config.threshold > 1) {
		throw new Error("coding-style-gate: threshold must be between 0 and 1");
	}
	if (
		typeof config.ruleThresholds !== "object" ||
		config.ruleThresholds === null ||
		Array.isArray(config.ruleThresholds) ||
		Object.entries(config.ruleThresholds).some(
			([id, threshold]) => !id || typeof threshold !== "number" || !Number.isFinite(threshold) || threshold < 0 || threshold > 1,
		)
	) {
		throw new Error("coding-style-gate: ruleThresholds must map rule IDs to thresholds between 0 and 1");
	}
	if (typeof config.enabled !== "boolean") {
		throw new Error("coding-style-gate: enabled must be a boolean");
	}
	if (typeof config.model !== "string" || !/^[a-zA-Z0-9._:-]{1,100}$/.test(config.model)) {
		throw new Error("coding-style-gate: model must be a safe model identifier");
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
	if (!Array.isArray(config.tools) || config.tools.some((tool) => typeof tool !== "string" || !tool)) {
		throw new Error("coding-style-gate: tools must be a list of tool names");
	}

	return config;
}
