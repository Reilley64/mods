import { afterEach, beforeEach, expect, test } from "bun:test";

import codexDiscoveryVersion from "../index";

const originalFetch = globalThis.fetch;
let requests: Array<{ input: RequestInfo | URL; init?: RequestInit }>;

beforeEach(() => {
	requests = [];
	globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
		requests.push({ input, init });
		return Promise.resolve(new Response("{}", { status: 200 }));
	}) as typeof fetch;
});

afterEach(() => {
	globalThis.fetch = originalFetch;
});

test("bumps only Codex model discovery without changing auth headers", async () => {
	codexDiscoveryVersion();
	const headers = { Authorization: "Bearer test-only" };
	await fetch("https://chatgpt.com/backend-api/codex/models?client_version=0.153.4", { headers });

	expect(requests).toHaveLength(1);
	expect(new URL(String(requests[0]?.input)).searchParams.get("client_version")).toBe("0.156.1");
	expect(requests[0]?.init?.headers).toBe(headers);
});

test("keeps unrelated requests and supported versions unchanged", async () => {
	codexDiscoveryVersion();
	const paths = [
		"https://chatgpt.com/backend-api/codex/responses?client_version=0.153.4",
		"https://chatgpt.com/backend-api/codex/models?client_version=0.156.1",
		"https://elsewhere.example/backend-api/codex/models?client_version=0.153.4",
	];
	for (const path of paths) {
		await fetch(path);
	}
	expect(requests.map(({ input }) => input)).toEqual(paths);
});

test("does not wrap fetch again when a child loads the extension", () => {
	codexDiscoveryVersion();
	const once = globalThis.fetch;
	codexDiscoveryVersion();
	expect(globalThis.fetch).toBe(once);
});
