import { expect, test } from "bun:test";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  release, runtimeFiles, directories, checkLayout, checksumText, verifyChecksums,
  compareFiles, prepare,
} from "../scripts/repack-v010-runtime";

test("runtime inventory retains notices and native adjacency, but not source", () => {
  expect(runtimeFiles).toHaveLength(23);
  expect(runtimeFiles.filter(name => name.startsWith("usvfs/"))).toHaveLength(4);
  expect(runtimeFiles.filter(name => name.startsWith("licenses/"))).toHaveLength(15);
  const listing = [...directories, ...runtimeFiles].join("\n") + "\n";
  expect(() => checkLayout(listing, false)).not.toThrow();
  for (const bad of [listing + "mods-source.tar.gz\n", listing + "mods.exe\n",
    listing.replace("mods.exe", "../mods.exe"), listing.replace("LICENSE\n", ""),
    listing + "unexpected/\n", listing.replace("mods.exe", "MODS.EXE"),
    listing.replace("mods.exe", "usvfs/../mods.exe"), listing.replace("usvfs/\n", ""),
    listing.replace("mods.exe", "mods.exe\\"), listing.replace("mods.exe", "/mods.exe"),
    "./\n" + listing, listing.replace("mods.exe", "./mods.exe")]) {
    expect(() => checkLayout(bad, false)).toThrow();
  }
  const original = ["./", ...[...directories, ...runtimeFiles].map(name => `./${name}`), "./mods-source.tar.gz"].join("\n") + "\n";
  expect(() => checkLayout(original, true)).not.toThrow();
  expect(() => checkLayout(original.replace("./mods-source.tar.gz\n", ""), false)).toThrow();
});

test("new checksum identity is exactly runtime then complete source, lowercase with LF", () => {
  const runtimeHash = "1".repeat(64);
  const text = `${runtimeHash}  mods-v0.1.0-runtime-r2-x86_64-pc-windows-msvc.zip\n${release.sourceSha256}  mods-v0.1.0-source.tar.gz\n`;
  expect(checksumText(runtimeHash)).toBe(text);
  expect(() => verifyChecksums(runtimeHash, release.sourceSha256, text)).not.toThrow();
  for (const bad of [text.trim(), text.replaceAll("\n", "\r\n"), text + "extra\n",
    text.replace("-runtime-r2-x86", "-x86"), text.replace(runtimeHash, "0".repeat(64))]) {
    expect(() => verifyChecksums(runtimeHash, release.sourceSha256, bad)).toThrow();
  }
  expect(() => verifyChecksums(runtimeHash, "0".repeat(64), text)).toThrow();
});

test("per-file comparison permits only the exact instruction replacement", () => {
  const original = new Map(runtimeFiles.map(name => [name, new TextEncoder().encode(name)]));
  const replacement = new TextEncoder().encode("approved source directions");
  const runtime = new Map(original);
  runtime.set("BUILD-AND-SOURCE.md", replacement);
  expect(compareFiles(original, runtime, replacement).filter(row => row.action === "unchanged")).toHaveLength(22);
  for (const name of ["mods.exe", "LICENSE", "usvfs/usvfs_x86.dll", "BUILD-AND-SOURCE.md"]) {
    const changed = new Map(runtime);
    changed.set(name, new Uint8Array([0]));
    expect(() => compareFiles(original, changed, replacement)).toThrow();
  }
  const missing = new Map(runtime);
  missing.delete("COPYRIGHT.md");
  expect(() => compareFiles(original, missing, replacement)).toThrow();
});

test("wrong input hash fails before creating or changing any output", async () => {
  const root = await mkdtemp(join(tmpdir(), "mods-repack-policy-"));
  try {
    const input = join(root, "wrong.zip");
    const output = join(root, "candidate");
    await writeFile(input, "not the approved release");
    await expect(prepare(input, output)).rejects.toThrow("Original ZIP checksum mismatch");
    expect(await Bun.file(join(output, release.runtime)).exists()).toBe(false);
    const { existsSync } = await import("node:fs");
    expect(existsSync(output)).toBe(false);
    await writeFile(output, "retain this existing file");
    await expect(prepare(input, output)).rejects.toThrow();
    expect(await readFile(output, "utf8")).toBe("retain this existing file");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
