# Upstream usvfs source

`usvfs/` is an unmodified submodule of
<https://github.com/ModOrganizer2/usvfs>, pinned to
`57f1ea5e6ad13f7435a7af184748e6c1312c5637`.

Initialize with `git submodule update --init native/usvfs`.
Do not substitute published v0.5.7.2 binaries: that release has a different
source revision despite the same version string.

## Clean upstream build

Use Visual Studio 2022 with the x86 and x64 C++ toolchains, Windows SDK,
CMake 3.31 or later, and vcpkg. Set `VCPKG_ROOT` to the vcpkg checkout.
From `native/usvfs`, run each architecture's existing upstream preset:

```text
cmake --preset vs2022-windows-x64 -B build_x64 -DBUILD_TESTING=OFF -DCMAKE_INSTALL_PREFIX=install/Release
cmake --build build_x64 --config Release --target INSTALL
cmake --preset vs2022-windows-x86 -B build_x86 -DBUILD_TESTING=OFF -DCMAKE_INSTALL_PREFIX=install/Release
cmake --build build_x86 --config Release --target INSTALL
```

The presets retain upstream's vcpkg registry baselines. Tests are disabled
because mods does not reproduce or rerun upstream's filesystem/injection
suite for this integration. See the evidence links in
`docs/research/issue-30-upstream-usvfs.md`.

A distributable adapter requires both `usvfs_x86.dll` and `usvfs_x64.dll`,
and both `usvfs_proxy_x86.exe` and `usvfs_proxy_x64.exe`, together in the
upstream-supported layout. Artifact hashes and dependency/runtime closure
must be recorded from a successful clean build before shipping. A source
pin alone is not binary provenance.

## Source and licenses

usvfs is GPL-3.0-or-later. Copies of its license and shipped upstream
third-party notices are in `licenses/usvfs/`. The unmodified submodule
retains upstream copyright notices. Binary distribution must include
corresponding source for this exact revision, the mods adapter source,
these build instructions, and the dependency source/notices required by
their licenses. Do not publish a binary-only package without its
corresponding-source arrangement.

## Build the Rust boundary and package

`native/build.ps1` wraps only the unchanged upstream configure/install commands.
For a Git checkout it checks the source revision, tracked changes and untracked
source files. The archive-only route below verifies the pinned source archive
without requiring usvfs Git metadata. Both routes write
`source-revision.txt`, and records all four SHA-256 hashes in `artifacts.json`.
Set `MODS_USVFS_ARTIFACTS` to its absolute `bin` output directory before Cargo.
Cargo rejects a different source revision and embeds hashes of this exact bundle.
Set `LIBCLANG_PATH` to the directory containing `libclang.dll`. Use the repository
Rust toolchain and install both `x86_64-pc-windows-msvc` and
`i686-pc-windows-msvc` targets.

The private header is C-compatible. Bindgen parses it and the pinned upstream
public header as C++ for the actual Windows target/SDK. It emits only the six
exception-boundary functions, their parameter types, and the two used link flags.
The shim's `decltype` function pointers preserve upstream parameter helpers'
`cdecl` convention and controller functions' `WINAPI` convention. On x86 the
loader uses the actual inspected stdcall-decorated exports; x64 names are plain.
No upstream C++ class or STL layout crosses into Rust.

Install the four native files under `usvfs/` beside `mods.exe`/`mods-mcp.exe`.
The loader never accepts an Environment Root, cwd, PATH, or caller-selected DLL
path. It checks every embedded artifact hash before loading the architecture's
controller with DLL-directory/System32-only search. The proxies remain beside
both DLLs, as upstream requires. Windows 11 and **both x86 and x64 Microsoft
Visual C++ 2015–2022 Redistributables** are runtime prerequisites. The observed
binary closure uses those release runtimes and Windows system DLLs; proxies also
import their matching usvfs DLL. No debug runtime DLL was in the import tables.

CI builds both upstream architectures without their semantic tests, checks x86
adapter bindings, and runs workspace checks on x64.

## Archive-only rebuild

The mods source tree need not be a Git checkout. Start with `native/usvfs` absent
or empty; an existing source tree is never overwritten or assumed verified.
`native/usvfs-source.json` pins the official upstream archive URL, exact revision
and SHA-256. All 278 files in this archive were compared byte-for-byte against the
pinned Git source. The wrapper validates the archive hash **before extraction**,
extracts with one root component removed, and runs the unchanged x86/x64 upstream
presets. A successful build emits `source-revision.txt` and an `artifacts.json`
containing all four binary SHA-256 values, exactly as the Git route does.

From the root of the extracted mods source, using an authorized PowerShell script
host and the build prerequisites described above:

```powershell
$manifest = Get-Content -Raw native/usvfs-source.json | ConvertFrom-Json
Invoke-WebRequest $manifest.url -OutFile "$pwd/usvfs-source.tar.gz"
$env:VCPKG_ROOT = "C:/path/to/vcpkg"
./native/build.ps1 -SourceArchive "$pwd/usvfs-source.tar.gz"
$env:MODS_USVFS_ARTIFACTS = "$pwd/native/artifacts/bin"
$env:LIBCLANG_PATH = "C:/path/to/LLVM/bin"
cargo build --locked --target x86_64-pc-windows-msvc --package mods --package mods-mcp
```

This route needs no usvfs `.git` directory. vcpkg and its registry fetches remain
build prerequisites. Rebuilding via the wrapper from an archive requires a fresh
empty source destination; the wrapper does not remove or repair partial work.
After a successful native build, ordinary Cargo rebuilds reuse its source and
artifact directory. Cargo tracks `source-revision.txt` and all four binary inputs
so a changed revision marker or bundle triggers revalidation.

## Binary publication gate

The combined mods project is **GPL-3.0-or-later**. See root `LICENSE` and
`COPYRIGHT.md`; all Cargo packages inherit the workspace license. Upstream and
third-party notices remain in place.

**Binary preview publishing is disabled** by a literal false gate in the GitHub
workflow. A mods source archive plus an upstream source archive does not supply
the full corresponding source for statically linked vcpkg dependencies.
`vcpkg.json` and registry references help identify dependencies, but are **not** a
claimed substitute for the required source distribution. Do not re-enable binary
publication until a complete corresponding-source arrangement, build inputs and
notices have been provided and reviewed. Dependency vendoring/source distribution
infrastructure remains a separate follow-up; this repair does not implement it.

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

New Rust tests are colocated in the configuration, process and wrapper modules. They
record mapping calls, validate Output Target policy, check bounded failures,
and load/release the exact compiled artifact set. Fake Job/session tests cover
retained ownership on early finish, explicit cleanup errors, confirmed drain and
bounded emergency cleanup/pinning. They do not launch an injected
process or reproduce upstream file-operation tests. The only C++ unit tests are
colocated at the bottom of `barrier.cpp` and replace launch/cleanup calls with
fakes. To run them in each architecture's VS Developer Command Prompt, from the
execution crate:

```text
cl /nologo /EHsc /std:c++20 /DMODS_BARRIER_UNIT_TEST /I../../../native/usvfs/include native/barrier.cpp /Fe:barrier-unit.exe
barrier-unit.exe
```

These check exception conversion, zero output on failed launch, exactly-once
handle cleanup and parameter/session release. Do not add an upstream injection
or filesystem test runner here.

## Local clean-build evidence

The clean source worktree was
`C:/Users/prime/source/usvfs/.worktrees/issue-30-upstream-clean`; `git status --short`
was empty after building. No artifacts from the custom baseline were used.
Both upstream Release `INSTALL` targets passed with tests disabled. Existing
upstream warnings (including x86 LNK4098/LNK4217) were not patched; `dumpbin
/DEPENDENTS` confirmed release runtime imports for all four outputs.

Tools: MSVC 19.44.35228.0 (VS2022 Community, tool directory 14.44.35207), Windows
SDK 10.0.26100.0, CMake 3.31.6, bindgen 0.72.1, cc 1.4.7,
Rust nightly-2026-08-21 (1.100.0-nightly, 8925ea358), and LLVM 20.1.8.
Rustup 1.29.1 and LLVM were installed under the build user's home after verifying
the official SHA-256 downloads. No global/default Rust toolchain is selected;
the repository toolchain and per-process environment select the tools.

Local artifact SHA-256 values (build evidence, not universal reproducible hashes):

| Artifact | SHA-256 |
| --- | --- |
| `usvfs_x86.dll` | `78d93fde38b2db16ec1c6d309144117a3520ff98d507a78db4afb203431a6ff5` |
| `usvfs_x64.dll` | `b67c4d1ad28079303faa722bc1e5a16009bc41070d52f2880ca598ae76534349` |
| `usvfs_proxy_x86.exe` | `860d0a28cde00d80d877088d99a5c8b92abed6f51960aa5f010ff857343889d6` |
| `usvfs_proxy_x64.exe` | `71e493a1a58243df1dc7e7448e3e7496017fdbfdf4ce3184af4da322b25f561b` |

Local builds generate and embed their own artifact manifest from the same source
pin. The GitHub workflow has not been run for this uncommitted work, and binary
publication is disabled. Local checks are not evidence of a published preview,
a complete corresponding-source distribution, or a shipped game launch.

### Archive-only validation during the bounded repair

The pinned archive checksum was verified, and its 278 source files matched the
Git pin byte-for-byte. Both unchanged upstream Release `INSTALL` builds also
passed from `C:/Users/prime/source/mods-archive-build-issue30/native/usvfs`, which
has no `.git` metadata. The resulting bundle has a revision marker and all four
SHA-256 entries; x86 and x64 adapter/load tests passed against that bundle.

The host's default PowerShell policy blocked direct `.ps1` execution. No policy
was changed or bypassed. The final wrapper passed PowerShell syntax parsing;
archive verification/extraction, the same upstream build commands, and manifest
creation were exercised as authorized individual commands instead. Thus the
archive inputs/build outputs were checked, but the wrapper entrypoint itself was
not executed end-to-end on this restricted host. The GitHub workflow was not run.
