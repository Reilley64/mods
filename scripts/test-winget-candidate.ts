import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

export function candidateNames(revision: string) {
  if (!/^[0-9a-f]{40}$/.test(revision)) throw new Error("Expected full lowercase HEAD SHA");
  return {
    runtime: `mods-${revision}-runtime-x86_64-pc-windows-msvc.zip`,
    source: `mods-${revision}-source.tar.gz`,
  };
}

export async function verifyCandidate(dist: string, revision: string) {
  const names = candidateNames(revision);
  const lines = (await readFile(join(dist, "SHA256SUMS"), "ascii"))
    .replace(/\r\n/g, "\n").replace(/\n$/, "").split("\n");
  for (const [index, name] of [names.runtime, names.source].entries()) {
    const match = /^([0-9a-f]{64})  (.+)$/.exec(lines[index] ?? "");
    if (lines.length !== 2 || !match || match[2] !== name) throw new Error("Unexpected SHA256SUMS entries");
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(join(dist, name))) hash.update(chunk);
    if (hash.digest("hex") !== match[1]) throw new Error(`Checksum mismatch: ${name}`);
  }
  return { ...names, hash: lines[0]!.slice(0, 64) };
}

export function manifests(version: string, url: string, hash: string) {
  if (!/^(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/.test(version)) throw new Error("Invalid product version");
  if (!/^http:\/\/127\.0\.0\.1:\d+\/[a-z0-9_-]+\.zip$/.test(url) || !/^[0-9a-f]{64}$/.test(hash)) {
    throw new Error("Invalid local candidate URL or hash");
  }
  const header = `PackageIdentifier: Reilley64.Mods\nPackageVersion: ${version}\n`;
  return {
    "Reilley64.Mods.yaml": `${header}DefaultLocale: en-US\nManifestType: version\nManifestVersion: 1.12.0\n`,
    "Reilley64.Mods.locale.en-US.yaml": `${header}PackageLocale: en-US\nPublisher: Reilley64\nPackageName: mods\nLicense: GPL-3.0-or-later\nShortDescription: Mod manager\nManifestType: defaultLocale\nManifestVersion: 1.12.0\n`,
    "Reilley64.Mods.installer.yaml": `${header}InstallerType: zip\nNestedInstallerType: portable\nNestedInstallerFiles:\n  - RelativeFilePath: mods.exe\n    PortableCommandAlias: mods\nMinimumOSVersion: 10.0.22000.0\nDependencies:\n  PackageDependencies:\n    - PackageIdentifier: Microsoft.VCRedist.2015+.x64\n    - PackageIdentifier: Microsoft.VCRedist.2015+.x86\nInstallers:\n  - Architecture: x64\n    InstallerUrl: ${url}\n    InstallerSha256: ${hash}\nManifestType: installer\nManifestVersion: 1.12.0\n`,
  };
}

export function serveCandidate(path: string, name: string) {
  const server = Bun.serve({
    hostname: "127.0.0.1", port: 0,
    fetch(request) {
      return new URL(request.url).pathname === `/${name}` && ["GET", "HEAD"].includes(request.method)
        ? new Response(Bun.file(path)) : new Response("Not found", { status: 404 });
    },
  });
  return { server, url: `http://127.0.0.1:${server.port}/${name}` };
}

async function run(args: string[]) {
  const child = Bun.spawn(args, { stdout: "inherit", stderr: "inherit" });
  if (await child.exited !== 0) throw new Error(`${args[0]} ${args[1]} failed`);
}

if (import.meta.main) {
  const [revision, ...extra] = Bun.argv.slice(2);
  candidateNames(revision ?? "");
  if (extra.length) throw new Error("Expected exactly one HEAD SHA");
  const head = Bun.spawnSync(["git", "rev-parse", "HEAD"], { stdout: "pipe", stderr: "inherit" });
  if (head.exitCode !== 0 || head.stdout.toString().trim() !== revision) throw new Error("Candidate revision differs from HEAD");
  const root = process.cwd();
  const dist = join(root, "dist");
  const candidate = await verifyCandidate(dist, revision!);
  const version = (await readFile(join(root, "version.txt"), "utf8")).trim();
  const { server, url } = serveCandidate(join(dist, candidate.runtime), candidate.runtime);
  try {
    const directory = join(dist, "winget-candidate");
    await mkdir(directory);
    for (const [name, text] of Object.entries(manifests(version, url, candidate.hash))) {
      await writeFile(join(directory, name), text, { flag: "wx" });
    }
    await run(["pwsh", "-NoProfile", "-File", join(root, "scripts/test-winget-install.ps1"),
      "-ManifestDirectory", directory, "-Version", version]);
  } finally {
    server.stop(true);
  }
}
