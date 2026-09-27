const patchedFetch = Symbol.for("mods.codex-discovery-version.patched-fetch");

export default function codexDiscoveryVersion(): void {
	if (Object.hasOwn(globalThis.fetch, patchedFetch)) {
		return;
	}

	const originalFetch = globalThis.fetch;
	const wrappedFetch: typeof fetch = (input, init) => {
		if (typeof input === "string") {
			let url: URL;
			try {
				url = new URL(input);
			} catch {
				return originalFetch(input, init);
			}

			if (url.protocol === "https:"
				&& url.hostname === "chatgpt.com"
				&& url.pathname === "/backend-api/codex/models"
				&& url.searchParams.get("client_version") === "0.153.4") {
				url.searchParams.set("client_version", "0.156.1");
				return originalFetch(url.toString(), init);
			}
		}
		return originalFetch(input, init);
	};

	Object.defineProperty(wrappedFetch, patchedFetch, { value: true });
	globalThis.fetch = wrappedFetch;
}
