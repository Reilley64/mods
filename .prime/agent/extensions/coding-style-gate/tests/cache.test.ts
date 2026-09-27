import { afterAll, afterEach, beforeAll, expect, test } from "bun:test";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HttpResponse, http } from "msw";
import { setupServer } from "msw/node";
import { TypeSafeClient } from "@typesafe-ai/sdk";
import { createReviewClient, backendIdentity } from "../provider";
import { cacheDirectory, requestKey } from "../cache";
import { reviewChanges } from "../reviewer";
import type { StyleRule } from "../rules";

const directories: string[] = [];
let calls = 0;
let lastRequest: unknown;
let bad: "model" | "answer" | "error" | "auth" | undefined;
const original = process.env.OPENROUTER_API_KEY;
const server = setupServer(http.post("https://openrouter.ai/api/v1/systemone", async ({ request }) => {
	calls++;
	expect(request.headers.get("authorization")).toBe("Bearer openrouter-test");
	const body = await request.json() as any;
	lastRequest = body;
	expect(body.model).toBe("typesafe/jev-1.13");
	if (bad === "auth") return HttpResponse.json({}, { status: 401 });
	if (bad === "error") return HttpResponse.json({}, { status: 503 });
	return HttpResponse.json({ model: bad === "model" ? "typesafe/jev-other" : "typesafe/jev-1.13-20260917",
		answers: Object.fromEntries(Object.keys(body.questions).map(id => [id, { type: "noul", noul: bad === "answer" ? 2 : 0.8 }])),
		usage: { input_tokens: 10, output_tokens: 2 } });
}));
beforeAll(() => { process.env.OPENROUTER_API_KEY = "openrouter-test"; server.listen({ onUnhandledRequest: "error" }); });
afterEach(async () => { calls = 0; bad = undefined; await Promise.all(directories.splice(0).map(path => rm(path, { recursive: true, force: true }))); });
afterAll(() => { server.close(); if (original === undefined) delete process.env.OPENROUTER_API_KEY; else process.env.OPENROUTER_API_KEY = original; });
const rule: StyleRule = { id: "r", section: "s", title: "t", text: "rule", violation: "bad", compliant: "good", badExamples: ["bad example"], goodExamples: ["good example"] };
const change = { path: "example.rs", before: "fn run() {}", after: "fn run() { work(); }", patch: "+work();" };
const config = { model: "typesafe/jev-1.13", timeoutMs: 1000 };
async function fixture() {
	const directory = await mkdtemp(join(tmpdir(), "style-cache-")); directories.push(directory);
	return { directory, options: { ...config, cacheDirectory: directory, ruleThresholds: { r: 0.7 } }, client: createReviewClient(config) };
}

test("OpenRouter auth/model and independent per-file persistence, thresholds and token accounting", async () => {
	const { options, client, directory } = await fixture();
	expect((await reviewChanges(client, [change], [rule], options)).inputTokens).toBe(10);
	expect(await readdir(directory)).toEqual([`${requestKey(lastRequest, backendIdentity())}.json`]);
	const freshClient = createReviewClient(config);
	const mixed = await reviewChanges(freshClient, [change, { ...change, path: "second.rs" }], [rule], options);
	expect(mixed).toMatchObject({ cachedFiles: 1, inputTokens: 10, outputTokens: 2, filesReviewed: 2 });
	const hit = await reviewChanges(freshClient, [change], [rule], { ...options, ruleThresholds: { r: 0.9 } });
	expect(hit).toMatchObject({ cachedFiles: 1, inputTokens: 0, outputTokens: 0, findings: [] });
	expect(calls).toBe(2);
	for (const file of await readdir(directory)) {
		const saved = await readFile(join(directory, file), "utf8");
		expect(saved).not.toContain("work()"); expect(saved).not.toContain("openrouter-test");
	}
});

test("complete request changes invalidate independently including distinct diffs with same final source", async () => {
	const { options, client } = await fixture();
	await reviewChanges(client, [change], [rule], options);
	for (const changed of [
		{ ...change, path: "other.rs" }, { ...change, patch: "-old\n+work();" },
		{ ...change, before: undefined }, { ...change, after: undefined },
		{ ...change, after: "pub fn example() {}" },
	]) await reviewChanges(client, [changed], [rule], options);
	await reviewChanges(client, [change], [rule], { ...options, moduleReferences: new Map([[change.path, ["caller.rs"]]]) });
	for (const field of ["text", "violation", "compliant", "badExamples", "goodExamples"] as const) {
		await reviewChanges(client, [change], [{ ...rule, [field]: field.endsWith("Examples") ? ["different"] : "different" }], options);
	}
	expect(calls).toBe(12);
	expect(requestKey({ model: "a" }, "legacy-direct-backend")).not.toBe(requestKey({ model: "a" }, backendIdentity()));
	expect(requestKey({ model: "a" }, "v1")).not.toBe(requestKey({ model: "b" }, "v1"));
	expect(requestKey({ model: "a" }, "v1")).not.toBe(requestKey({ model: "a" }, "v2"));
});

test("corrupt or invalid persisted records miss and IO failure still returns inference", async () => {
	const { options, client, directory } = await fixture();
	await reviewChanges(client, [change], [rule], options);
	const path = join(directory, (await readdir(directory))[0]!);
	const valid = JSON.parse(await readFile(path, "utf8"));
	for (const invalid of ["{", JSON.stringify({ ...valid, value: { model: config.model, probabilities: [null] } }), JSON.stringify({ ...valid, version: 999 })]) {
		await writeFile(path, invalid);
		expect((await reviewChanges(client, [change], [rule], options)).cachedFiles).toBe(0);
	}
	expect((await reviewChanges(client, [change], [rule], { ...options, cacheDirectory: path })).findings).toHaveLength(1);
	expect(calls).toBe(5);
});

test("failed, invalid and model-mismatched responses never poison cache or in-flight retries", async () => {
	const { options, directory } = await fixture();
	const client = new TypeSafeClient({ apiKey: "openrouter-test", baseURL: "https://openrouter.ai/api", retry: { maxRetries: 0 } });
	for (const failure of ["model", "answer", "error", "auth"] as const) {
		bad = failure;
		await expect(reviewChanges(client, [change], [rule], options)).rejects.toThrow();
		expect(await readdir(directory)).toEqual([]);
	}
	bad = undefined;
	const reports = await Promise.all([reviewChanges(client, [change], [rule], options), reviewChanges(client, [change], [rule], options)]);
	expect(calls).toBe(5);
	expect(reports.map(report => report.cachedFiles).sort()).toEqual([0, 1]);
	expect(reports.reduce((sum, report) => sum + report.inputTokens, 0)).toBe(10);
});

test("git common-dir cache is shared by worktrees", async () => {
	const { directory } = await fixture();
	await Bun.$`git init -q ${directory}`;
	await Bun.$`git -C ${directory} -c user.name=Test -c user.email=test@example.com commit --allow-empty -qm initial`;
	const worktree = join(directory, "linked");
	await Bun.$`git -C ${directory} worktree add -qb linked ${worktree}`;
	expect(await cacheDirectory(worktree)).toBe(await cacheDirectory(directory));
});


test("question order and complete question sets invalidate cache", async () => {
	const { options, client } = await fixture();
	const other = { ...rule, id: "second" };
	const configured = { ...options, ruleThresholds: { r: 0.7, second: 0.7 } };
	await reviewChanges(client, [change], [rule, other], configured);
	await reviewChanges(client, [change], [other, rule], configured);
	await reviewChanges(client, [change], [rule], configured);
	expect(calls).toBe(3);
});

test("missing OpenRouter key never falls back to legacy credentials or URL", () => {
	const saved = { openrouter: process.env.OPENROUTER_API_KEY, key: process.env.TYPESAFE_API_KEY, url: process.env.TYPESAFE_BASE_URL };
	try {
		delete process.env.OPENROUTER_API_KEY;
		process.env.TYPESAFE_API_KEY = "legacy-key";
		process.env.TYPESAFE_BASE_URL = "https://legacy.invalid";
		expect(() => createReviewClient(config)).toThrow("missing OPENROUTER_API_KEY");
	} finally {
		for (const [name, value] of [["OPENROUTER_API_KEY", saved.openrouter], ["TYPESAFE_API_KEY", saved.key], ["TYPESAFE_BASE_URL", saved.url]]) {
			if (value === undefined) delete process.env[name!]; else process.env[name!] = value;
		}
	}
});
