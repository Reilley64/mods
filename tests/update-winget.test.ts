import { expect, test } from "bun:test";
import { checkManifests, updateMode } from "../scripts/update-winget";
import { stableRelease } from "../scripts/stable-release";

const release = { ...stableRelease("v0.2.0"), hash: "a".repeat(64) };
function manifests() {
  const identity = { PackageIdentifier: "Reilley64.Mods", PackageVersion: release.version };
  return [
    { ...identity, ManifestType: "version", DefaultLocale: "en-US" },
    { ...identity, ManifestType: "defaultLocale", PackageLocale: "en-US" },
    { ...identity, ManifestType: "installer", MinimumOSVersion: "10.0.22000.0",
      InstallerType: "zip", NestedInstallerType: "portable",
      NestedInstallerFiles: [{ RelativeFilePath: "mods.exe", PortableCommandAlias: "mods" }],
      Dependencies: { PackageDependencies: [
        { PackageIdentifier: "Microsoft.VCRedist.2015+.x86" },
        { PackageIdentifier: "Microsoft.VCRedist.2015+.x64" },
      ] },
      Installers: [{ Architecture: "x64", InstallerUrl: release.url, InstallerSha256: release.hash.toUpperCase() }],
    },
  ] as any[];
}

test("preparation and submission are explicit modes without an opt-in variable", () => {
  expect(updateMode("prepare")).toBe("prepare");
  expect(updateMode("submit")).toBe("submit");
  for (const invalid of [undefined, "", "update"]) expect(() => updateMode(invalid)).toThrow();
});

test("accepts the verified public runtime and inherited portable contract", () => {
  expect(() => checkManifests(manifests(), release)).not.toThrow();
  const docs = manifests();
  for (const field of ["InstallerType", "NestedInstallerType", "MinimumOSVersion", "NestedInstallerFiles", "Dependencies"]) {
    docs[2].Installers[0][field] = docs[2][field];
    delete docs[2][field];
  }
  expect(() => checkManifests(docs, release)).not.toThrow();
});

test("rejects malformed sets and identity drift in every document", () => {
  for (const docs of [[], [null], manifests().slice(1), [...manifests(), manifests()[0]]]) {
    expect(() => checkManifests(docs, release)).toThrow();
  }
  for (const index of [0, 1, 2]) {
    for (const field of ["PackageIdentifier", "PackageVersion", "ManifestType"]) {
      const docs = manifests();
      docs[index][field] = "wrong";
      expect(() => checkManifests(docs, release)).toThrow();
    }
  }
});

test("rejects mismatched runtime URLs, hashes and installer variants", () => {
  for (const [field, value] of [
    ["InstallerUrl", release.sourceUrl], ["InstallerUrl", `${release.url}?other`],
    ["InstallerSha256", "b".repeat(64)], ["InstallerSha256", "bad"],
    ["Architecture", "x86"], ["InstallerType", "exe"], ["NestedInstallerType", "msi"],
    ["MinimumOSVersion", "10.0.19041.0"],
  ]) {
    const docs = manifests();
    docs[2].Installers[0][field!] = value;
    expect(() => checkManifests(docs, release)).toThrow();
  }
  for (const entries of [[], [null], [{}, {}]]) {
    const docs = manifests();
    docs[2].Installers = entries;
    expect(() => checkManifests(docs, release)).toThrow();
  }
});

test("rejects alias, executable and dependency changes, including installer overrides", () => {
  for (const [field, value] of [
    ["NestedInstallerFiles", []],
    ["NestedInstallerFiles", [{ RelativeFilePath: "other.exe", PortableCommandAlias: "mods" }]],
    ["NestedInstallerFiles", [{ RelativeFilePath: "mods.exe", PortableCommandAlias: "other" }]],
    ["Dependencies", {}],
    ["Dependencies", { PackageDependencies: [{ PackageIdentifier: "Microsoft.VCRedist.2015+.x64" }] }],
  ] as [string, unknown][]) {
    for (const override of [false, true]) {
      const docs = manifests();
      (override ? docs[2].Installers[0] : docs[2])[field] = value;
      expect(() => checkManifests(docs, release)).toThrow();
    }
  }
});

const workflow = Bun.YAML.parse(await Bun.file(".github/workflows/publish-stable.yml").text()) as any;
const updater = await Bun.file("scripts/update-winget.ts").text();
const install = await Bun.file("scripts/test-winget-install.ps1").text();

test("release submission waits for native install and retained evidence on a compatible runner", () => {
  const job = workflow.jobs.winget;
  expect(job.if).toBeUndefined();
  expect(job["runs-on"]).toBe("windows-2025");
  expect(job["timeout-minutes"]).toBe(30);
  expect(job.needs).toBe("stable");
  const steps = job.steps as any[];
  const provision = steps.findIndex(step => step.run?.includes("Repair-WinGetPackageManager"));
  const prepare = steps.findIndex(step => step.id === "prepare");
  const lifecycle = steps.findIndex(step => step.run?.includes("test-winget-install.ps1"));
  const retain = steps.findIndex(step => step.uses?.startsWith("actions/upload-artifact@"));
  const submit = steps.findIndex(step => step.run?.includes("update-winget.ts submit"));
  expect(provision).toBeGreaterThan(0);
  expect(steps[provision].run).toContain("-RequiredVersion 1.29.380");
  expect(steps[provision].run).toContain("-Version 1.29.380");
  expect(prepare).toBeGreaterThan(provision);
  expect(lifecycle).toBeGreaterThan(prepare);
  expect(retain).toBeGreaterThan(lifecycle);
  expect(submit).toBeGreaterThan(retain);
  expect(steps[retain].if).toBe("always()");
  const diagnostics = steps.find(step => step.name === "Collect Winget diagnostics");
  expect(diagnostics.if).toBe("always()");
  expect(diagnostics.run).toContain("Microsoft\\WindowsPackageManagerManifestCreator\\DiagOutputDir");
  expect(steps[submit].if).toBe("${{ success() && steps.prepare.outputs.ready == 'true' }}");
  expect(steps[submit].env.WINGET_CREATE_GITHUB_TOKEN).toBe("${{ secrets.WINGET_CREATE_GITHUB_TOKEN }}");
  expect(job.env.WINGET_CREATE_GITHUB_TOKEN).toBeUndefined();
  for (const step of steps.slice(0, submit)) expect(step.env?.WINGET_CREATE_GITHUB_TOKEN).toBeUndefined();
});

test("generation cannot submit and submit rechecks the existing directory", () => {
  expect(updater).toContain('"--out", output, "--format", "yaml"');
  expect(updater).not.toContain('"--submit"');
  expect(updater).not.toContain('"--token"');
  expect(updater).toContain('await mkdir(output)');
  expect(updater).toContain('identity.version === "0.1.0"');
  expect(updater).toContain('if (!bootstrap.ok)');
  expect(updater.indexOf('checkManifests(await')).toBeLessThan(updater.indexOf('await run(["winget", "validate"'));
  expect(updater.indexOf('await run(["winget", "validate"')).toBeLessThan(updater.indexOf('await run(["./wingetcreate.exe", "submit", directory'));
});

test("native gate refuses personal hosts, old OS, preexisting installations and extraction overrides", () => {
  for (const guard of ["$env:GITHUB_ACTIONS", "$env:RUNNER_ENVIRONMENT", "github-hosted", "10.0.22000.0",
    "Mods is already installed", "Preexisting alias", "archiveExtractionMethod"]) expect(install).toContain(guard);
  expect(install).toContain("$exitCode -notin $AllowedExitCodes");
  expect(install).toContain("$versionReceipt.Stdout.Trim()");
  expect(install).toContain("src/presentation/cli/Cargo.toml");
  expect(install).toContain('"mods $cliVersion"');
  expect(install).toContain("finally {");
  expect(install).toContain("'uninstall', '--id', $packageId, '--exact'");
  expect(install).toContain("'settings', '--disable', 'LocalManifestFiles'");
  expect(install).toContain("Portable alias remains after cleanup");
  for (const bypass of ["--ignore-security-hash", "--ignore-local-archive-malware-scan", "'--force'", "Stop-Process"]) {
    expect(install).not.toContain(bypass);
  }
});

test.skipIf(process.platform !== "win32")("Windows parses the native gate without installing anything", async () => {
  const child = Bun.spawn(["pwsh", "-NoProfile", "-NonInteractive", "-Command",
    "$tokens = $null; $errors = $null; [void][System.Management.Automation.Language.Parser]::ParseFile((Join-Path $pwd 'scripts/test-winget-install.ps1'), [ref]$tokens, [ref]$errors); if ($errors.Count) { $errors | Out-String | Write-Error; exit 1 }"],
    { stdout: "inherit", stderr: "inherit" });
  expect(await child.exited).toBe(0);
});


test("independent PR/main install CI builds the checkout without release credentials", async () => {
  const ci = Bun.YAML.parse(await Bun.file(".github/workflows/ci.yml").text()) as any;
  const candidate = Bun.YAML.parse(await Bun.file(".github/workflows/winget-install.yml").text()) as any;
  const call = ci.jobs["winget-install"];
  expect(call.uses).toBe("./.github/workflows/winget-install.yml");
  expect(call.if).toBe("${{ github.event_name == 'pull_request' || (github.event_name == 'push' && github.ref == 'refs/heads/main') }}");
  expect(call.secrets).toBeUndefined();
  expect(candidate.on).toHaveProperty("workflow_call");
  expect(candidate.permissions).toEqual({ contents: "read" });
  expect(candidate.jobs.install["runs-on"]).toBe("windows-2025");
  const steps = candidate.jobs.install.steps as any[];
  const build = steps.findIndex(step => step.run === "./scripts/package-windows.ps1 -ReleaseId $env:GITHUB_SHA");
  const install = steps.findIndex(step => step.run === "bun scripts/test-winget-candidate.ts $env:GITHUB_SHA");
  expect(build).toBeGreaterThan(0);
  expect(install).toBeGreaterThan(build);
  const artifact = steps.find(step => step.uses?.startsWith("actions/upload-artifact@"));
  expect(artifact.if).toBe("always()");
  expect(artifact.with.path).toBe("dist/");
  expect(artifact.with["retention-days"]).toBe(90);
  const text = JSON.stringify(candidate);
  for (const forbidden of ["secrets.", "WINGET_CREATE_GITHUB_TOKEN", "WINGET_UPDATES_ENABLED", "update-winget.ts", "pull_request_target"]) {
    expect(text).not.toContain(forbidden);
  }
  expect(ci.jobs.preview.if).toBe("${{ false }}");
  expect(updater).not.toContain("WINGET_UPDATES_ENABLED");
  expect(JSON.stringify(workflow)).not.toContain("WINGET_UPDATES_ENABLED");
});
