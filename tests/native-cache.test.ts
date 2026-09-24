import { describe, expect, test } from "bun:test";

const workflow = Bun.YAML.parse(await Bun.file(".github/workflows/ci.yml").text()) as any;
const steps = workflow.jobs.check.steps as any[];
const named = (name: string) => steps.find(step => step.name === name);

describe("native CI cache policy", () => {
	test("requires an exact bundle key and validates before Rust consumes it", () => {
		const restore = named("Restore exact native bundle");
		const save = named("Save verified native bundle");
		expect(restore.with["restore-keys"]).toBeUndefined();
		expect(save.with.key).toBe(restore.with.key);
		const validateIndex = steps.indexOf(named("Validate native bundle"));
		expect(validateIndex).toBeGreaterThan(steps.indexOf(named("Build unmodified upstream native artifacts")));
		expect(validateIndex).toBeLessThan(steps.indexOf(save));
		for (const name of ["Compile and test x86 adapter bindings", "Run Rust checks"]) {
			expect(steps.indexOf(named(name))).toBeGreaterThan(validateIndex);
			expect(named(name).if).not.toContain("cache-hit");
		}
	});

	test("clean runs bypass both restores but still build and validate", () => {
		expect(workflow.on.schedule.length).toBeGreaterThan(0);
		expect(workflow.on.workflow_dispatch.inputs.clean_native.type).toBe("boolean");
		for (const name of ["Restore exact native bundle", "Restore compiled vcpkg dependencies"]) {
			expect(named(name).if).toContain("env.NATIVE_CLEAN_BUILD != 'true'");
		}
		expect(named("Build unmodified upstream native artifacts").if).not.toContain("NATIVE_CLEAN_BUILD");
		expect(named("Validate native bundle").if).not.toContain("cache-hit");
	});

	test("saves native work before unrelated Rust checks can fail", () => {
		for (const name of ["Save verified native bundle", "Save compiled vcpkg dependencies"]) {
			expect(steps.indexOf(named(name))).toBeLessThan(steps.indexOf(named("Run Rust checks")));
			expect(named(name).if).toContain("steps.native-build.outcome == 'success'");
		}
		expect(named("Prepare native cache keys and environment").run).toContain("VCPKG_BINARY_SOURCES=clear;files,$binaryCache,readwrite");
		expect(named("Save compiled vcpkg dependencies").with.key).toBe(named("Restore compiled vcpkg dependencies").with.key);
	});
});
