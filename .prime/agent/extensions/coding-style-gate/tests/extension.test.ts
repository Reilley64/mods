import { afterAll, afterEach, beforeAll, describe, expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
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
const server = setupServer(
	http.post("https://api.typesafe.ai/v1/systemone", async ({ request }) => {
		const body = (await request.clone().json()) as {
			model: string;
			questions: Record<string, unknown>;
			state: { patch: string };
		};
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
	await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});
afterAll(() => {
	server.close();
	if (originalApiKey === undefined) delete process.env.TYPESAFE_API_KEY;
	else process.env.TYPESAFE_API_KEY = originalApiKey;
});

type Handler = (event: any, context: any) => Promise<any> | any;

function fakePrime() {
	const userMessages: Array<{ text: string; options: unknown }> = [];
	const sentMessages: Array<{ message: unknown; options: unknown }> = [];
	const handlers = new Map<string, Handler>();
	const commands = new Map<string, any>();
	return {
		handlers,
		commands,
		userMessages,
		sentMessages,
		api: {
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
	test("attaches Jev findings to the ipython result that wrote the code", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeFile(join(root, "CODING_STYLE.md"), style);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md example.rs`;
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
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 1 }));
		await writeFile(join(root, "CODING_STYLE.md"), style);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md example.rs`;
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
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode: "enforce", model: "jev-test" }));
		await writeFile(join(root, "CODING_STYLE.md"), style);
		await writeFile(join(root, "a.rs"), "fn a() {}\n");
		await writeFile(join(root, "b.rs"), "fn b() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md a.rs b.rs`;
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
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 2 }));
		await writeFile(join(root, "CODING_STYLE.md"), style);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md example.rs`;
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
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 1 }));
		await writeFile(join(root, "CODING_STYLE.md"), style);
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
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 2 }));
		await writeFile(join(root, "CODING_STYLE.md"), style);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md example.rs`;
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
			JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 2, threshold: 0.81 }),
		);
		await prime.handlers.get("agent_end")?.({ messages: [] }, context);
		expect(prime.userMessages).toHaveLength(2);
	});


	test("reviews the full task diff at agent end in advisory mode", async () => {
		const root = await mkdtemp(join(tmpdir(), "coding-style-gate-"));
		temporaryDirectories.push(root);
		await Bun.$`git init -q ${root}`;
		await writeFile(join(root, "CODING_STYLE.md"), style);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md example.rs`;
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
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 2 }));
		await writeFile(join(root, "CODING_STYLE.md"), style);
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md example.rs`;
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
		await writeFile(join(root, ".prime", "agent", "coding-style-gate.json"), JSON.stringify({ mode: "enforce", model: "jev-test", maxFollowUps: 1 }));
		await writeFile(join(root, "CODING_STYLE.md"), style);
		const prime = fakePrime();
		codingStyleGate(prime.api as never);
		const context = { cwd: root, signal: undefined, hasUI: false, ui: { notify() {} } };
		await prime.handlers.get("session_start")?.({ reason: "startup" }, context);
		await Bun.$`git init -q ${root}`;
		await writeFile(join(root, "example.rs"), "fn run() {}\n");
		await Bun.$`git -C ${root} add CODING_STYLE.md example.rs`;

		await prime.handlers.get("agent_end")?.({ messages: [] }, context);

		expect(prime.userMessages).toHaveLength(1);
		expect(prime.userMessages[0]?.text).toContain("could not review");
	});

});
