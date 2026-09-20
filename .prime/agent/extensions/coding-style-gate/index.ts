import { TypeSafeClient } from "@typesafe-ai/sdk";
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { join } from "node:path";

import { type GateConfig, loadConfig } from "./config";
import { reviewFingerprint, snapshotFingerprint } from "./fingerprint";
import { formatReview } from "./format";
import { reviewChanges, type StyleReviewReport } from "./reviewer";
import { loadStyleRules } from "./policy";
import type { StyleRule } from "./rules";
import {
	captureRustIndexSnapshot,
	captureRustWorkspaceSnapshot,
	diffRustSnapshots,
	findModuleReferencingFiles,
	type RustChange,
	type RustSnapshot,
	type RustWorkspaceSnapshot,
} from "./snapshot";

const MESSAGE_TYPE = "coding-style-gate";
const REVIEW_FAILED = "coding-style-gate could not review the Rust changes. Use /coding-style-gate status for details.";

interface PendingSnapshot {
	config: GateConfig;
	snapshot: RustWorkspaceSnapshot;
}

interface PreparedReview {
	changes: RustChange[];
	fingerprint: string;
	moduleReferences: ReadonlyMap<string, readonly string[]>;
	rules: StyleRule[];
}

interface CompletedReview {
	fingerprint: string;
	report: StyleReviewReport;
}

interface BlockedState {
	fingerprint: string;
}

interface OverrideState {
	fingerprint: string;
	reason: string;
}

interface GateState {
	activeConfig?: GateConfig;
	baselineCaptureFailed?: boolean;
	blocked?: BlockedState;
	exhaustedNoticeSent?: boolean;
	followUps?: number;
	lastError?: string;
	lastReport?: StyleReviewReport;
	override?: OverrideState;
	taskBaseline?: RustWorkspaceSnapshot;
}

class ReviewFailure extends Error {
	constructor(
		message: string,
		readonly fingerprint: string,
	) {
		super(message);
	}
}

function errorText(error: unknown): string {
	return error instanceof Error ? error.message.replace(/[\r\n]+/g, " ").slice(0, 500) : String(error).slice(0, 500);
}

function resultMessage(text: string) {
	return { type: "text" as const, text };
}

function runWasAborted(messages: readonly unknown[]): boolean {
	for (let index = messages.length - 1; index >= 0; index -= 1) {
		const message = messages[index];
		if (typeof message !== "object" || message === null || (message as { role?: unknown }).role !== "assistant") {
			continue;
		}
		const stopReason = (message as { stopReason?: unknown }).stopReason;
		return stopReason === "aborted" || stopReason === "error";
	}
	return false;
}

export default function codingStyleGate(pi: ExtensionAPI): void {
	const pendingSnapshots = new Map<string, PendingSnapshot>();
	const reviewCache = new Map<string, Promise<StyleReviewReport>>();
	const state: GateState = {};
	let reviewTail = Promise.resolve();

	async function serial<T>(operation: () => Promise<T>): Promise<T> {
		const run = reviewTail.then(operation, operation);
		reviewTail = run.then(
			() => undefined,
			() => undefined,
		);
		return run;
	}


	async function prepareReview(
		root: string,
		before: RustSnapshot,
		after: RustSnapshot,
		config: GateConfig,
	): Promise<PreparedReview | undefined> {
		const changes = diffRustSnapshots(before, after);
		if (changes.length === 0) {
			return undefined;
		}
		const rules = await loadStyleRules(root, config.styleFile);
		const moduleReferences = new Map(
			changes.map((change) => [change.path, findModuleReferencingFiles(after, change.path)]),
		);
		return {
			changes,
			rules,
			moduleReferences,
			fingerprint: reviewFingerprint(changes, rules, config, moduleReferences),
		};
	}

	async function inspect(
		root: string,
		before: RustSnapshot,
		after: RustSnapshot,
		config: GateConfig,
		signal?: AbortSignal,
	): Promise<CompletedReview | undefined> {
		const prepared = await prepareReview(root, before, after, config);
		if (prepared === undefined) {
			return undefined;
		}

		let request = reviewCache.get(prepared.fingerprint);
		if (request === undefined) {
			const client = new TypeSafeClient({
				defaultModel: config.model,
				logLevel: "warn",
				retry: { maxRetries: 1 },
				timeout: config.timeoutMs,
			});
			request = reviewChanges(client, prepared.changes, prepared.rules, {
				maxConcurrency: config.maxConcurrency,
				moduleReferences: prepared.moduleReferences,
				model: config.model,
				threshold: config.threshold,
				ruleThresholds: config.ruleThresholds,
				signal,
			});
			reviewCache.set(prepared.fingerprint, request);
		}

		try {
			return { fingerprint: prepared.fingerprint, report: await request };
		} catch (error) {
			reviewCache.delete(prepared.fingerprint);
			throw new ReviewFailure(errorText(error), prepared.fingerprint);
		}
	}

	interface ChangedRoot {
		root: string;
		before: RustSnapshot;
		after: RustSnapshot;
		config: GateConfig;
	}

	async function changedRoots(
		before: RustWorkspaceSnapshot,
		after: RustWorkspaceSnapshot,
		config: GateConfig,
		toolName?: string,
	): Promise<ChangedRoot[]> {
		const roots = new Set([...before.snapshots.keys(), ...after.snapshots.keys()]);
		const changed: ChangedRoot[] = [];
		for (const root of roots) {
			const afterSnapshot = after.snapshots.get(root);
			if (afterSnapshot === undefined) {
				continue;
			}
			const beforeSnapshot = before.snapshots.get(root) ?? (await captureRustIndexSnapshot(root));
			if (diffRustSnapshots(beforeSnapshot, afterSnapshot).length === 0) {
				continue;
			}
			const rootConfig = root === after.primaryRoot ? config : await loadConfig(root);
			if (!rootConfig.enabled || (toolName !== undefined && !rootConfig.tools.includes(toolName))) {
				continue;
			}
			changed.push({ root, before: beforeSnapshot, after: afterSnapshot, config: rootConfig });
		}
		return changed;
	}

	async function prepareWorkspaceFingerprint(
		before: RustWorkspaceSnapshot,
		after: RustWorkspaceSnapshot,
		config: GateConfig,
	): Promise<string | undefined> {
		const fingerprints: Array<readonly [string, string]> = [];
		for (const changed of await changedRoots(before, after, config)) {
			const prepared = await prepareReview(changed.root, changed.before, changed.after, changed.config);
			if (prepared !== undefined) {
				fingerprints.push([changed.root, prepared.fingerprint]);
			}
		}
		return fingerprints.length === 0 ? undefined : snapshotFingerprint(new Map(fingerprints));
	}

	async function inspectWorkspace(
		before: RustWorkspaceSnapshot,
		after: RustWorkspaceSnapshot,
		config: GateConfig,
		signal?: AbortSignal,
		toolName?: string,
	): Promise<CompletedReview | undefined> {
		const completed: Array<{ root: string; review: CompletedReview }> = [];
		for (const changed of await changedRoots(before, after, config, toolName)) {
			let review: CompletedReview | undefined;
			try {
				review = await inspect(changed.root, changed.before, changed.after, changed.config, signal);
			} catch (error) {
				if (error instanceof ReviewFailure) {
					const fingerprint = await prepareWorkspaceFingerprint(before, after, config);
					throw new ReviewFailure(error.message, fingerprint ?? error.fingerprint);
				}
				throw error;
			}
			if (review !== undefined) {
				completed.push({ root: changed.root, review });
			}
		}
		if (completed.length === 0) {
			return undefined;
		}

		return {
			fingerprint: snapshotFingerprint(new Map(completed.map(({ root, review }) => [root, review.fingerprint]))),
			report: {
				model: [...new Set(completed.map(({ review }) => review.report.model))].join(", "),
				filesReviewed: completed.reduce((total, { review }) => total + review.report.filesReviewed, 0),
				findings: completed.flatMap(({ root, review }) =>
					review.report.findings.map((finding) => ({
						...finding,
						file: root === after.primaryRoot ? finding.file : join(root, finding.file),
					})),
				),
				inputTokens: completed.reduce((total, { review }) => total + review.report.inputTokens, 0),
				outputTokens: completed.reduce((total, { review }) => total + review.report.outputTokens, 0),
			},
		};
	}

	function workspaceSnapshotFingerprint(snapshot: RustWorkspaceSnapshot): string {
		return snapshotFingerprint(
			new Map(
				[...snapshot.snapshots.entries()].flatMap(([root, files]) =>
					[...files.entries()].map(([path, content]) => [`${root}\0${path}`, content] as const),
				),
			),
		);
	}

	function workspaceOptions(config: GateConfig) {
		return {
			includeRegisteredWorktrees: config.worktreeScope === "registered",
			additionalRoots: config.additionalRoots,
		};
	}

	async function activeConfig(root: string): Promise<GateConfig> {
		const config = await loadConfig(root);
		state.activeConfig = config;
		return config;
	}

	function fallbackFailureFingerprint(current: RustWorkspaceSnapshot | undefined, config: GateConfig | undefined): string {
		return `error:${snapshotFingerprint(
			new Map([
				["snapshot", current === undefined ? "unavailable" : workspaceSnapshotFingerprint(current)],
				["config", config === undefined ? "unavailable" : JSON.stringify(config)],
				["baseline", state.baselineCaptureFailed ? "unavailable" : "available"],
			]),
		)}`;
	}

	function reportToUser(ctx: ExtensionContext, text: string, level: "info" | "warning" | "error" = "info"): void {
		if (ctx.hasUI) {
			ctx.ui.notify(text, level);
			return;
		}
		pi.sendMessage({ customType: MESSAGE_TYPE, content: text, display: true });
	}

	function clearBlock(): void {
		state.blocked = undefined;
		state.override = undefined;
	}

	function blockCompletion(fingerprint: string, text: string, config: GateConfig): void {
		if (state.override?.fingerprint === fingerprint) {
			state.blocked = undefined;
			return;
		}
		state.blocked = { fingerprint };
		state.followUps ??= 0;
		if (state.followUps < config.maxFollowUps) {
			state.followUps += 1;
			pi.sendUserMessage(`Automated coding-style-gate follow-up:

${text}`, { deliverAs: "followUp" });
			return;
		}
		if (!state.exhaustedNoticeSent) {
			state.exhaustedNoticeSent = true;
			pi.sendMessage(
				{
					customType: MESSAGE_TYPE,
					content: `${text}

The automatic correction limit was reached. Fix the code or use /coding-style-gate override <reason>.`,
					display: true,
				},
				{ deliverAs: "followUp" },
			);
		}
	}

	pi.on("session_start", async (_event, ctx) => {
		state.taskBaseline = undefined;
		state.baselineCaptureFailed = false;
		state.followUps = 0;
		state.exhaustedNoticeSent = false;
		try {
			const config = await loadConfig(ctx.cwd);
			state.activeConfig = config;
			state.taskBaseline = await captureRustWorkspaceSnapshot(ctx.cwd, workspaceOptions(config));
		} catch (error) {
			state.baselineCaptureFailed = true;
			state.lastError = errorText(error);
		}
	});

	pi.on("tool_call", async (event, ctx) => {
		try {
			const config = await activeConfig(ctx.cwd);
			if (!config.enabled) {
				return;
			}
			pendingSnapshots.set(event.toolCallId, {
				config,
				snapshot: await captureRustWorkspaceSnapshot(ctx.cwd, workspaceOptions(config)),
			});
		} catch (error) {
			state.lastError = errorText(error);
		}
	});

	pi.on("tool_result", async (event, ctx) => {
		const pending = pendingSnapshots.get(event.toolCallId);
		pendingSnapshots.delete(event.toolCallId);
		if (pending === undefined) {
			return;
		}

		return serial(async () => {
			try {
				const config = await activeConfig(ctx.cwd);
				const after = await captureRustWorkspaceSnapshot(ctx.cwd, workspaceOptions(config));
				const completed = await inspectWorkspace(
					pending.snapshot,
					after,
					config,
					ctx.signal,
					event.toolName,
				);
				if (completed === undefined) {
					return;
				}
				state.lastError = undefined;
				state.lastReport = completed.report;
				if (completed.report.findings.length === 0) {
					return;
				}
				return { content: [...event.content, resultMessage(formatReview(completed.report))] };
			} catch (error) {
				state.lastError = errorText(error);
				return { content: [...event.content, resultMessage(REVIEW_FAILED)] };
			}
		});
	});

	pi.on("agent_end", async (event, ctx) => {
		if (runWasAborted(event.messages)) {
			return;
		}
		await serial(async () => {
			let config: GateConfig | undefined;
			let current: RustWorkspaceSnapshot | undefined;
			try {
				config = await activeConfig(ctx.cwd);
				if (!config.enabled) {
					return;
				}
				current = await captureRustWorkspaceSnapshot(ctx.cwd, workspaceOptions(config));
				if (state.taskBaseline === undefined) {
					throw new Error("coding-style-gate: task baseline is unavailable");
				}
				const completed = await inspectWorkspace(state.taskBaseline, current, config, ctx.signal);
				if (completed === undefined) {
					clearBlock();
					return;
				}
				state.lastError = undefined;
				state.lastReport = completed.report;
				if (completed.report.findings.length === 0) {
					clearBlock();
					return;
				}
				const formatted = formatReview(completed.report);
				if (config.mode === "enforce") {
					blockCompletion(completed.fingerprint, formatted, config);
				} else {
					reportToUser(ctx, formatted, "warning");
				}
			} catch (error) {
				state.lastError = errorText(error);
				const errorFingerprint =
					error instanceof ReviewFailure
						? `error:${error.fingerprint}`
						: fallbackFailureFingerprint(current, config);
				const fallbackConfig = config ?? (await activeConfig(ctx.cwd).catch(() => undefined));
				if (fallbackConfig?.mode === "enforce") {
					blockCompletion(errorFingerprint, REVIEW_FAILED, fallbackConfig);
				} else if (fallbackConfig?.enabled) {
					reportToUser(ctx, REVIEW_FAILED, "warning");
				}
			}
		});
	});

	pi.registerCommand("coding-style-gate", {
		description: "Show status, check or override task-local Rust changes, or reset the baseline",
		getArgumentCompletions(prefix) {
			const options = ["status", "check", "reset", "override"]
				.filter((value) => value.startsWith(prefix))
				.map((value) => ({ value, label: value }));
			return options.length > 0 ? options : null;
		},
		async handler(args, ctx) {
			const [action = "status", ...remainder] = args.trim().split(/\s+/).filter(Boolean);
			if (action === "reset") {
				await ctx.waitForIdle();
				await serial(async () => {
					const config = await activeConfig(ctx.cwd);
					const current = await captureRustWorkspaceSnapshot(ctx.cwd, workspaceOptions(config));
					pendingSnapshots.clear();
					state.taskBaseline = current;
					state.baselineCaptureFailed = false;
					state.followUps = 0;
					state.exhaustedNoticeSent = false;
					state.lastReport = undefined;
					state.lastError = undefined;
					clearBlock();
				});
				reportToUser(ctx, "coding-style-gate baseline reset.");
				return;
			}

			if (action === "check") {
				await ctx.waitForIdle();
				await serial(async () => {
					const config = await activeConfig(ctx.cwd);
					const current = await captureRustWorkspaceSnapshot(ctx.cwd, workspaceOptions(config));
					if (state.taskBaseline === undefined) {
						throw new Error("coding-style-gate: task baseline is unavailable; use /coding-style-gate reset");
					}
					const completed = await inspectWorkspace(state.taskBaseline, current, config, ctx.signal);
					if (completed === undefined) {
						reportToUser(ctx, "coding-style-gate found no task-local Rust changes.");
						return;
					}
					state.lastError = undefined;
					state.lastReport = completed.report;
					reportToUser(ctx, formatReview(completed.report), completed.report.findings.length ? "warning" : "info");
				});
				return;
			}

			if (action === "override") {
				const reason = remainder.join(" ").trim();
				if (!reason) {
					reportToUser(ctx, "Usage: /coding-style-gate override <reason>", "warning");
					return;
				}
				await ctx.waitForIdle();
				await serial(async () => {
					const config = await activeConfig(ctx.cwd);
					const current = await captureRustWorkspaceSnapshot(ctx.cwd, workspaceOptions(config));
					const normalFingerprint =
						state.taskBaseline === undefined
							? undefined
							: await prepareWorkspaceFingerprint(state.taskBaseline, current, config);
					const errorFingerprint =
						normalFingerprint === undefined
							? fallbackFailureFingerprint(current, config)
							: `error:${normalFingerprint}`;
					const fingerprint =
						state.blocked?.fingerprint === errorFingerprint
							? errorFingerprint
							: normalFingerprint;
					if (fingerprint === undefined) {
						reportToUser(ctx, "coding-style-gate has no task-local Rust changes to override.", "warning");
						return;
					}
					state.override = { fingerprint, reason };
					state.blocked = undefined;
					reportToUser(ctx, `coding-style-gate override recorded for the current task state: ${reason}`, "warning");
				});
				return;
			}

			if (action !== "status") {
				reportToUser(ctx, "Usage: /coding-style-gate status | check | reset | override <reason>", "warning");
				return;
			}

			const config = await activeConfig(ctx.cwd);
			const summary = state.lastReport === undefined ? "No review has run in this session." : formatReview(state.lastReport);
			const override = state.override ? ` Override: ${state.override.reason}` : "";
			reportToUser(
				ctx,
				`coding-style-gate is ${config.enabled ? "enabled" : "disabled"} in ${config.mode} mode; TypeSafe credentials ${process.env.TYPESAFE_API_KEY ? "available" : "missing"}; model ${config.model}; threshold ${config.threshold.toFixed(2)}; ${Object.keys(config.ruleThresholds).length} calibrated rule override(s). ${summary}${state.lastError ? ` Last error: ${state.lastError}` : ""}${override}`,
				state.lastError || state.blocked ? "warning" : "info",
			);
		},
	});
}
