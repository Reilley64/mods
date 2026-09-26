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
