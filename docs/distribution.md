# Windows distribution candidates

The owner approved CLI-only `v0.1.0` stable publication and release PR merge.
Stable publication is enabled; preview publication and Winget remain disabled.
The actual `v0.1.0` public ZIP/checksum and existing-host portable lifecycle are
now verified; see the [acceptance ledger](acceptance/issue-33.md#published-stable-package-and-portable-lifecycle--2026-09-28).
Configured-`tar` Winget validation/install/alias/uninstall passed on the recorded
existing host; default Shell extraction failed. Human-controlled submission remains pending.
The aggregate version belongs to `version.txt` and `vX.Y.Z`, not the CLI package.
The CLI-only MVP source and workspace contain `mods.exe` but no MCP Presentation;
its ZIP ships only `mods.exe`. The earlier two-executable ZIP is historical evidence,
not an MVP package candidate. Issue #105 owns the MCP Presentation after MVP.
Ordinary CLI mutation success remains quiet. The pinned upstream usvfs baseline
and approved non-modal proxy logging fallback retain the other native behavior
and limits.

## Release-version consistency

Internal Cargo packages remain independently versioned with `publish = false`. Inherited
workspace path dependencies intentionally omit registry version requirements:
Release Please updates member manifests and the lockfile, but skips dependency
versions in the virtual root manifest. A stale root requirement such as `^0.0.0`
otherwise rejects a bumped local crate before Cargo can resolve the release.
Direct member path dependencies also omit version requirements. Internal paths
select the local crates; package versions remain independently managed.
See the [Cargo path dependency contract](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#multiple-locations)
and [Release Please workspace plugin](https://github.com/googleapis/release-please/blob/main/src/plugins/cargo-workspace.ts).

Run `bun run test:release-versions` after Cargo dependencies are cached by the
normal Rust checks. It uses the pinned project toolchain and temporary workspace
copies to check first-release and independent-version resolution, locked metadata,
and unchanged external dependency pins. It also runs in `bun run check:rust`.
This is release-wiring evidence, not a new Windows package acceptance run or
permission to publish. The aggregate version and dependency pins are unchanged.

## Prepare a candidate

On Windows, use PowerShell 7, Git, Windows tar, the pinned Rust toolchain in
`rust-toolchain.toml`, Visual Studio C++ tools, Windows SDK and LLVM/libclang.
Set `LIBCLANG_PATH` to the LLVM `bin` directory. Use a clean committed checkout:

```powershell
./scripts/package-windows.ps1 -ReleaseId (git rev-parse HEAD)
# For a stable candidate, check out its existing aggregate tag, then:
# ./scripts/package-windows.ps1 -ReleaseId v0.2.0
```

The output directory must not exist. `-OutputDirectory` selects another location.
`-BundleArchive` and `-SourceArchive` accept local copies of the pinned native
release assets; the same hash checks apply. The script archives HEAD, vendors all
locked Cargo dependencies (including Git dependencies), and builds the CLI
package from that snapshot with `--frozen`. It does not publish or change tags.

Future candidates contain three files:

- `mods-<ReleaseId>-runtime-x86_64-pc-windows-msvc.zip`;
- `mods-<ReleaseId>-source.tar.gz`;
- `SHA256SUMS`, with runtime then source SHA-256 entries.

The runtime ZIP contains `mods.exe`, exactly four native runtime files under
`usvfs/`, root `LICENSE` and `COPYRIGHT.md`, upstream and native release notices,
and `BUILD-AND-SOURCE.md`. ZIP paths have no `./` prefix. Corresponding source
stays outside the runtime ZIP. Its complete archive contains the tracked project,
Cargo.lock, vendored dependency source and its notices, generated Cargo source
replacement configuration, native source asset and original native binary ZIP.
The latter two are pinned by `native/usvfs-release.json`. The source revision is
recorded in `SOURCE-REVISION.txt`. Do not replace native assets with same-version
upstream binaries. `BUILD-AND-SOURCE.md` preserves these rebuild instructions and adds the exact
source filename, SHA-256 and revision. For stable releases it links to the free,
version-specific source download. For previews it points to the source alongside
the runtime in the same retained Actions artifact. No preview release URL is invented.
Vendored encrypted fixtures and native closure inputs are not filtered or truncated.
`SHA256SUMS` accompanies both archives; it is not inside the ZIP.
Windows 11 and both x86 and x64 Microsoft Visual C++ 2015–2022 Redistributables
are runtime prerequisites. Keep `usvfs/` beside `mods.exe`.

## Consumer source rebuild

The owner waived the disconnected-rebuild acceptance check for #33. Keep host
networking connected; no network-isolated build or offline environment is required.
This waiver does not waive complete corresponding source, notices, source/hash
verification, or accurate build evidence. It is a skipped check, not an offline pass.

Verify the source entry in `SHA256SUMS`, then extract `mods-<ReleaseId>-source.tar.gz`
into a fresh directory. Historical embedded-source packages retain their original filenames. Preinstall the toolchain,
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
cargo build --release --frozen --target x86_64-pc-windows-msvc --package mods --bins
if ($LASTEXITCODE -ne 0) { throw "Consumer rebuild failed" }
```

Keep `.cargo/config.toml`, `vendor/`, and root `include/` intact. The pinned
`vendor/usvfs-sys-0.0.0/build.rs` resolves `../../include` for bindgen and the C++
shim. Cargo does not vendor this external tree. Packaging extracts the complete
`include/` tree from `source-candidate/checkouts/fork.tar.gz` inside the
SHA-256-verified native source asset, without changing the dependency or its build
script. The nested Git archive identifies fork revision
`c23705ce1a4baba19c72156900bb913c9e090307` (the manifest's `forkRevision`);
that checkout preserves the approved upstream headers. The outer source SHA-256 is
`fff05ea6e168694646806295def137ee06d37433eaff415baf88069ede32f92a`.
These headers ship at the source root in the corresponding-source sidecar, not just inside
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
The waiver alone does not authorize publication or waive the remaining acceptance checks.
The owner separately approved CLI-only `v0.1.0` publication. Public-byte and
existing-host portable lifecycle evidence is recorded in [issue #33](acceptance/issue-33.md);
Configured-`tar` Winget install/alias/uninstall passed; submission remains pending.

The disabled preview recipe delegates to the shared script and uploads the ZIP
and complete source sidecar and checksum in one artifact with full-SHA naming and 90-day retention. Enabling it additionally
requires accepted-main-push gating and successful Rust and repository-tool
checks. Stable publication is enabled as described below; Winget remains disabled.
Actual stable-artifact help/version and owned installation/removal passed on the
recorded existing Windows host. This is not a pristine-host or Winget result.
There is no byte-for-byte reproducibility claim.


## Stable publication and recovery

`publish-stable.yml` accepts a published non-prerelease release or recovery dispatch
with its existing exact `vX.Y.Z` tag. It checks out `refs/tags/<tag>`, runs
`bun run check`, and calls the same clean committed packaging script as previews.
The nonzero aggregate `version.txt` must match the tag. No component tag qualifies.
The owner approved enabling stable publication for CLI-only `v0.1.0` and merging
the regenerated release PR #36. Only the stable job's `false &&` gate was removed;
the dispatch/non-prerelease event condition and release validation remain.
There is no new manual approval environment: merging the release PR is publication
approval. Public ZIP/checksum verification and bounded existing-host portable
lifecycle results are now recorded in the acceptance ledger. These results are
separate from the earlier approval. The local Winget check below is limited to
configured-`tar` extraction; submission remains outstanding.

Future publication adds missing assets only. It never replaces public bytes,
deletes a release, or recreates a version. The generic publisher refuses historical
`v0.1.0` publication before any write; the existing tag, assets and version-specific
r2 helper remain unchanged. Future names do not fall back to the old embedded-source ZIP.

The Windows workflow extracts the actual runtime through Windows Shell, compares
every payload path and hash, and runs packaged `--help` and `--version` before upload.
Run locally with PowerShell 7 in STA mode:

```powershell
pwsh -NoProfile -STA -File ./scripts/test-windows-package.ps1 -ReleaseId v0.2.0
```

The smoke uses fresh temporary outputs, retains failures, and removes only its own
successful outputs. It does not change extractor settings or install anything.
The always-on Windows CI synthetic regression exercises the same ZIP formatter and
Shell extractor. It is not evidence of an actual production package build.

Publication verifies the complete local checksum set and all existing public assets
before adding anything. It uploads source first and verifies its anonymous public
hash before adding runtime, then publishes the original two-entry checksum file.
Only after all three public assets pass verification does it add an idempotent
marked download section to the release body, preserving the changelog.

Partial recovery requires the original candidate, retained as a workflow artifact
for 90 days. A rebuilt candidate with different bytes fails closed; do not mix it
with existing assets. Restore the original candidate to `dist/` and run
`bun scripts/stable-release.ts publish <tag>`. The workflow may rebuild on dispatch;
that does not authorize replacing assets if the rebuilt hashes differ. A complete
public set can be verified with `bun scripts/stable-release.ts verify <tag>` without
local `dist/`. Visibility delays fail verification safely; retry after visibility
returns. No URL or hash in tests is evidence of publication.

## Winget bootstrap and later updates (disabled)

No product manifest is committed yet. The public `v0.1.0` ZIP/hash and bounded
existing-host portable ZIP lifecycle are verified in the acceptance ledger.
The separately approved hand-authored manifest passed native validation and
existing-host installation/alias/removal with WinGet's supported `tar` extractor.
The default Windows Shell extractor failed; preserve that result rather than
claiming default-extractor or pristine-host acceptance. Both temporary settings
were restored. See the [local Winget evidence](acceptance/issue-33.md#local-winget-validation-and-configured-tar-lifecycle--2026-09-28).

The retained manifest is hand-authored, not WingetCreate output. Before initial
human-controlled submission, reverify current public bytes with
the recorded version-specific `v0.1.0` verification recipe (not the future-layout publisher). The recommended interactive
`wingetcreate new <verified-url> --out <manifest-directory>` workflow remains
available; any regenerated manifest must be reviewed and validated again.
Do not accept its submission prompt without separate owner approval.
Use package identifier `Reilley64.Mods`, package name `mods`, and aggregate version
`0.1.0`. The installer manifest must describe:

- `Architecture: x64`, `InstallerType: zip`, `NestedInstallerType: portable`;
- `NestedInstallerFiles` entries with `RelativeFilePath: mods.exe` and
  `PortableCommandAlias: mods`;
- Windows 11 minimum (`MinimumOSVersion: 10.0.22000.0`) and dependencies on both
  `Microsoft.VCRedist.2015+.x86` and `Microsoft.VCRedist.2015+.x64`;
- exact verified `InstallerUrl` and `InstallerSha256`, and no self-updater.

Validate the actual manifest with `winget validate --manifest <directory>`.
Record the target's prerequisite state and original Winget settings. For local
testing, enable local manifests only if needed, install the local manifest
silently, run the `mods` alias, and uninstall silently. Restore settings changed
by the test. The completed check used existing reill/SID1003 and temporary
`installBehavior.archiveExtractionMethod: tar`, not default Shell extraction. Confirm the whole
ZIP remains installed, especially `usvfs/` beside `mods.exe`, and that uninstall
removes the alias and package files. WinGet adds its own hidden tracking database
inside the portable installation; verify the 24 release files separately and
preserve that database until native uninstall. Merely listing `mods.exe` in a
manifest does not prove runtime layout or VFS behavior. Preserve
logs, tool versions, hashes, and managed Steam Data unchanged evidence. Record
accepted manifest validation, ZIP layout, and clean install/run/remove evidence
before submitting the human-controlled initial package.

### Draft activation preparation (disabled)

Automatic updates require the repository Actions variable
`WINGET_UPDATES_ENABLED` to equal the exact string `true`. An absent variable
keeps the job disabled. Preparing or merging this code does not configure the
variable or credentials. Preview publication remains disabled independently.

The Windows update job runs only after stable publication and anonymous
verification of the runtime ZIP, full corresponding source, and checksum set.
It checks the exact merged version directory and open PR changed paths; an
existing version is reported rather than submitted again. API errors and
incomplete searches stop the job. The v0.1.0 bootstrap must already be merged;
the updater will never generate or submit that human-controlled version.

For a new version, the job performs these steps in order:

1. Generate YAML with pinned WingetCreate into a fresh directory, **without**
   `--submit`. Preserve the exact three generated manifests.
2. Check the package identifier/version, verified runtime URL/hash, single x64
   ZIP installer, nested portable `mods.exe` with alias `mods`, Windows 11
   minimum, and both VC runtime dependencies. Installer-level overrides are
   checked, not just root fields.
3. Run native `winget validate` against that exact version directory.
4. On a disposable GitHub-hosted Windows 2025 runner, install the local manifest
   using the default ZIP handler, run the installed alias's `--version` and
   `--help`, and uninstall the owned package. Restore temporary local-manifest
   settings and retain command receipts/native diagnostics. No tar fallback,
   OS bypass, or personal-machine installation is permitted. Windows 2022 is
   below the manifest's Windows 11 minimum build and is not used for this gate.
5. Retain manifests and diagnostics as an Actions artifact. Reverify public
   assets and manifest fields, validate again, then submit **the same directory**
   with `wingetcreate submit`, not a second `update --submit` generation.

The submission token is supplied only to the final step through
`WINGET_CREATE_GITHUB_TOKEN`, never a command-line argument. Workflow concurrency
serializes retries per tag. A failed install, smoke command, cleanup, validation,
or evidence upload blocks submission. Winget failures do not roll back the
already-published GitHub Release. This release-job gate is not a general PR
install test, a fresh-user prerequisite test, or upstream validation clearance.

Before taking the activation PR out of draft:

- Confirm microsoft/winget-pkgs#442597 is merged and v0.1.0 is available in the
  catalog. Successful upstream checks alone are not a merge.
- Review the generated manifests and obtain a successful native Windows CI
  install/help/version/uninstall receipt. Local Bun policy tests do not prove
  Winget provisioning or the native lifecycle. This remains pending while the
  job is disabled; do not enable it merely to claim preparation is complete.
- Separately approve configuration of the `winget` environment and a classic PAT
  with `public_repo` as `WINGET_CREATE_GITHUB_TOKEN`. Fine-grained PATs are not
  supported by the pinned WingetCreate. Do not reuse Release Please credentials.
- Separately approve setting the repository variable to `true`. Until then,
  keep it absent or `false`; no credentials or settings are provisioned by this PR.

The workflow pins WingetCreate v1.12.13.0 with the SHA-256 reported by Microsoft's
release API on 2026-09-25. No project dependencies are added. Live Microsoft
references used for CLI and manifest fields:

- https://github.com/microsoft/winget-create/blob/main/doc/new.md
- https://github.com/microsoft/winget-create/blob/main/doc/update.md
- https://github.com/microsoft/winget-create/blob/main/doc/token.md
- https://github.com/microsoft/winget-pkgs/blob/master/doc/manifest/schema/1.10.0/installer.md

The original preparation used direct upstream documents when Context7 was
unavailable. Later local testing used WinGet `1.29.380`; exact-version source
and current documentation confirmed the portable tracking database, dependency
handling and supported `tar` option. Manifest validation and the configured-tar
lifecycle passed within the recorded limits. No submission occurred.
