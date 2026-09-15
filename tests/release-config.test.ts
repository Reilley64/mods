import { expect, test } from "bun:test";

test("release metadata follows presentation-owned product packages", async () => {
  const config = await Bun.file("release-please-config.json").json();
  const manifest = await Bun.file(".release-please-manifest.json").json();

  expect(config.packages["."]).toBeUndefined();
  expect(manifest["."]).toBeUndefined();
  expect(config.packages["presentation/cli"]).toMatchObject({
    component: "mods",
    "package-name": "mods",
    "include-component-in-tag": false,
    "skip-github-release": false,
  });
  expect(config.packages["presentation/mcp"]).toMatchObject({
    component: "mods-mcp",
    "package-name": "mods-mcp",
    "skip-github-release": true,
  });
  expect(manifest["presentation/cli"]).toBe("0.0.0");
  expect(manifest["presentation/mcp"]).toBe("0.0.0");
});
