import { describe, expect, test } from "bun:test";
import { checkDependencyGraphAsync, validateProjectGraph } from "../scripts/check-dependency-graph";

describe("workspace dependency graph", () => {
  test("accepts the issue #18 workspace", async () => {
    expect(await checkDependencyGraphAsync()).toEqual([]);
  });

  test("rejects a forbidden project dependency reported from a Cargo dev-dependency section", () => {
    expect(
      validateProjectGraph({
        packages: [
          { name: "mods", dependencies: [{ name: "cli" }, { name: "mcp" }, { name: "infrastructure" }] },
          { name: "domain", dependencies: [] },
          { name: "application", dependencies: [{ name: "domain" }, { name: "cli", kind: "dev" }] },
          { name: "environment", dependencies: [{ name: "application" }, { name: "domain" }] },
          { name: "settings", dependencies: [{ name: "application" }, { name: "domain" }] },
          { name: "game-platform", dependencies: [{ name: "application" }, { name: "domain" }] },
          { name: "archive", dependencies: [{ name: "application" }, { name: "domain" }] },
          { name: "execution", dependencies: [{ name: "application" }, { name: "domain" }] },
          { name: "infrastructure", dependencies: [{ name: "environment" }, { name: "settings" }, { name: "game-platform" }, { name: "archive" }, { name: "execution" }, { name: "application" }, { name: "domain" }] },
          { name: "cli", dependencies: [{ name: "application" }, { name: "domain" }] },
          { name: "mcp", dependencies: [{ name: "application" }, { name: "domain" }] },
        ],
      }),
    ).toContain("application dependencies must be domain");
  });
});


test("rejects an unexpected workspace package", () => {
  expect(
    validateProjectGraph({
      packages: [
        { name: "unexpected", dependencies: [] },
      ],
    }),
  ).toContain("workspace must not contain package unexpected");
});
