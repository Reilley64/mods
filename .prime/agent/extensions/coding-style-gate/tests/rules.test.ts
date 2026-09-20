import { describe, expect, test } from "bun:test";
import { readFile } from "node:fs/promises";

import { extractStyleRules } from "../rules";

const style = `# Coding style

## Control flow

### Guard clauses

#### Rule

Prefer guard clauses that return early.

#### Violation

The change nests the successful path under a condition whose opposite branch exits.

#### Compliant

The exiting condition is handled first and the successful path stays unnested.

#### Bad example

\`\`\`rust
if ready {
    publish();
} else {
    return Err(NotReady);
}
\`\`\`

#### Good example

\`\`\`rust
if !ready {
    return Err(NotReady);
}

publish();
\`\`\`

### Workspace formatting

#### Rule

Handwritten Rust matches rustfmt output.

#### Violation

The file differs from rustfmt output.

#### Compliant

The file matches rustfmt output.

#### Bad example

\`\`\`text
cargo fmt --check fails
\`\`\`

#### Good example

\`\`\`text
cargo fmt --check passes
\`\`\`
`;

describe("coding style rules", () => {
	test("extracts every rubric item with Jev criteria", () => {
		expect(extractStyleRules(style)).toEqual([
			{
				id: "control-flow-guard-clauses",
				section: "Control flow",
				title: "Guard clauses",
				text: "Prefer guard clauses that return early.",
				violation: "The change nests the successful path under a condition whose opposite branch exits.",
				compliant: "The exiting condition is handled first and the successful path stays unnested.",
				badExamples: ["if ready {\n    publish();\n} else {\n    return Err(NotReady);\n}"],
				goodExamples: ["if !ready {\n    return Err(NotReady);\n}\n\npublish();"],
			},
			{
				id: "control-flow-workspace-formatting",
				section: "Control flow",
				title: "Workspace formatting",
				text: "Handwritten Rust matches rustfmt output.",
				violation: "The file differs from rustfmt output.",
				compliant: "The file matches rustfmt output.",
				badExamples: ["cargo fmt --check fails"],
				goodExamples: ["cargo fmt --check passes"],
			},
		]);
	});

	test("rejects an incomplete rubric item", () => {
		expect(() =>
			extractStyleRules(`## Control flow\n\n### Guard clauses\n\n#### Rule\n\nPrefer guards.\n`),
		).toThrow("missing Violation");
	});

	test("extracts every repository rubric item with examples", async () => {
		const rules = extractStyleRules(await readFile("CODING_STYLE.md", "utf8"));

		expect(rules.length).toBeGreaterThan(32);
		expect(rules.some((rule) => rule.title === "Workspace formatting")).toBeFalse();
		expect(rules.every((rule) => rule.badExamples.length > 0 && rule.goodExamples.length > 0)).toBeTrue();
	});

	test("treats Markdown headings inside examples as example content", () => {
		const withHeading = style.replace("cargo fmt --check fails", "#### not a rubric field");

		expect(extractStyleRules(withHeading)[1]?.badExamples).toEqual(["#### not a rubric field"]);
	});

	test("rejects unknown rubric fields", () => {
		expect(() => extractStyleRules(style.replace("#### Violation", "#### Applies to"))).toThrow(
			"unknown rubric field Applies to",
		);
	});

});
