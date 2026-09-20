import { describe, expect, test } from "bun:test";
import { validateProjectGraph } from "../scripts/check-dependency-graph";

const validPackages = [
  {
    name: "mods",
    dependencies: [{ name: "application" }, { name: "domain" }, { name: "infrastructure" }],
    manifest_path: "/repo/src/presentation/cli/Cargo.toml",
    targets: [{ name: "mods", kind: ["bin"] }],
  },
  {
    name: "mods-mcp",
    dependencies: [{ name: "application" }, { name: "domain" }, { name: "infrastructure" }],
    manifest_path: "/repo/src/presentation/mcp/Cargo.toml",
    targets: [{ name: "mods-mcp", kind: ["bin"] }],
  },
  { name: "domain", dependencies: [] },
  { name: "application", dependencies: [{ name: "domain" }] },
  { name: "environment", dependencies: [{ name: "application" }, { name: "domain" }] },
  { name: "settings", dependencies: [{ name: "application" }, { name: "domain" }] },
  { name: "game-platform", dependencies: [{ name: "application" }, { name: "domain" }] },
  { name: "archive", dependencies: [{ name: "application" }, { name: "domain" }] },
  { name: "execution", dependencies: [{ name: "application" }, { name: "domain" }] },
  {
    name: "infrastructure",
    dependencies: [
      { name: "environment" },
      { name: "settings" },
      { name: "game-platform" },
      { name: "archive" },
      { name: "execution" },
      { name: "application" },
      { name: "domain" },
    ],
  },
];

describe("workspace dependency graph", () => {
  test("accepts the presentation-owned binary workspace fixture", () => {
    expect(validateProjectGraph({ packages: validPackages })).toEqual([]);
  });

  test("rejects a presentation library target", () => {
    const packages = validPackages.map((item) => ({
      ...item,
      dependencies: [...item.dependencies],
      targets: item.targets ? [...item.targets] : undefined,
    }));
    const mods = packages.find((item) => item.name === "mods");
    mods?.targets?.push({ name: "cli", kind: ["lib"] });

    expect(validateProjectGraph({ packages })).toContain("mods must be binary-only");
  });

  test("rejects a forbidden project dependency reported from a Cargo dev-dependency section", () => {
    const packages = validPackages.map((item) => ({
      ...item,
      dependencies: [...item.dependencies],
    }));
    const application = packages.find((item) => item.name === "application");
    application?.dependencies.push({ name: "mods" });

    expect(validateProjectGraph({ packages })).toContain("application dependencies must be domain");
  });
});

test("rejects an unexpected workspace package", () => {
  expect(validateProjectGraph({ packages: [{ name: "unexpected", dependencies: [] }] })).toContain(
    "workspace must not contain package unexpected",
  );
});
