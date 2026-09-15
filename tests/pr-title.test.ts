import { describe, expect, test } from "bun:test";
import { validatePullRequestTitle } from "../scripts/validate-pr-title";

describe("squash pull request titles", () => {
  test(
    "accepts Conventional Commit titles",
    () => {
      expect(validatePullRequestTitle("feat(cli): add init")).toEqual([]);
    },
    15_000,
  );

  test(
    "rejects invalid titles",
    () => {
      expect(validatePullRequestTitle("add init").length).toBeGreaterThan(0);
    },
    15_000,
  );
});
