import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { candidateNames, manifests, serveCandidate, verifyCandidate } from "../scripts/test-winget-candidate";

const revision = "a".repeat(40);

test("verifies exact runtime and corresponding source before serving", async () => {
  const directory = mkdtempSync(join(tmpdir(), "mods-candidate-"));
  try {
    const { runtime, source } = candidateNames(revision);
    const hash = (text: string) => createHash("sha256").update(text).digest("hex");
    writeFileSync(join(directory, runtime), "runtime");
    writeFileSync(join(directory, source), "source");
    writeFileSync(join(directory, "SHA256SUMS"), `${hash("runtime")}  ${runtime}\n${hash("source")}  ${source}\n`);
    expect((await verifyCandidate(directory, revision)).hash).toBe(hash("runtime"));
    writeFileSync(join(directory, source), "tampered");
    await expect(verifyCandidate(directory, revision)).rejects.toThrow("Checksum mismatch");
    writeFileSync(join(directory, source), "source");
    writeFileSync(join(directory, "SHA256SUMS"), `${hash("runtime")}  ${source}\n${hash("source")}  ${runtime}\n`);
    await expect(verifyCandidate(directory, revision)).rejects.toThrow("Unexpected SHA256SUMS");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("generates local portable WinGet manifest and serves only candidate ZIP", async () => {
  const directory = mkdtempSync(join(tmpdir(), "mods-candidate-"));
  const name = candidateNames(revision).runtime;
  const path = join(directory, name);
  writeFileSync(path, "ZIP contents");
  const { server, url } = serveCandidate(path, name);
  try {
    expect(server.hostname).toBe("127.0.0.1");
    expect(await (await fetch(url)).text()).toBe("ZIP contents");
    const head = await fetch(url, { method: "HEAD" });
    expect(head.status).toBe(200);
    expect(await head.text()).toBe("");
    expect((await fetch(url, { method: "POST" })).status).toBe(404);
    expect((await fetch(url.replace(name, "unrelated.zip"))).status).toBe(404);
    const docs = manifests("0.1.0", url, "b".repeat(64));
    expect(Object.keys(docs)).toHaveLength(3);
    for (const text of Object.values(docs)) {
      const parsed = Bun.YAML.parse(text) as { ManifestType: string };
      expect(text.split("\n")[0]).toBe(`# yaml-language-server: $schema=https://aka.ms/winget-manifest.${parsed.ManifestType}.1.12.0.schema.json`);
    }
    const installer = Bun.YAML.parse(docs["Reilley64.Mods.installer.yaml"]!);
    expect(installer).toMatchObject({ PackageIdentifier: "Reilley64.Mods", PackageVersion: "0.1.0",
      MinimumOSVersion: "10.0.22000.0", InstallerType: "zip", NestedInstallerType: "portable",
      NestedInstallerFiles: [{ RelativeFilePath: "mods.exe", PortableCommandAlias: "mods" }],
      Dependencies: { PackageDependencies: [
        { PackageIdentifier: "Microsoft.VCRedist.2015+.x64" },
        { PackageIdentifier: "Microsoft.VCRedist.2015+.x86" },
      ] }, Installers: [{ Architecture: "x64", InstallerUrl: url, InstallerSha256: "b".repeat(64) }],
    });
    expect(() => manifests("0.1.0", "https://example.com/file.zip", "b".repeat(64))).toThrow();
  } finally {
    server.stop(true);
    rmSync(directory, { recursive: true, force: true });
  }
});
