import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

export const release = {
  original: "mods-v0.1.0-x86_64-pc-windows-msvc.zip",
  originalSha256: "a1c01c39412f34cea07c6a26c944cc3303e2f4451872049256a99a9b48231610",
  revision: "1635e408c913181c15b518bc95339ebdd6926169",
  runtime: "mods-v0.1.0-runtime-r2-x86_64-pc-windows-msvc.zip",
  source: "mods-v0.1.0-source.tar.gz",
  sourceSha256: "ba8bf1b99786a4addbed6c99235bbc87ee04a0f07edb7a8d204c1344efc56cca",
  sourceSize: 99068488,
  sums: "mods-v0.1.0-runtime-r2-SHA256SUMS",
  sourceUrl: "https://github.com/Reilley64/mods/releases/download/v0.1.0/mods-v0.1.0-source.tar.gz",
} as const;

const notices = ["asmjit.txt", "boost.txt", "cppformat.txt", "googletest.txt", "qt.txt", "spdlog.txt", "udis86.txt"];
export const runtimeFiles = [
  "BUILD-AND-SOURCE.md", "COPYRIGHT.md", "LICENSE", "mods.exe",
  ...["usvfs_proxy_x64.exe", "usvfs_proxy_x86.exe", "usvfs_x64.dll", "usvfs_x86.dll"].map(name => `usvfs/${name}`),
  ...[...notices, "LICENSE"].map(name => `licenses/usvfs/${name}`),
  ...notices.map(name => `licenses/native-release/${name}`),
].sort();
export const directories = ["licenses/", "usvfs/", "licenses/native-release/", "licenses/usvfs/"];
const instructionPath = new URL("../docs/runtime-v0.1.0-BUILD-AND-SOURCE.md", import.meta.url);
const sha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");
const identity = (bytes: Uint8Array) => ({ size: bytes.byteLength, sha256: sha256(bytes) });

export function checkLayout(listing: string, original: boolean) {
  const members = [...directories, ...runtimeFiles];
  const expected = new Set(original ? ["./", ...members.map(name => `./${name}`), "./mods-source.tar.gz"] : members);
  const entries = listing.endsWith("\n") ? listing.slice(0, -1).split("\n") : listing.split("\n");
  if (entries.length !== expected.size || new Set(entries).size !== entries.length || entries.some(name => !expected.has(name))) {
    throw new Error("Unexpected archive layout (exact paths, directories and unique names required)");
  }
}

export function checksumText(runtimeHash: string) {
  if (!/^[a-f0-9]{64}$/.test(runtimeHash)) throw new Error("Invalid runtime hash");
  return `${runtimeHash}  ${release.runtime}\n${release.sourceSha256}  ${release.source}\n`;
}

export function verifyChecksums(runtimeHash: string, sourceHash: string, sums: string) {
  if (sourceHash !== release.sourceSha256 || sums !== checksumText(runtimeHash)) {
    throw new Error("Runtime/source checksum identity mismatch");
  }
}

export function compareFiles(original: Map<string, Uint8Array>, runtime: Map<string, Uint8Array>, instructions: Uint8Array) {
  if (runtime.size !== runtimeFiles.length || runtimeFiles.some(name => !runtime.has(name) || !original.has(name))) {
    throw new Error("Runtime file inventory mismatch");
  }
  return runtimeFiles.map(name => {
    const input = identity(original.get(name)!);
    const output = identity(runtime.get(name)!);
    const replacement = name === "BUILD-AND-SOURCE.md";
    const expected = replacement ? identity(instructions) : input;
    if (output.sha256 !== expected.sha256 || output.size !== expected.size) throw new Error(`Runtime file mismatch: ${name}`);
    return { name, action: replacement ? "instructions-replaced" : "unchanged", input, output };
  });
}

async function tar(...args: string[]) {
  const process = Bun.spawn(["tar", ...args], { stdout: "pipe", stderr: "pipe" });
  const [stdout, stderr, code] = await Promise.all([
    new Response(process.stdout).arrayBuffer(), new Response(process.stderr).text(), process.exited,
  ]);
  if (code !== 0) throw new Error(`tar failed (${code}): ${stderr}`);
  return new Uint8Array(stdout);
}

async function pinnedOriginal(path: string) {
  const bytes = await readFile(path);
  if (sha256(bytes) !== release.originalSha256) throw new Error("Original ZIP checksum mismatch");
  return identity(bytes);
}

async function archiveFiles(path: string, original: boolean) {
  const header = await readFile(path);
  if (header[0] !== 0x50 || header[1] !== 0x4b || header[2] !== 3 || header[3] !== 4) throw new Error("Expected ZIP format, not a renamed tar");
  const listing = new TextDecoder().decode(await tar("-tf", path));
  checkLayout(listing, original);
  const names = listing.trimEnd().split("\n");
  const details = new TextDecoder().decode(await tar("-tvf", path)).trimEnd().split("\n");
  if (details.length !== names.length || details.some((line, index) => line[0] !== (names[index].endsWith("/") ? "d" : "-"))) {
    throw new Error("Only ordinary files and directories are allowed in the ZIP");
  }
  const files = new Map<string, Uint8Array>();
  for (const name of runtimeFiles) files.set(name, await tar("-xOf", path, original ? `./${name}` : name));
  return files;
}

async function sourceIdentity(path: string) {
  const bytes = await readFile(path);
  if (bytes.byteLength !== release.sourceSize || sha256(bytes) !== release.sourceSha256) throw new Error("Complete source identity mismatch");
  const marker = new TextDecoder().decode(await tar("-xOf", path, "./SOURCE-REVISION.txt"));
  if (marker !== `${release.revision}\r\n`) throw new Error("Original source revision mismatch");
  return identity(bytes);
}

// Recompute from the pinned original, not from the preparer's receipt or staging tree.
export async function verify(inputPath: string, outputDirectory: string) {
  const input = resolve(inputPath);
  const output = resolve(outputDirectory);
  const originalIdentity = await pinnedOriginal(input);
  const original = await archiveFiles(input, true);
  const embedded = await tar("-xOf", input, "./mods-source.tar.gz");
  if (sha256(embedded) !== release.sourceSha256 || embedded.byteLength !== release.sourceSize) throw new Error("Embedded source identity mismatch");
  const runtimePath = join(output, release.runtime);
  const runtime = await archiveFiles(runtimePath, false);
  const files = compareFiles(original, runtime, await readFile(instructionPath));
  const source = await sourceIdentity(join(output, release.source));
  const runtimeIdentity = identity(await readFile(runtimePath));
  const sums = await readFile(join(output, release.sums), "utf8");
  verifyChecksums(runtimeIdentity.sha256, source.sha256, sums);
  await pinnedOriginal(input);
  return {
    status: "local-byte-verification-only", revision: release.revision,
    original: { name: release.original, ...originalIdentity },
    runtime: { name: release.runtime, ...runtimeIdentity },
    source: { name: release.source, url: release.sourceUrl, originalMember: "./mods-source.tar.gz", ...source },
    checksum: { name: release.sums, ...identity(new TextEncoder().encode(sums)) },
    files,
  };
}

export async function prepare(inputPath: string, outputDirectory: string) {
  const input = resolve(inputPath);
  const output = resolve(outputDirectory);
  await pinnedOriginal(input);
  const original = await archiveFiles(input, true);
  const source = await tar("-xOf", input, "./mods-source.tar.gz");
  if (source.byteLength !== release.sourceSize || sha256(source) !== release.sourceSha256) throw new Error("Embedded source identity mismatch");
  const instructions = await readFile(instructionPath);
  await pinnedOriginal(input);

  // Nonrecursive mkdir is the exclusive claim. Existing paths are never reused.
  // Leave all output and work files in place if any later step fails.
  await mkdir(output);
  const work = join(output, "work");
  await mkdir(work);
  for (const directory of ["licenses", "usvfs", "licenses/native-release", "licenses/usvfs"]) await mkdir(join(work, directory));
  for (const name of runtimeFiles) {
    await writeFile(join(work, name), name === "BUILD-AND-SOURCE.md" ? instructions : original.get(name)!, { flag: "wx" });
  }
  await writeFile(join(output, release.source), source, { flag: "wx" });
  // Windows Shell exposes no members when this ZIP contains a "./" root entry.
  await tar("-a", "-cf", join(output, release.runtime), "-C", work, ...new Set(runtimeFiles.map(name => name.split("/")[0]!)));
  await writeFile(join(output, release.sums), checksumText(sha256(await readFile(join(output, release.runtime)))), { flag: "wx" });
  const receipt = await verify(input, output);
  await writeFile(join(output, "repack-receipt.json"), JSON.stringify({
    ...receipt,
    tooling: { bun: Bun.version, tar: new TextDecoder().decode(await tar("--version")).trim(), platform: process.platform },
  }, null, 2) + "\n", { flag: "wx" });
  return receipt;
}

if (import.meta.main) {
  const [mode, input, output, ...extra] = Bun.argv.slice(2);
  if ((mode !== "prepare" && mode !== "verify") || !input || !output || extra.length) {
    throw new Error("Usage: bun scripts/repack-v010-runtime.ts <prepare|verify> <original.zip> <output-directory>");
  }
  console.log(JSON.stringify(await (mode === "prepare" ? prepare(input, output) : verify(input, output)), null, 2));
}
