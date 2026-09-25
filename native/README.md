# Pinned usvfs release

mods consumes the public <https://github.com/Reilley64/usvfs-rs> fork. The fork
owns the unmodified upstream source, native build, raw `usvfs-sys` crate and
exception shim. mods retains its safe wrapper, configuration and process policy.
The upstream source revision remains
`57f1ea5e6ad13f7435a7af184748e6c1312c5637`. Do not substitute the upstream
v0.5.7.2 binaries: their source revision differs despite the same version string.

## Fetch the native bundle

Use PowerShell 7 and an authorized script host:

```powershell
./native/fetch.ps1
$env:MODS_USVFS_ARTIFACTS = "$pwd/native/artifacts/bin"
$env:LIBCLANG_PATH = "C:/path/to/LLVM/bin"
cargo build --locked --target x86_64-pc-windows-msvc --package mods --package mods-mcp
```

`native/usvfs-release.json` must pin an actually published release before this
command can succeed. A missing manifest fails explicitly. Never fill it with
placeholder checksums, an unpublished release or `latest`.

The manifest contract is:

| Field | Value |
| --- | --- |
| `schema` | `1` |
| `repository` | `Reilley64/usvfs-rs` |
| `tag` | `usvfs-0.5.7.2-rs.1` |
| `bundleAsset` | ZIP asset basename |
| `bundleSha256` | Verified lowercase SHA-256 of the published ZIP |
| `sourceRevision` | The upstream revision above |
| `sourceAsset` | Corresponding-source asset basename |
| `sourceSha256` | Verified lowercase SHA-256 of that source asset |
| `forkRevision` | Full release commit SHA; also pin this in the Cargo dependency |

The downloader uses the explicit HTTPS GitHub release URL. It verifies the ZIP
hash before extraction, then validates `bin/source-revision.txt` and
`bin/artifacts.json`. The latter contains `source`, `configuration` (`Release`),
and `artifacts`, with exactly four filename-to-SHA-256 entries:
`usvfs_x86.dll`, `usvfs_x64.dll`, `usvfs_proxy_x86.exe`, and
`usvfs_proxy_x64.exe`. The release bundle also contains license notices.

It stages into an owned temporary directory and moves the verified bundle into
place only if the destination does not exist. Existing user artifacts are never
overwritten. Use `-InstallDirectory` to choose a new destination.
`-BundleArchive` accepts an offline copy of the same pinned ZIP and performs the
same checks. `-Mode Validate` rechecks an installed bundle against the source and
artifact metadata; it does not prove the original ZIP's hash. No downloaded
binary is executed by this tool. Run offline validation tests with
`pwsh -NoProfile -File native/fetch.tests.ps1`.

## Rust and runtime boundary

The Windows-only Git dependency `usvfs-sys` generates raw bindings and compiles
the exception shim. Cargo still requires Visual Studio C++ tools, Windows SDK
and LLVM/libclang, but it no longer builds the upstream DLLs or vcpkg graph.
Install the repository Rust toolchain and both `x86_64-pc-windows-msvc` and
`i686-pc-windows-msvc` targets. CI fetches the same pinned bundle, tests the x86
adapter and runs workspace checks on x64. It does not run upstream semantic or
injection tests.

mods' build script retains source-marker validation and embeds hashes of all
four exact native inputs from `MODS_USVFS_ARTIFACTS`. Cargo tracks those inputs
for revalidation. Install the four files under `usvfs/` beside `mods.exe` and
`mods-mcp.exe`. The loader never accepts an Environment Root, cwd, PATH or
caller-selected DLL path. It checks every embedded artifact hash before loading
the architecture's controller with DLL-directory/System32-only search. Proxies
remain beside both DLLs. Windows 11 and both x86 and x64 Microsoft Visual C++
2015–2022 Redistributables are runtime prerequisites.

## Source, licenses and publication gate

The combined mods project is GPL-3.0-or-later. See root `LICENSE`, `COPYRIGHT.md`
and `licenses/usvfs/`. Preserve the release bundle's upstream and dependency
notices when packaging. The release's pinned corresponding-source asset and
build instructions live in the fork. `native/usvfs-source.json` retains the
original upstream archive identity for provenance; it is not the bundle pin.
Native rebuilding and shim tests belong in the fork, not this consumer tree.

**Combined mods binary preview publishing remains disabled** by the literal
false gate in the workflow. A native release does not itself satisfy mods' full
corresponding-source requirements. The disabled packaging recipe checks the
native source asset hash and includes its notices, but mods' Rust dependencies
and other required source/build inputs still need a reviewed distribution
arrangement. Registry references alone are not a substitute. Do not enable this
job as part of the native dependency migration.

## Owned lifecycle and tests

The exception shim catches allocation and other C++ exceptions before Rust.
Native failure codes are captured before cleanup; cleanup status/error are
separate fields. Launch output is zeroed. On failure the shim requests termination
of any created root, waits at most the shared five-second bound, and closes
returned handles without transferring them to Rust. An unconfirmed cleanup is
reported and pins native state; termination is not guaranteed.
No failed root is resumed. These rules do not prove successful interception or
detect unreported descendant injection failures.

`VirtualGameView` permits only one controller session per process and is not
`Send`/`Sync`. It owns the upstream parameters and DLL through the shim.
`launch` returns a suspended root already assigned to a kill-on-close Job;
`resume` permits one attempt. #31 owns quoting, resolved paths, stream choice,
cancellation, polling/waiting for complete Job drain, and root status mapping.
`HookedProcess::finish(&mut self)` is explicit and reports errors. A nonempty Job
or query error leaves ownership with the caller, so #31 can continue draining;
it does not terminate or consume the execution. Confirmed drain permits native
teardown. Native teardown failures are reported without retry.

Drop is best-effort emergency cleanup, not the normal completion path. If the Job
is not known empty, it requests termination and waits for confirmed complete Job
drain for at most `MODS_CLEANUP_TIMEOUT_MS` (5,000 ms). The synchronous emergency
wait queries the Job with short sleeps; there is no background supervisor. Only
if drain cannot be confirmed (timeout/query failure) does it retain the native
session until controller exit. Successful emergency drain releases normally.
Drop cannot return cleanup errors; callers requiring those errors must use the
explicit path. Failed root cleanup and native teardown exceptions likewise pin
uncertain native state and prevent reconnecting in that controller. This is
last-resort lifetime safety, not a termination guarantee, filesystem rollback,
or a recovery protocol.

Rust tests remain colocated in the configuration, process and safe wrapper
modules. They cover project-owned mapping, output policy, lifecycle failures and
exact-artifact loading. They do not launch injected processes or reproduce
upstream filesystem tests. The extracted shim's unit tests belong in the fork.

## Evidence boundary

The prior clean build and archive-only evidence is recorded in repository
history and `docs/research/issue-30-upstream-usvfs.md`. It is not evidence that the
new release download or consumer Windows build has passed. Validate both Windows
architectures against the final published manifest and final Git revision before
merging the local CI change. No remote CI or publication is changed by local edits.
