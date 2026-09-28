import { describe, expect, test } from "bun:test";

const workflow = Bun.YAML.parse(await Bun.file(".github/workflows/ci.yml").text()) as any;
const steps = workflow.jobs.check.steps as any[];
const named = (name: string) => steps.find(step => step.name === name);

describe("native release consumption policy", () => {
	test("repository tool checks run independently of native CI", async () => {
		const tools = Bun.YAML.parse(await Bun.file(".github/workflows/tools.yml").text()) as any;
		expect(named("Run repository tool checks")).toBeUndefined();
		expect(tools.on).toEqual({ workflow_call: null });
		expect(workflow.on).toHaveProperty("pull_request");
		expect(workflow.on.push.branches).toEqual(["main"]);
		expect(workflow.on).toHaveProperty("workflow_dispatch");
		expect(workflow.on.schedule).toEqual([{ cron: "23 5 * * 1" }]);
		expect(workflow.jobs.tools.uses).toBe("./.github/workflows/tools.yml");
		expect(workflow.jobs.tools.needs).toBeUndefined();
		expect(workflow.jobs.tools.if).toBeUndefined();
		expect(tools.jobs.tools.needs).toBeUndefined();
		expect(tools.jobs.tools.steps.some((step: any) => step.run === "bun run check:tools")).toBe(true);
		expect(tools.jobs.tools.steps.some((step: any) => step.run === "bun install --frozen-lockfile")).toBe(true);
	});

	test("fetches a pinned release before Rust consumes it", () => {
		const fetchIndex = steps.indexOf(named("Fetch pinned native release"));
		expect(fetchIndex).toBeGreaterThan(steps.indexOf(named("Test native release validation")));
		expect(named("Fetch pinned native release").run).toContain("./native/fetch.ps1");
		for (const name of ["Compile and test x86 adapter bindings", "Run Rust checks"]) {
			expect(steps.indexOf(named(name))).toBeGreaterThan(fetchIndex);
		}
		expect(steps.some(step => step.uses?.startsWith("actions/cache/"))).toBe(false);
		expect(steps.some(step => step.run?.includes("native/build.ps1"))).toBe(false);
	});

	test("keeps combined mods binary publication disabled", () => {
		expect(workflow.jobs.preview.if).toBe("${{ false }}");
		expect(workflow.jobs.preview.steps.some((step: any) => step.run?.includes("native/fetch.ps1"))).toBe(true);
	});
});
