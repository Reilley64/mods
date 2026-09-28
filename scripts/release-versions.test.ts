import { expect, test } from "bun:test";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const bumpedPackages = [
  ["src/domain", "domain"],
  ["src/application", "application"],
  ["src/infrastructure/dependencies", "infrastructure-dependencies"],
  ["src/presentation/cli", "mods"],
  ["src/infrastructure/environment", "infrastructure-environment"],
  ["src/infrastructure/settings", "infrastructure-settings"],
  ["src/infrastructure/game_platform", "infrastructure-game-platform"],
  ["src/infrastructure/archive", "infrastructure-archive"],
  ["src/infrastructure/execution", "infrastructure-execution"],
] as const;

test.each([
  ["first release", ["0.1.0", "0.1.0", "0.1.0", "0.1.0", "0.1.0", "0.1.0", "0.1.0", "0.1.0", "0.1.0"]],
  ["independent releases", ["0.2.0", "0.3.1", "0.4.0", "0.5.2", "0.6.0", "0.7.1", "0.8.0", "0.9.2", "0.10.0"]],
] as const)("local release graph resolves with coherent locked versions: %s", (_, versions) => {
  const directory = mkdtempSync(join(tmpdir(), "mods-release-versions-"));
  try {
    for (const path of ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "src"]) {
      cpSync(join(root, path), join(directory, path), { recursive: true });
    }
    const originalLock = Bun.TOML.parse(readFileSync(join(directory, "Cargo.lock"), "utf8"));
    for (const [index, [path]] of bumpedPackages.entries()) {
      const manifest = join(directory, path, "Cargo.toml");
      const content = readFileSync(manifest, "utf8");
      const parsed = Bun.TOML.parse(content);
      writeFileSync(manifest, content.replace(
        `version = "${parsed.package.version}"`, `version = "${versions[index]}"`,
      ));
    }

    // Exercise our real inherited path wiring after release version changes, not
    // Release Please internals. Cargo must resolve before it can refresh the lock.
    const update = Bun.spawnSync(["cargo", "update", "--offline", "--workspace"], { cwd: directory });
    expect(update.stderr.toString()).not.toContain("failed to select a version");
    expect(update.exitCode).toBe(0);
    const lock = readFileSync(join(directory, "Cargo.lock"), "utf8");
    // Native Rust checks cache host dependencies, not sources for every platform.
    const rustc = Bun.spawnSync(["rustc", "-vV"], { cwd: directory });
    expect(rustc.exitCode).toBe(0);
    const host = /^host: (.+)$/m.exec(rustc.stdout.toString())?.[1];
    expect(host).toBeDefined();
    const metadata = Bun.spawnSync([
      "cargo", "metadata", "--offline", "--locked", "--format-version", "1",
      "--filter-platform", host!,
    ], { cwd: directory });
    expect(metadata.exitCode).toBe(0);
    expect(readFileSync(join(directory, "Cargo.lock"), "utf8")).toBe(lock);

    const graph = JSON.parse(metadata.stdout.toString());
    const updatedLock = Bun.TOML.parse(lock);
    for (const [index, [, name]] of bumpedPackages.entries()) {
      const local = graph.packages.find((pkg: any) => pkg.name === name && pkg.source === null);
      expect(local.version).toBe(versions[index]);
      expect(graph.workspace_members).toContain(local.id);
      expect(updatedLock.package.find((pkg: any) => pkg.name === name && !pkg.source).version)
        .toBe(versions[index]);
    }
    for (const [name, dependencies] of [
      ["application", ["domain"]],
      ["infrastructure-dependencies", [
        "infrastructure-environment", "infrastructure-settings",
        "infrastructure-game-platform", "infrastructure-archive", "infrastructure-execution",
      ]],
      ["mods", ["application", "domain", "infrastructure-dependencies"]],
    ] as const) {
      const local = graph.packages.find((pkg: any) => pkg.name === name && pkg.source === null);
      const node = graph.resolve.nodes.find((item: any) => item.id === local.id);
      for (const dependency of dependencies) {
        const target = graph.packages.find((pkg: any) => pkg.name === dependency && pkg.source === null);
        expect(node.dependencies).toContain(target.id);
      }
    }
    // The repair must not refresh registry or pinned Git dependencies.
    expect(updatedLock.package.filter((pkg: any) => pkg.source))
      .toEqual(originalLock.package.filter((pkg: any) => pkg.source));
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 30_000);
