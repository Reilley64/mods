import { describe, expect, test } from "bun:test";

describe("tracked Rust sources", () => {
  test("contain code", () => {
    const listed = Bun.spawnSync(["git", "ls-files", "*.rs"], { cwd: import.meta.dir + "/.." });
    const paths = new TextDecoder().decode(listed.stdout).split("\n").filter(Boolean);
    const empty = paths.filter((path) => Bun.file(import.meta.dir + "/../" + path).size === 0);
    expect(paths.length).toBeGreaterThan(0);
    expect(empty).toEqual([]);
  });
});
