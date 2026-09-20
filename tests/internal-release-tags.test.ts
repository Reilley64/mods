import { expect, test } from "bun:test";
import { selectInternalTags } from "../scripts/internal-release-tags";

test("selects changed internal versions and never tags the aggregate product", () => {
  expect(
    selectInternalTags(
      {},
      { ".": "0.0.0", domain: "0.0.0", "presentation/cli": "0.0.0" },
    ),
  ).toEqual([]);
  expect(
    selectInternalTags(
      {
        ".": "0.0.0",
        domain: "0.0.0",
        application: "0.1.0",
        "presentation/cli": "0.0.0",
        "presentation/mcp": "0.0.0",
      },
      {
        ".": "0.1.0",
        domain: "0.1.0",
        application: "0.1.0",
        "presentation/cli": "0.1.0",
        "presentation/mcp": "0.2.0",
      },
    ),
  ).toEqual(["domain-v0.1.0", "mods-cli-v0.1.0", "mods-mcp-v0.2.0"]);
});
