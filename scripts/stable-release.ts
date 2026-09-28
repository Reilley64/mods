import { createHash } from "node:crypto";

export function stableRelease(tag: string) {
  if (!/^v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)$/.test(tag) || tag === "v0.0.0") {
    throw new Error("Expected a nonzero aggregate stable tag");
  }
  const archive = `mods-${tag}-runtime-x86_64-pc-windows-msvc.zip`;
  const source = `mods-${tag}-source.tar.gz`;
  const base = `https://github.com/Reilley64/mods/releases/download/${tag}/`;
  return { tag, version: tag.slice(1), archive, source, url: base + archive, sourceUrl: base + source, checksumUrl: base + "SHA256SUMS" };
}

export function parseChecksums(tag: string, sums: string) {
  const release = stableRelease(tag);
  const lines = sums.replace(/\r\n/g, "\n").replace(/\n$/, "").split("\n");
  const names = [release.archive, release.source];
  if (lines.length !== 2) throw new Error("Expected exactly two checksum entries");
  return Object.fromEntries(names.map((name, index) => {
    const match = /^([0-9a-f]{64})  (.+)$/.exec(lines[index]!);
    if (!match || match[2] !== name) throw new Error("Checksum filename, order or hash is invalid");
    return [name, match[1]!];
  }));
}

export function verifyChecksum(name: string, bytes: Uint8Array, expected: string) {
  const hash = createHash("sha256").update(bytes).digest("hex");
  if (hash !== expected) throw new Error(`Checksum mismatch: ${name}; do not mix rebuilt and published artifacts`);
  return hash;
}

async function publicBytes(url: string) {
  // Deliberately anonymous; authenticated downloads are not public evidence.
  const response = await fetch(url);
  if (!response.ok) throw new Error(`Asset is not public: ${url}`);
  return new Uint8Array(await response.arrayBuffer());
}

export async function publicAssets(tag: string) {
  const release = stableRelease(tag);
  const sums = parseChecksums(tag, new TextDecoder().decode(await publicBytes(release.checksumUrl)));
  const hash = verifyChecksum(release.archive, await publicBytes(release.url), sums[release.archive]!);
  const sourceHash = verifyChecksum(release.source, await publicBytes(release.sourceUrl), sums[release.source]!);
  return { ...release, hash, sourceHash };
}

export async function gh(...args: string[]) {
  const result = Bun.spawn(["gh", ...args], { stdout: "pipe", stderr: "inherit" });
  const output = await new Response(result.stdout).text();
  if (await result.exited !== 0) throw new Error(`gh ${args[0]} failed`);
  return output;
}

export function validateMode(mode: string | undefined, tag: string) {
  if (mode !== "publish" && mode !== "verify") throw new Error("Expected publish or verify");
  const release = stableRelease(tag);
  if (mode === "publish" && tag === "v0.1.0") throw new Error("Historical v0.1.0 is immutable; use its version-specific verification recipe");
  return release;
}

export function downloadBody(body: string, tag: string) {
  const release = stableRelease(tag);
  const start = "<!-- mods-downloads:start -->";
  const end = "<!-- mods-downloads:end -->";
  const section = `${start}\n## Downloads\n\n- [Windows runtime ZIP](${release.url})\n- [Complete corresponding source (free)](${release.sourceUrl})\n- [SHA-256 checksums](${release.checksumUrl})\n${end}`;
  if (body.includes(start) || body.includes(end)) {
    if (body.split(start).length !== 2 || body.split(end).length !== 2 || body.indexOf(end) < body.indexOf(start)) throw new Error("Malformed download markers");
    return body.slice(0, body.indexOf(start)) + section + body.slice(body.indexOf(end) + end.length);
  }
  return `${section}\n\n${body}`;
}

// Injectable operations keep recovery tests local and do not publish fixtures.
export async function publishAssets(tag: string, metadata: { assets: { name: string }[]; body?: string }, io: {
  local: (name: string) => Promise<Uint8Array>;
  remote: (url: string) => Promise<Uint8Array>;
  upload: (name: string) => Promise<void>;
  editBody: (body: string) => Promise<void>;
}) {
  const release = validateMode("publish", tag);
  const names = new Set(metadata.assets.map(asset => asset.name));
  const ordered = [release.source, release.archive, "SHA256SUMS"];
  const urls = { [release.source]: release.sourceUrl, [release.archive]: release.url, SHA256SUMS: release.checksumUrl };
  const complete = ordered.every(name => names.has(name));
  const checksumBytes = complete ? await io.remote(release.checksumUrl) : await io.local("SHA256SUMS");
  const sums = parseChecksums(tag, new TextDecoder().decode(checksumBytes));
  // Validate the entire packaged set and every existing public asset before any write.
  if (!complete) {
    for (const name of [release.source, release.archive]) verifyChecksum(name, await io.local(name), sums[name]!);
  }
  for (const name of ordered.filter(name => names.has(name))) {
    const bytes = await io.remote(urls[name]!);
    if (name === "SHA256SUMS") {
      const existing = parseChecksums(tag, new TextDecoder().decode(bytes));
      if (JSON.stringify(existing) !== JSON.stringify(sums)) throw new Error("Public checksum set conflicts with packaged set");
    } else verifyChecksum(name, bytes, sums[name]!);
  }
  for (const name of ordered) {
    if (!names.has(name)) await io.upload(name);
    // Source must be publicly verified before runtime upload.
    const bytes = await io.remote(urls[name]!);
    if (name === "SHA256SUMS") {
      if (JSON.stringify(parseChecksums(tag, new TextDecoder().decode(bytes))) !== JSON.stringify(sums)) throw new Error("Public checksum set changed");
    } else verifyChecksum(name, bytes, sums[name]!);
  }
  const body = downloadBody(metadata.body ?? "", tag);
  if (body !== (metadata.body ?? "")) await io.editBody(body);
}

if (import.meta.main) {
  const [mode, tag, ...extra] = Bun.argv.slice(2);
  if (extra.length) throw new Error("Expected mode and tag only");
  const release = validateMode(mode, tag ?? "");
  const metadata = JSON.parse(await gh("api", `repos/Reilley64/mods/releases/tags/${release.tag}`));
  if (metadata.draft || metadata.prerelease || metadata.tag_name !== release.tag) throw new Error("Release must be published and stable");
  if (mode === "publish") await publishAssets(release.tag, metadata, {
    local: async name => new Uint8Array(await Bun.file(`dist/${name}`).arrayBuffer()),
    remote: publicBytes,
    upload: async name => { await gh("release", "upload", release.tag, `dist/${name}`, "--repo", "Reilley64/mods"); },
    editBody: async body => { await gh("api", "--method", "PATCH", `repos/Reilley64/mods/releases/${metadata.id}`, "-f", `body=${body}`); },
  });
  console.log(JSON.stringify(await publicAssets(release.tag), null, 2));
}
