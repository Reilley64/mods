import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { stableRelease, verifyChecksum } from "../scripts/stable-release";

test("only aggregate nonzero canonical stable tags can enter publication", () => {
  for (const tag of ["v0.0.0", "0.1.0", "mods-cli-v0.1.0", "v01.2.3", "v1.2.3-rc.1", "a".repeat(40)]) {
    expect(() => stableRelease(tag)).toThrow();
  }
  expect(stableRelease("v0.1.0")).toEqual({ tag: "v0.1.0", version: "0.1.0",
    archive: "mods-v0.1.0-x86_64-pc-windows-msvc.zip",
    url: "https://github.com/Reilley64/mods/releases/download/v0.1.0/mods-v0.1.0-x86_64-pc-windows-msvc.zip" });
});

test("public checksum must bind the exact ZIP bytes and stable filename", () => {
  const bytes = new TextEncoder().encode("test ZIP fixture, not release evidence");
  const hash = createHash("sha256").update(bytes).digest("hex");
  const archive = stableRelease("v0.1.0").archive;
  expect(verifyChecksum(archive, bytes, `${hash}  ${archive}\n`)).toBe(hash);
  for (const sums of [`${"0".repeat(64)}  ${archive}`, `${hash}  wrong.zip`, `${hash}  ${archive}\nextra`]) {
    expect(() => verifyChecksum(archive, bytes, sums)).toThrow();
  }
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
