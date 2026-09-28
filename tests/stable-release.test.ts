import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { stableRelease, verifyChecksum, parseChecksums, publishAssets, validateMode, downloadBody } from "../scripts/stable-release";

test("future names and historical publication guard", () => {
  for (const tag of ["v0.0.0", "0.1.0", "v01.2.3", "v1.2.3-rc.1"]) expect(() => stableRelease(tag)).toThrow();
  expect(stableRelease("v0.2.0").archive).toBe("mods-v0.2.0-runtime-x86_64-pc-windows-msvc.zip");
  expect(() => validateMode("publish", "v0.1.0")).toThrow("immutable");
  expect(() => validateMode("invalid", "v0.2.0")).toThrow();
});

const tag = "v0.2.0";
const release = stableRelease(tag);
const runtime = new TextEncoder().encode("runtime fixture");
const source = new TextEncoder().encode("complete source fixture");
const digest = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");
const sums = `${digest(runtime)}  ${release.archive}\n${digest(source)}  ${release.source}\n`;

test("strict two-line checksum set rejects missing, duplicate, extra, malformed and reordered entries", () => {
  expect(parseChecksums(tag, sums)[release.archive]).toBe(digest(runtime));
  const lines = sums.trimEnd().split("\n");
  for (const invalid of [lines[0]!, `${lines[0]}\n${lines[0]}\n`, sums + "extra\n", sums.replace(digest(source), "xyz"), lines.reverse().join("\n"), sums.replace(release.source, "wrong.tar.gz"), sums + "\n"]) {
    expect(() => parseChecksums(tag, invalid)).toThrow();
  }
  expect(() => verifyChecksum(release.archive, source, digest(runtime))).toThrow("Checksum mismatch");
});

function fixture(existing: string[] = []) {
  const local = new Map([[release.archive, runtime], [release.source, source], ["SHA256SUMS", new TextEncoder().encode(sums)]]);
  const remote = new Map(existing.map(name => [name, local.get(name)!]));
  const events: string[] = [];
  return { local, remote, events, io: {
    local: async (name: string) => { events.push(`local:${name}`); if (!local.has(name)) throw new Error("missing candidate"); return local.get(name)!; },
    remote: async (url: string) => { const name = url.split("/").at(-1)!; events.push(`verify:${name}`); if (!remote.has(name)) throw new Error("not public"); return remote.get(name)!; },
    upload: async (name: string) => { events.push(`upload:${name}`); remote.set(name, local.get(name)!); },
    editBody: async (body: string) => { events.push("body"); expect(body).toContain("Original changelog"); },
  } };
}

test("publishes and verifies source before runtime, checksums before body", async () => {
  const f = fixture();
  await publishAssets(tag, { assets: [], body: "Original changelog" }, f.io);
  expect(f.events.filter(event => event.startsWith("upload:"))).toEqual([`upload:${release.source}`, `upload:${release.archive}`, "upload:SHA256SUMS"]);
  expect(f.events.indexOf(`verify:${release.source}`)).toBeLessThan(f.events.indexOf(`upload:${release.archive}`));
  expect(f.events.at(-1)).toBe("body");
});

test("partial recovery verifies original candidate and refuses conflicting rebuilds before writes", async () => {
  for (const names of [[release.source], [release.archive], ["SHA256SUMS"], [release.source, release.archive]]) {
    const f = fixture(names);
    await publishAssets(tag, { assets: names.map(name => ({ name })), body: "Original changelog" }, f.io);
    expect(f.events.at(-1)).toBe("body");
  }
  const f = fixture([release.source]);
  f.remote.set(release.source, runtime);
  await expect(publishAssets(tag, { assets: [{ name: release.source }] }, f.io)).rejects.toThrow("do not mix");
  expect(f.events.some(event => event.startsWith("upload:") || event === "body")).toBe(false);
});

test("complete public set needs no local dist and detects public tampering", async () => {
  const names = [release.source, release.archive, "SHA256SUMS"];
  const f = fixture(names);
  f.local.clear();
  await publishAssets(tag, { assets: names.map(name => ({ name })), body: "Original changelog" }, f.io);
  expect(f.events.some(event => event.startsWith("local:") || event.startsWith("upload:"))).toBe(false);
  f.remote.set(release.archive, source);
  await expect(publishAssets(tag, { assets: names.map(name => ({ name })) }, f.io)).rejects.toThrow();
});

test("download links are prominent and idempotent without replacing changelog", () => {
  const body = downloadBody("Original changelog", tag);
  expect(body).toContain(release.sourceUrl);
  expect(body).toContain(release.url);
  expect(body).toContain(release.checksumUrl);
  expect(downloadBody(body, tag)).toBe(body);
  expect(body.endsWith("Original changelog")).toBe(true);
});

test("Winget duplicate matching uses the exact version directory", async () => {
  const { containsVersion } = await import("../scripts/update-winget");
  expect(containsVersion([{ filename: "manifests/r/Reilley64/Mods/0.1.0/Reilley64.Mods.yaml" }], "0.1.0")).toBe(true);
  expect(containsVersion([{ filename: "manifests/r/Reilley64/Mods/0.1.01/Reilley64.Mods.yaml" }], "0.1.0")).toBe(false);
  expect(containsVersion([{ filename: "manifests/r/Other/Mods/0.1.0/Other.Mods.yaml" }], "0.1.0")).toBe(false);
});

const workflow = Bun.YAML.parse(await Bun.file(".github/workflows/publish-stable.yml").text()) as any;

test("stable publication is active only for dispatch or non-prerelease events while Winget stays blocked", () => {
  expect(workflow.on.release.types).toEqual(["published"]);
  expect(workflow.on.workflow_dispatch.inputs.tag).toMatchObject({ required: true, type: "string" });
  expect(workflow.jobs.stable.if).toBe("${{ (github.event_name == 'workflow_dispatch' || !github.event.release.prerelease) }}");
  expect(workflow.jobs.winget.needs).toBe("stable");
  expect(workflow.jobs.winget.if).toBe("${{ false }}");
});

test("stable activation retains exact-tag validation, checks, packaging and public-byte verification", () => {
  const stable = workflow.jobs.stable;
  const steps = stable.steps as any[];
  const validation = steps.find(step => step.id === "release");
  expect(stable.env.RELEASE_TAG).toBe("${{ github.event.release.tag_name || inputs.tag }}");
  expect(validation.run).toContain("'^v(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$'");
  expect(validation.run).toContain("$env:RELEASE_TAG -eq 'v0.0.0'");
  expect(validation.run).toContain('gh api "repos/$env:GITHUB_REPOSITORY/releases/tags/$env:RELEASE_TAG"');
  expect(validation.run).toContain("if ($LASTEXITCODE -ne 0) { throw 'Existing release is required' }");
  expect(validation.run).toContain("if ($release.draft -or $release.prerelease -or $release.tag_name -cne $env:RELEASE_TAG) { throw 'Published stable release required' }");
  const checkout = steps.find(step => step.uses?.startsWith("actions/checkout@"));
  expect(checkout.with.ref).toBe("refs/tags/${{ steps.release.outputs.tag }}");
  const checks = steps.findIndex(step => step.run === "bun run check");
  const packaging = steps.findIndex(step => step.run === "./scripts/package-windows.ps1 -ReleaseId $env:RELEASE_TAG");
  const publication = steps.findIndex(step => step.run === "bun scripts/stable-release.ts publish $env:RELEASE_TAG");
  expect(steps.indexOf(validation)).toBeLessThan(steps.indexOf(checkout));
  expect(checks).toBeGreaterThan(steps.indexOf(checkout));
  expect(packaging).toBeGreaterThan(checks);
  expect(publication).toBeGreaterThan(packaging);
});

test("stable smoke and retained original candidate precede publication", () => {
  const steps = workflow.jobs.stable.steps as any[];
  const smoke = steps.findIndex(step => step.run?.includes("test-windows-package.ps1"));
  const retained = steps.findIndex(step => step.name === "Retain exact candidate for recovery");
  const publish = steps.findIndex(step => step.run?.includes("stable-release.ts publish"));
  expect(smoke).toBeGreaterThan(0);
  expect(steps[smoke].run).toContain("-STA");
  expect(retained).toBeGreaterThan(smoke);
  expect(publish).toBeGreaterThan(retained);
  expect(steps.find(step => step.id === "release").run).toContain("Historical v0.1.0 publication is immutable");
});

test("public checksum conflicts and missing candidate fail without mutation", async () => {
  const f = fixture(["SHA256SUMS"]);
  f.remote.set("SHA256SUMS", new TextEncoder().encode(sums.replace(digest(source), "0".repeat(64))));
  await expect(publishAssets(tag, { assets: [{ name: "SHA256SUMS" }] }, f.io)).rejects.toThrow("conflicts");
  expect(f.events.some(event => event.startsWith("upload:"))).toBe(false);
  const missing = fixture([release.source]);
  missing.local.clear();
  await expect(publishAssets(tag, { assets: [{ name: release.source }] }, missing.io)).rejects.toThrow("missing candidate");
  expect(missing.events.some(event => event.startsWith("upload:"))).toBe(false);
});

test("public source visibility failure prevents runtime upload", async () => {
  const f = fixture();
  f.io.upload = async name => { f.events.push(`upload:${name}`); };
  await expect(publishAssets(tag, { assets: [] }, f.io)).rejects.toThrow("not public");
  expect(f.events.filter(event => event.startsWith("upload:"))).toEqual([`upload:${release.source}`]);
});
