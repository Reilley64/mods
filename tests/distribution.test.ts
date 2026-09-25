import { describe, expect, test } from "bun:test";

const script = await Bun.file("scripts/package-windows.ps1").text();
const workflow = Bun.YAML.parse(await Bun.file(".github/workflows/ci.yml").text()) as any;

// Static project policy checks, not PowerShell execution or offline rebuild evidence.
describe("Windows distribution recipe policy", () => {
	test("builds both presentation packages from the vendored committed snapshot", () => {
		expect(script).toContain("git archive --format=tar");
		expect(script).toContain("Package only a clean committed checkout");
		expect(script).toContain("cargo vendor --locked --versioned-dirs vendor");
		expect(script).toContain("Set-Content -Encoding utf8NoBOM .cargo/config.toml");
		expect(script).toContain("cargo build --release --frozen --target x86_64-pc-windows-msvc --package mods --package mods-mcp --bins");
		expect(script.indexOf("cargo vendor")).toBeLessThan(script.indexOf("cargo build"));
		expect(script).toContain('@("mods.exe", "mods-mcp.exe")');
	});

	test("reuses native validation and verifies corresponding source before packaging", () => {
		expect(script).toContain("./native/fetch.ps1 -BundleArchive $bundle");
		expect(script).toContain("(Get-FileHash -Algorithm SHA256 -LiteralPath $nativeSource).Hash -ne $release.sourceSha256");
		expect(script).toContain('@("usvfs_x86.dll", "usvfs_x64.dll", "usvfs_proxy_x86.exe", "usvfs_proxy_x64.exe")');
		expect(script).toContain("Copy-Item LICENSE, COPYRIGHT.md $package");
		expect(script).toContain('Copy-Item -Recurse licenses/usvfs "$package/licenses"');
		expect(script).toContain('Copy-Item -Recurse native/artifacts/licenses "$package/licenses/native-release"');
		expect(script.indexOf("Native corresponding-source checksum mismatch")).toBeLessThan(script.indexOf('tar -czf "$package/mods-source.tar.gz"'));
	});

	test("preserves the pinned external header tree at the vendored shim's relative path", () => {
		expect(script).toContain('tar -xf $nativeSource -C $nativeCheckout source-candidate/checkouts/fork.tar.gz');
		expect(script).toContain('tar -xf $forkArchive -C $source include');
		expect(script).toContain('Test-Path include/usvfs/usvfs.h -PathType Leaf');
		expect(script.indexOf("Native corresponding-source checksum mismatch"))
			.toBeLessThan(script.indexOf('tar -xf $nativeSource'));
		expect(script.indexOf('tar -xf $forkArchive'))
			.toBeLessThan(script.indexOf("cargo build"));
		expect(script).not.toContain('Remove-Item -Recurse -Force include');
	});

	test("writes a checksum of the final ZIP outside the ZIP", () => {
		expect(script).toContain('tar -a -cf "$result/$name" -C $package .');
		expect(script).toContain('(Get-FileHash -Algorithm SHA256 -LiteralPath "$result/$name").Hash.ToLowerInvariant()');
		expect(script).toContain('"$hash  $name" | Set-Content -Encoding ascii "$result/SHA256SUMS"');
		expect(script.indexOf('tar -a -cf')).toBeLessThan(script.indexOf('"$hash  $name"'));
	});

	test("keeps the preview gated while sharing candidate packaging", () => {
		const preview = workflow.jobs.preview;
		expect(preview.if).toBe("${{ false }}");
		expect(preview.steps.find((step: any) => step.name === "Package preview").run)
			.toBe("./scripts/package-windows.ps1 -ReleaseId $env:GITHUB_SHA");
		const upload = preview.steps.find((step: any) => step.name === "Upload tagless preview");
		expect(upload.with.path).toContain("dist/SHA256SUMS");
		expect(upload.with.path).toContain("dist/mods-${{ github.sha }}-x86_64-pc-windows-msvc.zip");
		expect(upload.with["retention-days"]).toBe(90);
	});
});
