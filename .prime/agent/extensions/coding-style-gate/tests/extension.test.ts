import { afterAll, afterEach, beforeAll, describe, expect, test } from "bun:test";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";

import codingStyleGate from "../index";

const temporaryDirectories: string[] = [];
const originalApiKey = process.env.TYPESAFE_API_KEY;
const style = `# Coding style

## Comments and documentation

### Reason comments


#### Rule

Comments explain why instead of narrating code.

#### Violation

An added comment merely restates the adjacent operation.

#### Compliant

The comment explains a necessary constraint or surprising decision.

#### Bad example

\`\`\`rust
// Run the function.
run();
\`\`\`

#### Good example

\`\`\`rust
// The provider requires the call before publication.
run();
\`\`\`
`;
const ruleThresholds = { "comments-and-documentation-reason-comments": 0.86 };

async function writeStyleFixture(root: string): Promise<void> {
	await writeFile(join(root, "CODING_STYLE.md"), style);
	await mkdir(join(root, ".prime", "agent"), { recursive: true });
	try {
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds }), { flag: "wx" });
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
	}
}

const reviewedFiles: string[] = [];

const server = setupServer(
	http.post("https://api.typesafe.ai/v1/systemone", async ({ request }) => {
		const body = (await request.clone().json()) as {
			model: string;
			questions: Record<string, unknown>;
			state: { file?: string; patch: string };
		};
		reviewedFiles.push(body.state.file ?? "");
		const narrativeComment = body.state.patch.includes("Run the function");
		return HttpResponse.json({
			model: body.model,
			answers: Object.fromEntries(
				Object.keys(body.questions).map((key) => [
					key,
					{ type: "noul", noul: key === "comments-and-documentation-reason-comments" && narrativeComment ? 0.94 : 0.02 },
				]),
			),
			usage: { input_tokens: 80, output_tokens: 2 },
		});
	}),
);

beforeAll(() => {
	process.env.TYPESAFE_API_KEY = "test-key";
	server.listen({ onUnhandledRequest: "error" });
});
afterEach(async () => {
	server.resetHandlers();
	reviewedFiles.length = 0;
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});
afterAll(() => {
	server.close();
	if (originalApiKey === undefined) delete process.env.TYPESAFE_API_KEY;
	else process.env.TYPESAFE_API_KEY = originalApiKey;
});

type Handler = (event: any, context: any) => Promise<any> | any;

function fakePrime() {
	const entries: Array<{ customType: string; data: any }> = [];
	const userMessages: Array<{ text: string; options: unknown }> = [];
	const sentMessages: Array<{ message: unknown; options: unknown }> = [];
	const handlers = new Map<string, Handler>();
	const commands = new Map<string, any>();
	return {
		entries,
		handlers,
		commands,
		userMessages,
		sentMessages,
		api: {
			appendEntry(customType: string, data: unknown) { entries.push({ customType, data }); },
			on(name: string, handler: Handler) {
				handlers.set(name, handler);
			},
			registerCommand(name: string, command: any) {
				commands.set(name, command);
			},
			sendMessage(message: unknown, options: unknown) {
				sentMessages.push({ message, options });
			},
			sendUserMessage(text: string, options: unknown) {
				userMessages.push({ text, options });
			},
		},
	};
}

describe("Prime coding style gate", () => {

	test("allows missing-threshold overrides and invalidates them on source or config changes", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-missing-override-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		const configPath = join(root, ".prime", "agent", "coding-style-gate.json");
		const config = { mode: "enforce", ruleThresholds: {}, maxFollowUps: 4 };
		await writeFile(configPath, JSON.stringify(config));
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} }, waitForIdle: async () => {} };
		await prime.handlers.get("session_start")?.({}, context);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(1);
		await prime.commands.get("coding-style-gate").handler("override thresholds need calibration", context);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(1);
		await writeFile(join(root, "example.rs"), "// Run the function again.\nfn run() {}\n");
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(2);
		await prime.commands.get("coding-style-gate").handler("override current state accepted", context);
		await writeFile(configPath, JSON.stringify({ ...config, ruleThresholds: { unrelated: 0.4 } }));
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(3);
		expect(reviewedFiles).toHaveLength(0);
	});


	for (const mode of ["advisory", "enforce"]) {
		test(`rejects missing thresholds without API requests in ${mode} mode`, async () => {
			const root = await mkdtemp(join(tmpdir(), "coding-style-threshold-"));
			temporaryDirectories.push(root);
			await Bun.$`git init -q ${root}`;
			await writeStyleFixture(root);
			await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode, ruleThresholds: {} }));
			await writeFile(join(root, "example.rs"), "fn run() {}\n");
			const prime = fakePrime();
			codingStyleGate(prime.api as never);
			const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
			await prime.handlers.get("session_start")?.({}, context);
			await prime.handlers.get("tool_call")?.({ toolName: "ipython", toolCallId: "missing-threshold" }, context);
			await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
			const result = await prime.handlers.get("tool_result")?.({ toolName: "ipython", toolCallId: "missing-threshold", content: [] }, context);
			expect(result.content[0].text).toContain("could not review");
			await prime.handlers.get("agent_end")?.({ messages: [] }, context);
			expect(reviewedFiles).toHaveLength(0);
			expect(prime.userMessages).toHaveLength(mode === "enforce" ? 1 : 0);
			await prime.commands.get("coding-style-gate").handler("status", context);
			expect(JSON.stringify(prime.sentMessages)).toContain("missing threshold for rule comments-and-documentation-reason-comments");
		});
	}

	test("reports after-tool snapshot failures and manual check failures", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-after-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} }, waitForIdle: async () => {} };
		await prime.handlers.get("session_start")?.({}, context);
		await prime.handlers.get("tool_call")?.({ toolName: "ipython", toolCallId: "after-failure" }, context);
		await rm(join(root, ".git"), { recursive: true });
		const result = await prime.handlers.get("tool_result")?.({ toolName: "ipython", toolCallId: "after-failure", content: [] }, context);
		expect(result.content[0].text).toContain("after-tool snapshot");
		expect(prime.entries.at(-1)?.data).toMatchObject({ outcome: "failed", stage: "after_tool_snapshot" });
		await prime.commands.get("coding-style-gate").handler("check", context);
		expect(prime.entries.at(-1)?.data).toMatchObject({ trigger: "check", outcome: "failed", stage: "final_snapshot" });
		expect(JSON.stringify(prime.sentMessages)).toContain("final snapshot");
	});


	test("persists clean tool and cached final review receipts", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-receipt-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("session_start")?.({}, context);
		await prime.handlers.get("tool_call")?.({ toolName: "ipython", toolCallId: "receipt-edit" }, context);
		await writeFile(join(root, "example.rs"), "fn run() { let ready = true; }\n");
		const result = await prime.handlers.get("tool_result")?.({ toolName: "ipython", toolCallId: "receipt-edit", content: [] }, context);
		expect(result).toBeUndefined();
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(reviewedFiles).toEqual(["example.rs"]);
		const receipts = prime.entries.filter(entry => entry.customType === "coding-style-gate-receipt").map(entry => entry.data);
		expect(receipts).toHaveLength(2);
		expect(receipts[0]).toMatchObject({ version: 1, trigger: "tool_result", toolCallId: "receipt-edit", toolName: "ipython", outcome: "reviewed", files: ["example.rs"], findingCount: 0, cachedFiles: 0 });
		expect(receipts[0].fingerprint).toBeTruthy();
		expect(receipts[0].model).toBeTruthy();
		expect(receipts[1]).toMatchObject({ trigger: "agent_end", outcome: "reviewed", cachedFiles: 1 });
		expect(receipts[1].fingerprint).toEqual(receipts[0].fingerprint);
		expect(JSON.stringify(receipts)).not.toContain("let ready");
		await prime.commands.get("coding-style-gate").handler("check", { ...context, waitForIdle: async () => {} });
		expect(prime.entries.at(-1)?.data).toMatchObject({ trigger: "check", outcome: "reviewed", cachedFiles: 1 });
	});

	test("reports pre-tool snapshot failures in the result and persists failure receipt", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-failure-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("tool_call")?.({ toolName: "ipython", toolCallId: "failed-snapshot" }, { ...context, cwd: join(root, "missing") });
		const result = await prime.handlers.get("tool_result")?.({ toolName: "ipython", toolCallId: "failed-snapshot", content: [{ type: "text", text: "original" }] }, context);
		expect(result.content[0].text).toBe("original");
		expect(result.content[1].text).toContain("before-tool snapshot");
		expect(result.content[1].text).toContain("not reviewed");
		expect(prime.entries.at(-1)?.data).toMatchObject({ outcome: "failed", stage: "before_tool_snapshot", toolCallId: "failed-snapshot" });
		expect(reviewedFiles).toHaveLength(0);
	});

	test("persists advisory end findings even with an interactive UI", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-ui-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const notifications: string[] = [];
		const context = { cwd: root, signal: undefined, hasUI: true, ui: { notify(text: string) { notifications.push(text); } } };
		await prime.handlers.get("session_start")?.({}, context);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.entries.at(-1)?.data).toMatchObject({ outcome: "reviewed", trigger: "agent_end", findingCount: 1 });
		expect(JSON.stringify(prime.sentMessages)).toContain("likely violation");
		expect(notifications).toHaveLength(1);
	});

	test("reviews a worktree created and edited during one tool call", async () => {
		const container = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(container);
		const root = join(container, "root");
		const worktree = join(container, "created-worktree");
		await mkdir(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		await Bun.$`git -C ${root} -c user.name=Test -c user.email=test@example.invalid commit -qm baseline`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("tool_call")?.(
			{ toolName: "ipython", toolCallId: "call-create", input: { code: "create and edit worktree" } },
			context,
		);
		await Bun.$`git -C ${root} worktree add -q -b created-test ${worktree}`;
		await writeFile(join(worktree, "example.rs"), "// Run the function.\nfn run() {}\n");
		const result = await prime.handlers.get("tool_result")?.(
			{
				toolName: "ipython",
				toolCallId: "call-create",
				input: { code: "create and edit worktree" },
				content: [{ type: "text", text: "written" }],
				isError: false,
			},
			context,
		);

		expect(result.content[1]?.text).toContain("created-worktree");
		expect(result.content[1]?.text).toContain("likely violation");
	});

	test("does not load review configuration for an unchanged worktree", async () => {
		const container = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(container);
		const root = join(container, "root");
		const worktree = join(container, "unchanged-worktree");
		await mkdir(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		await Bun.$`git -C ${root} -c user.name=Test -c user.email=test@example.invalid commit -qm baseline`;
		await Bun.$`git -C ${root} worktree add -q -b unchanged-test ${worktree}`;
		await Bun.$`mkdir -p ${join(worktree, ".prime", "agent")}`;
		await writeFile(join(worktree, ".prime", "agent", "coding-style-gate.json"), "not JSON");
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("tool_call")?.(
			{ toolName: "ipython", toolCallId: "call-unchanged", input: { code: "write session code" } },
			context,
		);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
		const result = await prime.handlers.get("tool_result")?.(
			{
				toolName: "ipython",
				toolCallId: "call-unchanged",
				input: { code: "write session code" },
				content: [{ type: "text", text: "written" }],
				isError: false,
			},
			context,
		);

		expect(result.content[1]?.text).toContain("likely violation");
		expect(result.content[1]?.text).not.toContain("could not review");
	});

	test("reviews Rust edits in a registered worktree outside the session root", async () => {
		const container = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(container);
		const root = join(container, "root");
		const worktree = join(container, "external-worktree");
		await mkdir(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		await Bun.$`git -C ${root} -c user.name=Test -c user.email=test@example.invalid commit -qm baseline`;
		await Bun.$`git -C ${root} worktree add -q -b external-test ${worktree}`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("tool_call")?.(
			{ toolName: "ipython", toolCallId: "call-external", input: { code: "write external code" } },
			context,
		);
		await writeFile(join(worktree, "example.rs"), "// Run the function.\nfn run() {}\n");
		const result = await prime.handlers.get("tool_result")?.(
			{
				toolName: "ipython",
				toolCallId: "call-external",
				input: { code: "write external code" },
				content: [{ type: "text", text: "written" }],
				isError: false,
			},
			context,
		);

		expect(result.content).toHaveLength(2);
		expect(result.content[1]?.text).toContain("external-worktree");
		expect(result.content[1]?.text).toContain("coding-style-gate found 1 likely violation");
		expect(reviewedFiles).toEqual(["example.rs"]);
		expect(reviewedFiles.join("\n")).not.toContain(container);
	});

	test("honors a watched worktree's tool allowlist", async () => {
		const container = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(container);
		const root = join(container, "root");
		const worktree = join(container, "edit-only-worktree");
		await mkdir(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		await Bun.$`git -C ${root} -c user.name=Test -c user.email=test@example.invalid commit -qm baseline`;
		await Bun.$`git -C ${root} worktree add -q -b edit-only-test ${worktree}`;
		await Bun.$`mkdir -p ${join(worktree, ".prime", "agent")}`;
		await writeFile(
			join(worktree, ".prime", "agent", "coding-style-gate.json"),
			JSON.stringify({ ruleThresholds, tools: ["edit"] }),
		);
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("tool_call")?.(
			{ toolName: "ipython", toolCallId: "call-disallowed", input: { code: "write disallowed code" } },
			context,
		);
		await writeFile(join(worktree, "example.rs"), "// Run the function.\nfn run() {}\n");
		const result = await prime.handlers.get("tool_result")?.(
			{
				toolName: "ipython",
				toolCallId: "call-disallowed",
				input: { code: "write disallowed code" },
				content: [{ type: "text", text: "written" }],
				isError: false,
			},
			context,
		);

		expect(result).toBeUndefined();
	});

	test("uses the watched worktree's own style configuration", async () => {
		const container = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(container);
		const root = join(container, "root");
		const worktree = join(container, "configured-worktree");
		await mkdir(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		await Bun.$`git -C ${root} -c user.name=Test -c user.email=test@example.invalid commit -qm baseline`;
		await Bun.$`git -C ${root} worktree add -q -b configured-test ${worktree}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(
			join(root, ".prime", "agent", "coding-style-gate.json"),
			JSON.stringify({ ruleThresholds, tools: ["edit"] }),
		);
		await rm(join(worktree, "CODING_STYLE.md"));
		await Bun.$`mkdir -p ${join(worktree, ".prime", "agent")}`;
		await writeFile(join(worktree, "WORKTREE_STYLE.md"), style);
		await writeFile(
			join(worktree, ".prime", "agent", "coding-style-gate.json"),
			JSON.stringify({ ruleThresholds, styleFile: "WORKTREE_STYLE.md" }),
		);
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("tool_call")?.(
			{ toolName: "ipython", toolCallId: "call-configured", input: { code: "write configured code" } },
			context,
		);
		await writeFile(join(worktree, "example.rs"), "// Run the function.\nfn run() {}\n");
		const result = await prime.handlers.get("tool_result")?.(
			{
				toolName: "ipython",
				toolCallId: "call-configured",
				input: { code: "write configured code" },
				content: [{ type: "text", text: "written" }],
				isError: false,
			},
			context,
		);

		expect(result.content[1]?.text).toContain("configured-worktree");
		expect(result.content[1]?.text).toContain("likely violation");
		expect(result.content[1]?.text).not.toContain("could not review");
	});

	test("reviews Rust edits in an explicitly configured unrelated repository", async () => {
		const container = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(container);
		const root = join(container, "root");
		const additionalRoot = join(container, "additional-root");
		await mkdir(root);
		await mkdir(additionalRoot);
		await Bun.$`git init -q ${root}`;
		await Bun.$`git init -q ${additionalRoot}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(
			join(root, ".prime", "agent", "coding-style-gate.json"),
			JSON.stringify({ ruleThresholds, additionalRoots: [additionalRoot] }),
		);
		await writeStyleFixture(root);
		await writeFile(join(root, "session.rs"), "fn session() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md session.rs`;
		await writeStyleFixture(additionalRoot);
		await writeFile(join(additionalRoot, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${additionalRoot} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("tool_call")?.(
			{ toolName: "ipython", toolCallId: "call-additional", input: { code: "write additional code" } },
			context,
		);
		await writeFile(join(additionalRoot, "example.rs"), "// Run the function.\nfn run() {}\n");
		const result = await prime.handlers.get("tool_result")?.(
			{
				toolName: "ipython",
				toolCallId: "call-additional",
				input: { code: "write additional code" },
				content: [{ type: "text", text: "written" }],
				isError: false,
			},
			context,
		);

		expect(result.content).toHaveLength(2);
		expect(result.content[1]?.text).toContain("additional-root");
	});

	test("attaches Jev findings to the ipython result that wrote the code", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("tool_call")?.(
			{ toolName: "ipython", toolCallId: "call-1", input: { code: "write code" } },
			context,
		);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
		const result = await prime.handlers.get("tool_result")?.(
			{
				toolName: "ipython",
				toolCallId: "call-1",
				input: { code: "write code" },
				content: [{ type: "text", text: "written" }],
				isError: false,
			},
			context,
		);

		expect(result.content).toHaveLength(2);
		expect(result.content[1]?.text).toContain("coding-style-gate found 1 likely violation");
		expect(result.content[1]?.text).toContain("Comments and documentation");
		expect(result.content[1]?.text).toContain("0.94");
	});

	test("queues one follow-up when enforce mode sees an unreviewed final change", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds, mode: "enforce", model: "jev-test", maxFollowUps: 1 }));
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };

		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		expect(prime.userMessages).toHaveLength(1);
		expect(prime.userMessages[0]?.options).toEqual({ deliverAs: "followUp" });
		expect(prime.userMessages[0]?.text).toContain("Automated coding-style-gate follow-up");
		expect(prime.sentMessages).toHaveLength(1);
	});


	test("keeps an earlier violation blocking after a later clean edit", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds, mode: "enforce", model: "jev-test" }));
		await writeStyleFixture(root);
		await writeFile(join(root, "a.rs"), "fn a() {}\n");
		await writeFile(join(root, "b.rs"), "fn b() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md a.rs b.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);

		await prime.handlers.get("tool_call")?.({ toolName: "ipython", toolCallId: "call-a", input: {} }, context);
		await writeFile(join(root, "a.rs"), "// Run the function.\nfn a() {}\n");
		await prime.handlers.get("tool_result")?.(
			{ toolName: "ipython", toolCallId: "call-a", content: [{ type: "text", text: "a" }], isError: false },
			context,
		);

		await prime.handlers.get("tool_call")?.({ toolName: "ipython", toolCallId: "call-b", input: {} }, context);
		await writeFile(join(root, "b.rs"), "fn b() -> bool { true }\n");
		const cleanResult = await prime.handlers.get("tool_result")?.(
			{ toolName: "ipython", toolCallId: "call-b", content: [{ type: "text", text: "b" }], isError: false },
			context,
		);
		expect(cleanResult).toBeUndefined();

		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		expect(prime.userMessages).toHaveLength(1);
		expect(prime.userMessages[0]?.text).toContain("a.rs");
	});


	test("requires an explicit override and invalidates it when the task state changes", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds, mode: "enforce", model: "jev-test", maxFollowUps: 2 }));
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} }, waitForIdle: async () => {} };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		await prime.commands.get("coding-style-gate")?.handler("override accepted exception", context);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(1);

		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() { let _changed = true; }\n");
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(2);
	});


	test("bounds follow-ups when final snapshot capture keeps failing", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds, mode: "enforce", model: "jev-test", maxFollowUps: 1 }));
		await writeStyleFixture(root);
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);
		await rm(join(root, ".git"), { recursive: true });

		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		expect(prime.userMessages).toHaveLength(1);
		expect(prime.sentMessages).toHaveLength(1);
	});


	test("allows an explicit override after a review-service failure", async () => {
		server.use(http.post("https://api.typesafe.ai/v1/systemone", () => new HttpResponse(null, { status: 503 })));
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds, mode: "enforce", model: "jev-test", maxFollowUps: 2 }));
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} }, waitForIdle: async () => {} };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(1);

		await prime.commands.get("coding-style-gate")?.handler("override TypeSafe is unavailable", context);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		expect(prime.userMessages).toHaveLength(1);

		await writeFile(
			join(root, ".prime", "agent", "coding-style-gate.json"),
			JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 2, ruleThresholds: { "comments-and-documentation-reason-comments": 0.81 } }),
		);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(2);
	});


	test("reviews the full task diff at agent end in advisory mode", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);
		await writeFile(join(root, "example.rs"), "// Run the function.\nfn run() {}\n");

		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		expect(prime.userMessages).toHaveLength(0);
		expect(prime.sentMessages).toHaveLength(1);
		expect(JSON.stringify(prime.sentMessages[0]?.message)).toContain("likely violation");
	});

	test("bounds automatic follow-ups across different failing fingerprints", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds, mode: "enforce", model: "jev-test", maxFollowUps: 2 }));
		await writeStyleFixture(root);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);

		for (let attempt = 0; attempt < 4; attempt += 1) {
			await writeFile(join(root, "example.rs"), `// Run the function.\nfn run() { let _attempt = ${attempt}; }\n`);
			await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		}

		expect(prime.userMessages).toHaveLength(2);
		expect(prime.sentMessages).toHaveLength(1);
		expect(JSON.stringify(prime.sentMessages[0]?.message)).toContain("correction limit");
	});

	test("fails closed in enforce mode when the task baseline was not captured", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`mkdir -p ${join(root, ".prime", "agent")}`;
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ ruleThresholds, mode: "enforce", model: "jev-test", maxFollowUps: 1 }));
		await writeStyleFixture(root);
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);
		await Bun.$`git init -q ${root}`;
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add .prime/agent/coding-style-gate.json CODING_STYLE.md example.rs`;

		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		expect(prime.userMessages).toHaveLength(1);
		expect(prime.userMessages[0]?.text).toContain("baseline snapshot");
	});

});
