import { createHash, randomUUID } from "node:crypto";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { mkdir, readFile, realpath, rename, unlink, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

export const REQUEST_VERSION = 1;
export const CACHE_VERSION = 1;
const exec = promisify(execFile);
const inFlight = new Map<string, Promise<unknown>>();

export async function cacheDirectory(root: string): Promise<string | undefined> {
	try {
		const { stdout } = await exec("git", ["rev-parse", "--git-common-dir"], { cwd: root });
		return join(await realpath(resolve(root, stdout.trim())), "coding-style-gate-cache", "v1");
	} catch { return undefined; }
}

export function requestKey(request: unknown, backend: string): string {
	return createHash("sha256").update(JSON.stringify({ cacheVersion: CACHE_VERSION, requestVersion: REQUEST_VERSION, backend, request })).digest("hex");
}

export async function cachedInference<T>(
	directory: string | undefined,
	key: string,
	validate: (value: unknown) => T,
	infer: () => Promise<T>,
): Promise<{ value: T; cached: boolean }> {
	if (!directory) return { value: await infer(), cached: false };
	const path = join(directory, `${key}.json`);
	try {
		const record = JSON.parse(await readFile(path, "utf8"));
		if (record.version === CACHE_VERSION && record.key === key) {
			return { value: validate(record.value), cached: true };
		}
	} catch { /* Corruption and cache IO failures are misses, never clean reviews. */ }
	const pending = inFlight.get(path);
	if (pending) return { value: validate(await pending), cached: true };
	const run = (async () => {
		const value = await infer();
		const temporary = `${path}.${randomUUID()}.tmp`;
		try {
			await mkdir(directory, { recursive: true });
			await writeFile(temporary, JSON.stringify({ version: CACHE_VERSION, key, value }), { mode: 0o600, flag: "wx" });
			await rename(temporary, path);
		} catch { /* Successful inference must survive unavailable local storage. */ }
		finally { await unlink(temporary).catch(() => undefined); }
		return value;
	})();
	inFlight.set(path, run);
	try { return { value: await run, cached: false }; }
	finally { inFlight.delete(path); }
}
