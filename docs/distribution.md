# Windows distribution candidates

Publication remains disabled. This recipe prepares candidates, not release approval.
The aggregate version belongs to `version.txt` and `vX.Y.Z`, not either presentation
package. Both `mods.exe` and `mods-mcp.exe` ship in one ZIP. No product interfaces
change. Latest #32 decisions require structured MCP mutation success; ordinary CLI
mutation success remains quiet. Upstream usvfs behavior and limits remain accepted.

## Prepare a candidate

On Windows, use PowerShell 7, Git, Windows tar, the pinned Rust toolchain in
`rust-toolchain.toml`, Visual Studio C++ tools, Windows SDK and LLVM/libclang.
Set `LIBCLANG_PATH` to the LLVM `bin` directory. Use a clean committed checkout:

```powershell
./scripts/package-windows.ps1 -ReleaseId (git rev-parse HEAD)
# For a stable candidate, check out its existing aggregate tag, then:
# ./scripts/package-windows.ps1 -ReleaseId v0.1.0
```

The output directory must not exist. `-OutputDirectory` selects another location.
`-BundleArchive` and `-SourceArchive` accept local copies of the pinned native
release assets; the same hash checks apply. The script archives HEAD, vendors all
locked Cargo dependencies (including Git dependencies), and builds both packages
from that snapshot with `--frozen`. It does not publish or change tags.

The ZIP contains both executables, exactly four native runtime files under
`usvfs/`, root `LICENSE` and `COPYRIGHT.md`, upstream and native release notices,
and `mods-source.tar.gz`. The source archive contains the tracked project,
Cargo.lock, vendored dependency source and its notices, generated Cargo source
replacement configuration, native source asset and original native binary ZIP.
The latter two are pinned by `native/usvfs-release.json`. The source revision is
recorded in `SOURCE-REVISION.txt`. Do not replace native assets with same-version
upstream binaries. `SHA256SUMS` accompanies the final ZIP; it is not inside it.
Windows 11 and both x86 and x64 Microsoft Visual C++ 2015–2022 Redistributables
are runtime prerequisites. Keep `usvfs/` beside both executables.

## Consumer source rebuild

The owner waived the disconnected-rebuild acceptance check for #33. Keep host
networking connected; no network-isolated build or offline environment is required.
This waiver does not waive complete corresponding source, notices, source/hash
verification, or accurate build evidence. It is a skipped check, not an offline pass.

Extract `mods-source.tar.gz` into a fresh directory. Preinstall the toolchain,
MSVC, Windows SDK, LLVM/libclang and PowerShell listed above. Use a new empty
Cargo home and target directory, with the installed Rust toolchain available
through rustup. From the extracted source root:

```powershell
$env:CARGO_HOME = "$pwd/consumer-cargo-home"
$env:CARGO_TARGET_DIR = "$pwd/consumer-target"
$env:LIBCLANG_PATH = "C:/Program Files/LLVM/bin"
$release = Get-Content -Raw native/usvfs-release.json | ConvertFrom-Json
./native/fetch.ps1 -BundleArchive "native/archives/$($release.bundleAsset)"
$sourceHash = (Get-FileHash -Algorithm SHA256 "native/archives/$($release.sourceAsset)").Hash
if ($sourceHash -ne $release.sourceSha256) { throw "Native source hash mismatch" }
$env:MODS_USVFS_ARTIFACTS = "$pwd/native/artifacts/bin"
cargo build --release --frozen --target x86_64-pc-windows-msvc --package mods --package mods-mcp --bins
if ($LASTEXITCODE -ne 0) { throw "Consumer rebuild failed" }
```

Keep `.cargo/config.toml`, `vendor/`, and root `include/` intact. The pinned
`vendor/usvfs-sys-0.0.0/build.rs` resolves `../../include` for bindgen and the C++
shim. Cargo does not vendor this external tree. Packaging extracts the complete
`include/` tree from `source-candidate/checkouts/fork.tar.gz` inside the
SHA-256-verified native source asset, without changing the dependency or its build
script. The nested Git archive identifies fork revision
`eb4949fb2439fe5b98901e2fb1afceee752a6133` (the manifest's `forkRevision`);
all seven header files were compared byte-for-byte with that pinned checkout.
The outer source SHA-256 is
`961478a1e69cf6b0156e78970181ef6375974aaadd437af5fdfe8e905db85199`.
These headers ship at the source root in `mods-source.tar.gz`, not just inside
the nested native archive. Run from the source root so Cargo
uses that configuration. No registry or Git download is required by Cargo.
This consumer rebuild compiles the Rust packages and shim against the pinned
native runtime; it does **not** rebuild upstream usvfs. Extract the included
native corresponding-source archive and follow its own build instructions for
that step. Required native source/build inputs must remain accounted for;
a hash check alone does not establish source completeness.

## Evidence and publication gate

The disconnected Windows Rust/native rebuild check is waived by the owner.
Do not claim that `--frozen`, a connected build, or origin blocking proves a
disconnected build. Preserve the recorded source inventory and actual build
evidence, including source identities, tool versions, commands, inputs, hashes
and results. Complete corresponding source and required notices remain mandatory.
The waiver does not enable publication or waive the remaining acceptance checks.

The disabled preview recipe delegates to the shared script and uploads the ZIP
and checksum with full-SHA naming and 90-day retention. Enabling it additionally
requires accepted-main-push gating and successful Rust and repository-tool
checks. Stable publication and Winget recipes are described below, but remain disabled.
Packaged-binary smoke tests and clean Windows installation/removal evidence
remain acceptance prerequisites.
There is no byte-for-byte reproducibility claim.


## Stable publication and recovery (disabled)

`publish-stable.yml` accepts a published non-prerelease release or recovery dispatch
with its existing exact `vX.Y.Z` tag. It checks out `refs/tags/<tag>`, runs
`bun run check`, and calls the same clean committed packaging script as previews.
The nonzero aggregate `version.txt` must match the tag. No component tag qualifies.
The workflow has an explicit false job gate. Remove only the `false &&` portion
of the stable condition after accepting the source/build evidence above
and required Windows acceptance evidence. Keep its event
condition. Do not replace this with a new manual approval environment: merging
the release PR is publication approval.

Publication adds missing assets only. It never replaces public ZIP bytes, deletes
a release, or recreates a version. Anonymous HTTPS downloads verify the exact
ZIP filename and SHA-256 from public `SHA256SUMS` before the Winget job can run.
Recovery can restore a missing checksum from the existing public ZIP. A conflicting
checksum or checksum without ZIP requires investigation, not automatic overwrite.
If upload visibility is delayed, verification fails safely; rerun after visibility
is restored. Re-dispatch the exact tag for missing assets, or rerun only the failed
Winget job for a submission failure. A valid GitHub Release survives either failure.
No stable product URL or hash in tests is evidence of actual publication.

## Winget bootstrap and later updates (disabled)

No product manifest is committed yet: there is no verified public `v0.1.0` product
ZIP/hash or clean Windows portable-install evidence. This is an explicit blocker,
not a placeholder manifest. After that release is public, a human must first run
`bun scripts/stable-release.ts verify v0.1.0`, then use its verified URL with
`wingetcreate new <verified-url> --out <manifest-directory>`. Do not submit from
the wizard until reviewing the generated files and testing installation/removal.
Use package identifier `Reilley64.Mods`, package name `mods`, and aggregate version
`0.1.0`. The installer manifest must describe:

- `Architecture: x64`, `InstallerType: zip`, `NestedInstallerType: portable`;
- `NestedInstallerFiles` entries with `RelativeFilePath: mods.exe` and
  `PortableCommandAlias: mods`, plus `RelativeFilePath: mods-mcp.exe` and
  `PortableCommandAlias: mods-mcp`;
- Windows 11 minimum (`MinimumOSVersion: 10.0.22000.0`) and dependencies on both
  `Microsoft.VCRedist.2015+.x86` and `Microsoft.VCRedist.2015+.x64`;
- exact verified `InstallerUrl` and `InstallerSha256`, and no self-updater.

Validate the real generated manifest with `winget validate --manifest <directory>`.
On a clean Windows 11 host, enable local manifests for testing, install the local
manifest silently, run **both aliases**, and uninstall silently. Confirm the whole
ZIP remains installed, especially shared `usvfs/` beside both executables, and
that uninstall removes both aliases and package files. Merely listing the two
executables in a manifest does not prove runtime layout or VFS behavior. Preserve
logs, tool versions, hashes, and managed Steam Data unchanged evidence. Record
accepted manifest validation, ZIP layout, and clean install/run/remove evidence
before submitting the human-controlled initial package.

Only after that bootstrap is merged and those checks pass, remove the Winget
job's literal false gate. This is a one-time readiness block, not per-release
manual approval. Configure the `winget` environment **without required reviewers**
and store a separate classic PAT with `public_repo` as
`WINGET_CREATE_GITHUB_TOKEN`; do not reuse the Release Please App credential.
The update job waits for stable public verification, verifies again, checks the
exact merged version directory and open PR changed paths, and reports an existing
link instead of duplicating a submission. API or incomplete-search errors stop
submission. Subsequent updates preserve the reviewed bootstrap manifest layout
through `wingetcreate update Reilley64.Mods --urls <verified-url> --version
<version> --submit --no-open`. Workflow concurrency serializes retries per tag.
Winget review delays or failures never roll back the GitHub Release.

The workflow pins WingetCreate v1.12.13.0 with the SHA-256 reported by Microsoft's
release API on 2026-09-25. No project dependencies are added. Live Microsoft
references used for CLI and manifest fields:

- https://github.com/microsoft/winget-create/blob/main/doc/new.md
- https://github.com/microsoft/winget-create/blob/main/doc/update.md
- https://github.com/microsoft/winget-create/blob/main/doc/token.md
- https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.10.0/installer.md

Context7 lookup was unavailable because its monthly quota was exhausted; the
upstream documents above were fetched directly instead. No Winget client,
manifest validation, submission, or Windows installation was executed here.
