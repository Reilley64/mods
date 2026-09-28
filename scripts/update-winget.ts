import { appendFile, mkdir, readdir } from "node:fs/promises";
import { gh, publicAssets, stableRelease } from "./stable-release";

export function manifestPath(version: string) {
  stableRelease(`v${version}`);
  return `manifests/r/Reilley64/Mods/${version}/`;
}

export function containsVersion(files: { path?: string; filename?: string }[], version: string) {
  return files.some(file => (file.path ?? file.filename ?? "").startsWith(manifestPath(version)));
}

function object(value: unknown): Record<string, any> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Expected manifest object");
  return value as Record<string, any>;
}

export function checkManifests(documents: unknown[], release: { version: string; url: string; hash: string }) {
  const manifests = documents.map(object);
  if (manifests.length !== 3 || new Set(manifests.map(doc => doc.ManifestType)).size !== 3) {
    throw new Error("Expected exactly one version, defaultLocale and installer manifest");
  }
  for (const type of ["version", "defaultLocale", "installer"]) {
    if (!manifests.some(doc => doc.ManifestType === type)) throw new Error(`Missing ${type} manifest`);
  }
  for (const doc of manifests) {
    if (doc.PackageIdentifier !== "Reilley64.Mods" || doc.PackageVersion !== release.version) {
      throw new Error("Manifest identity differs from verified release");
    }
  }
  const installer = manifests.find(doc => doc.ManifestType === "installer")!;
  if (!Array.isArray(installer.Installers) || installer.Installers.length !== 1) throw new Error("Expected one installer");
  // Installer-level fields override inherited root fields in the WinGet schema.
  const entry = { ...installer, ...object(installer.Installers[0]) };
  if (entry.Architecture !== "x64" || entry.InstallerType !== "zip" || entry.NestedInstallerType !== "portable" ||
      entry.MinimumOSVersion !== "10.0.22000.0") throw new Error("Portable platform contract changed");
  if (entry.InstallerUrl !== release.url || !/^[a-fA-F0-9]{64}$/.test(entry.InstallerSha256 ?? "") ||
      entry.InstallerSha256.toLowerCase() !== release.hash) throw new Error("Installer differs from verified public runtime");
  const nested = entry.NestedInstallerFiles;
  if (!Array.isArray(nested) || nested.length !== 1 || nested[0]?.RelativeFilePath !== "mods.exe" ||
      nested[0]?.PortableCommandAlias !== "mods") throw new Error("Portable executable or alias changed");
  const dependencies = entry.Dependencies?.PackageDependencies;
  const expected = ["Microsoft.VCRedist.2015+.x64", "Microsoft.VCRedist.2015+.x86"];
  if (!Array.isArray(dependencies) || JSON.stringify(dependencies.map(dep => dep?.PackageIdentifier).sort()) !== JSON.stringify(expected)) {
    throw new Error("Both VC runtime dependencies are required");
  }
}

export function updateMode(mode: string | undefined, enabled: string | undefined) {
  if (mode !== "prepare" && mode !== "submit") throw new Error("Expected prepare or submit");
  if (mode === "submit" && enabled !== "true") throw new Error("Submission requires WINGET_UPDATES_ENABLED=true");
  return mode;
}

async function run(command: string[]) {
  const child = Bun.spawn(command, { stdout: "inherit", stderr: "inherit" });
  if (await child.exited !== 0) throw new Error(`${command[0]} ${command[1]} failed; retain evidence and retry after inspection`);
}

async function existingUpdate(version: string) {
  const path = manifestPath(version);
  const merged = await fetch(`https://api.github.com/repos/microsoft/winget-pkgs/contents/${path}`);
  if (merged.ok) return `https://github.com/microsoft/winget-pkgs/tree/master/${path}`;
  if (merged.status !== 404) throw new Error(`Winget lookup failed: ${merged.status}`);
  const search = JSON.parse(await gh("api", "--method", "GET", "search/issues",
    "-f", "q=repo:microsoft/winget-pkgs is:pr is:open Reilley64.Mods in:title", "-f", "per_page=100"));
  if (search.incomplete_results || search.total_count > 100) throw new Error("Incomplete duplicate lookup; retry after inspection");
  for (const pr of search.items) {
    const pages = JSON.parse(await gh("api", "--paginate", "--slurp", `repos/microsoft/winget-pkgs/pulls/${pr.number}/files`));
    if (containsVersion(pages.flat(), version)) return pr.html_url as string;
  }
}

if (import.meta.main) {
  const [modeArg, tag, ...extra] = Bun.argv.slice(2);
  const mode = updateMode(modeArg, process.env.WINGET_UPDATES_ENABLED);
  if (extra.length) throw new Error("Expected mode and stable tag only");
  const identity = stableRelease(tag ?? "");
  if (identity.version === "0.1.0") throw new Error("Human-controlled v0.1.0 bootstrap cannot be automated");
  const bootstrap = await fetch("https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/r/Reilley64/Mods/0.1.0");
  if (!bootstrap.ok) throw new Error("Human-controlled v0.1.0 bootstrap must be merged first");
  const existing = await existingUpdate(identity.version);
  if (existing) {
    console.log(`Already merged or open: ${existing}`);
  } else {
    // Anonymous verification covers both the runtime and complete corresponding source.
    const release = await publicAssets(identity.tag);
    const output = "dist/winget";
    const directory = `${output}/${manifestPath(release.version)}`;
    if (mode === "prepare") {
      await mkdir("dist", { recursive: true });
      await mkdir(output); // A retry must not silently reuse a partial manifest set.
      await run(["./wingetcreate.exe", "update", "Reilley64.Mods", "--urls", `${release.url}|x64`,
        "--version", release.version, "--out", output, "--format", "yaml"]);
    }
    const files = await readdir(directory);
    if (files.length !== 3 || files.some(file => !file.endsWith(".yaml"))) throw new Error("Unexpected generated manifest files");
    checkManifests(await Promise.all(files.map(async file => Bun.YAML.parse(await Bun.file(`${directory}/${file}`).text()))), release);
    await run(["winget", "validate", "--manifest", directory, "--disable-interactivity"]);
    if (mode === "prepare") {
      if (process.env.GITHUB_OUTPUT) await appendFile(process.env.GITHUB_OUTPUT, `directory=${directory}\nversion=${release.version}\nready=true\n`);
    } else {
      if (!process.env.WINGET_CREATE_GITHUB_TOKEN) throw new Error("Winget environment token is required");
      await run(["./wingetcreate.exe", "submit", directory, "--no-open"]);
    }
  }
}
