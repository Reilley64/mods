import { describe, expect, test } from "bun:test";
import { readFile, readdir } from "node:fs/promises";

import { loadConfig } from "../config";
import { loadStyleRules } from "../policy";
import { extractStyleRules, ruleAppliesTo } from "../rules";

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
		const rules = await loadStyleRules(process.cwd(), (await loadConfig(process.cwd())).styleFiles);

		expect(rules.length).toBeGreaterThan(32);
		expect(rules.some((rule) => rule.title === "Workspace formatting")).toBeFalse();
		expect(rules.every((rule) => rule.badExamples.length > 0 && rule.goodExamples.length > 0)).toBeTrue();
		expect(Object.fromEntries(rules.filter((rule) => rule.appliesTo !== undefined).map((rule) => [rule.id, rule.appliesTo]))).toEqual({
			"application-use-cases-and-ports-use-case-declaration-order": ["src/application/**"],
			"application-use-cases-and-ports-use-case-parameters": ["src/application/**"],
			"application-use-cases-and-ports-reusable-capability-ports": ["src/application/**"],
			"application-use-cases-and-ports-focused-use-case-orchestration": ["src/application/**"],
			"application-use-cases-and-ports-use-case-local-implementation-modules": ["src/application/**"],
			"cli-arguments-required-positional-arguments-and-optional-named-arguments": ["src/presentation/**"],
			"errors-rootcause-lower-layer-results": ["src/domain/**", "src/application/**", "src/infrastructure/**"],
			"errors-presentation-error-allowlists": ["src/presentation/**"],
		});
	});

	test("indexes every configured area file from CODING_STYLE.md", async () => {
		const { styleFiles } = await loadConfig(process.cwd());
		const index = await readFile("CODING_STYLE.md", "utf8");
		const areaFiles = (await readdir("docs/coding-style")).map((file) => `docs/coding-style/${file}`);

		expect([...styleFiles].sort()).toEqual(areaFiles.sort());
		expect(extractStyleRules(index)).toEqual([]);
		for (const file of styleFiles) {
			expect(index).toContain(`](${file})`);
		}
	});

	test("treats Markdown headings inside examples as example content", () => {
		const withHeading = style.replace("cargo fmt --check fails", "#### not a rubric field");

		expect(extractStyleRules(withHeading)[1]?.badExamples).toEqual(["#### not a rubric field"]);
	});

	test("rejects unknown rubric fields", () => {
		expect(() => extractStyleRules(style.replace("#### Violation", "#### Scope"))).toThrow(
			"unknown rubric field Scope",
		);
	});

	test("scopes an item to the files its Applies to globs match", () => {
		const scoped = style.replace(
			"### Guard clauses\n",
			"### Guard clauses\n\n#### Applies to\n\n- `src/application/**`\n- src/domain/**\n",
		);
		const [guardClauses, workspaceFormatting] = extractStyleRules(scoped);

		expect(guardClauses?.appliesTo).toEqual(["src/application/**", "src/domain/**"]);
		expect(workspaceFormatting).not.toHaveProperty("appliesTo");
		expect(ruleAppliesTo(guardClauses!, "src/application/src/installation/install_mod.rs")).toBeTrue();
		expect(ruleAppliesTo(guardClauses!, "src/domain/src/mods.rs")).toBeTrue();
		expect(ruleAppliesTo(guardClauses!, "src/infrastructure/archive/src/adapter.rs")).toBeFalse();
		expect(ruleAppliesTo(guardClauses!, "src/applications/src/lib.rs")).toBeFalse();
		expect(ruleAppliesTo(workspaceFormatting!, "src/infrastructure/archive/src/adapter.rs")).toBeTrue();
	});

	test("rejects an empty, malformed, or escaping Applies to list", () => {
		const withScope = (body: string) => style.replace("### Guard clauses\n", `### Guard clauses\n\n#### Applies to\n\n${body}\n`);

		expect(() => extractStyleRules(withScope(""))).toThrow("empty Applies to list");
		expect(() => extractStyleRules(withScope("src/application/**"))).toThrow("list of repository-relative globs");
		for (const glob of ["/src/**", "../outside/**", "src/../../outside/**", "src\\application\\**"]) {
			expect(() => extractStyleRules(withScope(`- \`${glob}\``))).toThrow("must stay within the repository");
		}
	});

});
