import { expect, test } from "bun:test";
import { selectInternalTags } from "../scripts/internal-release-tags";

test("selects only changed internal versions and never tags the unreleased baseline", () => {
  expect(selectInternalTags({}, { domain: "0.0.0", application: "0.0.0" })).toEqual([]);
  expect(
    selectInternalTags(
      { domain: "0.0.0", application: "0.1.0", "presentation/cli": "0.0.0" },
      { domain: "0.1.0", application: "0.1.0", "presentation/cli": "0.0.0" },
    ),
  ).toEqual(["domain-v0.1.0"]);
});
