# Upstream usvfs and rust-bindgen for issue #30

## Decision recorded after evaluation

The owner accepted the upstream limitations and approved replacing the custom-fork direction. Issues #30 and #31 now carry the replacement contract; ADR-0005 records the accepted decision. The evaluation below preserves the evidence and comparison against the former requirements. Implementation has not started, and existing experimental work has not been reverted.

## Question and conclusion

Can unmodified upstream usvfs replace the modified-usvfs design in [#30][30]?

**Not under the current acceptance contract.** It can supply a substantially smaller, conventional Virtual Game View adapter. Its public C-linkage API supports overlays, creation targets, ordinary mapped-file writes, and managed process launch. It does **not** supply Steam copy-on-write, persistent provider-owned Tombstones, Rust-owned transactional mutations, or the required fail-closed descendant guarantees. Bindgen removes handwritten declarations; it does not add those semantics. The existing upstream tests explicitly document several of these differences. [API][api] [Legacy behavior tests][basic] [Descendant implementation][kernel]

**Recommendation:** consider upstream plus generated bindings only after an explicit product decision revises those requirements. Do not silently call it a compliant implementation of #30. If clean Steam protection, persistent Tombstones, and fail-closed descendants remain mandatory, reject this alternative for this ticket. Do not replace the missing features with another broad broker or hidden fork during integration.

This is evaluation only. No builds, tests, Windows probes, dependencies, or native/Rust changes were made. Existing work was not reverted. Implementation needs owner approval.

## Evidence boundary and versions

- Investigated upstream commit: **`57f1ea5e6ad13f7435a7af184748e6c1312c5637`**, obtained through `git show` in `/private/tmp/usvfs-fork`. GitHub's master endpoint returned this same commit, dated **2026-04-15**. Local custom commits `2d3238f` and `3451b1f` are not evidence of upstream behavior. [Pinned tree][tree] [Master API][master]
- Latest published release returned by GitHub: **v0.5.7.2**, published **2025-06-01**, asset `usvfs_v0.5.7.2.7z`, tag commit **`a50d84c64c9244f80dc67e9fe7af209bfe514d5b`**. The investigated master still declares version 0.5.7.2; a version string is therefore not a sufficient source identity. Release binaries were not inspected and must not be assumed identical to master. [Release][release] [Version header][version]
- The README calls usvfs alpha and a core Mod Organizer 2 component with real-world use. The repository has x86/x64 build and test CI. This is evidence of maintained infrastructure, not a warranty of complete Windows interception or current CI success. [README][readme] [Workflow][ci]
- Requirements were read from current issue bodies and comments for [#30][30], [#25][25], and [#31][31]. Terminology follows [CONTEXT.md](../../CONTEXT.md). #25 supersedes #31's stale instruction to settle pending work: MVP operations do not recover or settle automatically. No VFS-specific ADR exists in the inspected `docs/adr/` inventory.

## Requirement mapping

**API** = supported public API/configuration. **Wrapper** = mods-owned translation or lifecycle. **Revise** = change/defer the product requirement; the public API cannot implement it. **Uncertain** = decision gate, not an asserted guarantee.

| Essential requirement | Classification | Finding |
| --- | --- | --- |
| Steam + enabled Data Mods + Overwrite precedence | API + Wrapper | File links and recursive static directory links support overlays. mods selects enabled providers, validates paths, and computes effective winners in Mod Priority order with Overwrite highest. Prefer mapping already-resolved winners when precedence is nontrivial; do not ask bindgen to define policy. Existing linked-file replacement and layered mapping behavior are upstream-owned. [api][api] [implementation][impl] [fixture][fixture] |
| New Data files and directories use Output Target | API + Wrapper | `LINKFLAG_CREATETARGET` sends creation to a source directory. One target per destination; a deeper target overrides an ancestor. mods validates the selected enabled Data Mod or default Overwrite. Configure the create target separately from read winners, without recursively relinking a low-priority selected target last and thereby promoting its files. [api][api] [implementation][impl] |
| Writes to existing mod files | API | Ordinary mapped opens modify the backing provider. The legacy tests assert this behavior; selecting an Output Target does not turn all writes into output-only writes. [basic][basic] |
| Writable Steam open leaves Steam unchanged through bounded COW | Revise | Existing physical files can be modified in place. The legacy test's `copy_on_readwrite_implemented = false` branch explicitly expects base-file changes. No public COW switch or Rust mutation callback exists. Reject the claim that create-target protects Steam. Pre-copying the installation or adding a separate mutation engine would be a different architecture, not a small wrapper. [basic][basic] [api][api] |
| Copy/file-move destinations always use selected Output Target | API for new destinations; Revise for blanket rule | Copy/move hooks use destination rerouting, but an existing mapped destination can be replaced in its current provider. The legacy move-over test expects exactly that. A move may remove its physical source. Preserve the new-destination rule, or explicitly revise the stronger Output Target definition in CONTEXT.md. [tracker][tracker] [kernel][kernel] [basic][basic] |
| Delete preserves lower providers and records durable Tombstones | Revise | Delete calls the physical delete on the rerouted or real path and removes mappings. Upstream tests expect physical provider/base deletion. Internal deleted-path tracking is not mods' persistent provider-owned metadata. There is no public whiteout API; README's virtual-unlink statement is expressly a final-goal description, not a delivered contract. [kernel][kernel] [basic][basic] [api][api] [readme][readme] |
| Initial provider-owned Tombstones and restart precedence | Wrapper partly; Revise overall | mods can omit suppressed mod links when computing winners, but omission cannot hide an existing physical Steam file in the destination namespace. No exposed inclusive-subtree whiteout operation fills this gap. Persistent metadata may remain useful to conflict analysis, but claiming full execution parity would be incorrect. [api][api] [impl][impl] |
| Named INI/plugin files and private saves | API + Wrapper | File links can point named paths to canonical Profile State. A dedicated virtual save path can have a create target. mods owns the list of paths, `SLocalSavePath=__mods_saves\`, activation/load-order policy, and generated invalidation files. [api][api] [31][31] |
| Hide unrelated real user files and real save directories | Revise / Uncertain | A directory overlay is not an opaque replacement: physical entries can remain visible. Neither skipping source links nor linking an empty directory proves hiding. Exact FNV path isolation requires a separate approved routing design; the API offers no general hide-path operation. [api][api] [tvfs][tvfs] [31][31] |
| Synthetic timestamps; reject direct timestamp writes | Revise for exact contract | mods can prepare physical metadata, but that is not a virtual timestamp policy or a write-rejection hook. No public policy callback exists. Do not conflate generated Profile State with enforcement inside applications. [api][api] [31][31] |
| `.mohidden` visibility; no blacklist/suffix exception | API + Wrapper | Shared blacklist and skip collections start empty. Public clear functions exist for executable blacklist, suffixes, and directories. No `.mohidden` literal occurs in the inspected upstream `src/` or `include/`; skipping is configurable, not a hard-coded `.mohidden` exception there. Clear lists at session setup and do not populate them. This is configuration policy, not a tamper-proof guarantee against arbitrary in-process callers. [shared][shared] [impl][impl] |
| Managed root launch and Job ownership | API + Wrapper; order revision | `usvfsCreateProcessHooked` can return the root suspended. mods can then assign its Job before resuming and own handle cleanup/status. However injection happens inside that API before return; the exact #30 order “Job assignment, then injection” is not exposed by the controller API. No public inject-an-already-created-root function exists. [api][api] [impl][impl] |
| Descendants inherit the view | API in ordinary operation; Revise fail-closed guarantee | The process hook attempts descendant injection. On an injection exception it logs and then can resume the child. This directly contradicts mandatory status 125 plus whole-Job termination. A Job or controller poll cannot prove interception occurred before the child ran. [kernel][kernel] |
| Exact root handshake / hook proof / no physical fallback | Revise | No such controller handshake exists. `InitHooks` catches initialization exceptions and logs them; some native file hooks have physical fallback paths. Cross-bitness helper waiting does not itself validate proxy exit status. A successful launch return is not the exact required proof. [impl][impl] [ntdll][ntdll] [inject][inject] |
| Serialized mutation broker, two-phase open, durable pending evidence | Revise | No public request/completion interface exists. Native hooks perform ordinary filesystem work. There is no mods-controlled staging/publication boundary for these writes. Drop MDPP/MDMW, request-slot, replay/deadline, and synchronous completion-effect requirements if adopting stock behavior. [api][api] [kernel][kernel] |
| Mandatory rejection of links, mappings/sections, inherited/remote handles, delete-pending and unsupported information classes | Revise | The public configuration provides no comprehensive deny-policy interface. No claim of pre-mutation rejection or fail-closed security isolation is justified. This does not mean all such Windows operations work; it means the required rejection contract is unavailable. [api][api] |
| Argument/cwd handling, streams, cancellation, Job drain, root status and 125/126/127 mapping | Wrapper | These remain mods/#31 responsibilities. Preserve them where compatible with the revised launch boundary; do not use upstream's process list as a replacement for owned Job supervision. The wrapper cannot turn unreported native descendant failures into the old guarantee. [31][31] [api][api] |

## Smallest architecture and bindgen boundary

The candidate is **existing application ports → narrow execution adapter → generated upstream bindings → pinned upstream DLLs/proxies**. Keep winner selection and Profile State configuration in Rust. Keep ordinary filesystem interception in upstream. Use no custom mutation broker, MDPP encoder, MDMW transport, health handshake, hook manifest, or mutation worker in this alternative. One upstream controller connection per controller process is a documented constraint; do not expose unconstrained concurrent sessions from MCP. [api][api]

The existing `extern "C"` exports are enough for basic configuration, mapping, and launch. They are declared in C++ headers (scoped enums, default arguments, `<chrono>`); parse them as C++, but allowlist only the needed C-linkage functions/constants and use opaque `usvfsParameters` with its create/set/free functions. Binding upstream C++ implementation classes or STL containers is unnecessary. Bindgen documents incomplete STL support, nontrivial C++ ABI limitations, and unsupported exceptions. [parameters][parameters] [Bindgen C++][bcpp] [Bindgen FAQ][bfaq]

Generate against the pinned headers and actual Windows target/SDK. Preserve mixed conventions: controller operations use `WINAPI`; parameter helpers use their declared default convention. x86 calling convention, pointer size, `size_t`, Windows structs, enums and layouts must not be guessed from the host or copied from x64 bindings. Bindgen supports passing Clang a target; it does not produce DLLs, select the correct binary, or prove semantic compatibility. Linking/import-library selection or dynamic symbol loading is still build/adapter work. Generated layout checks protect this ABI seam, not filesystem behavior. [api][api] [parameters][parameters] [Bindgen FAQ][bfaq] [Bindgen tutorial][btutorial]

A tiny safe wrapper should own:

- checked, NUL-terminated Windows strings and call-scoped buffers;
- parameters freed by `usvfsFreeParameters`, session disconnect, and Windows process/thread handles exactly once;
- DLL lifetime longer than every function pointer/session, with no reconnect while a managed execution is active;
- checked BOOL/null results and immediate preservation of relevant native error information;
- trusted absolute artifact paths and restricted DLL search, never loading from an Environment Root, Data Mod, cwd, or arbitrary PATH;
- session shutdown only after Job drain. Avoid the allocated process-list API and its cross-runtime `free` obligation when Job ownership already supplies lifecycle.

These are proposed mods-owned invariants, not upstream promises. The parameter allocator uses `new (std::nothrow)`, but **C linkage alone does not guarantee every exported call is exception-safe**. Audit the narrow chosen calls before implementation sign-off. If an uncaught C++ exception can cross a retained export, Rust cannot repair that with bindgen or `catch_unwind`; a narrowly scoped exception barrier or upstream correction is a decision gate, not permission for a semantic fork. [parameter implementation][paramimpl] [api][api] [Bindgen C++][bcpp]

## Artifacts, maintenance and source distribution

Ship the matching architecture controller DLL and all supported target architectures' upstream artifacts. For the intended mixed-bitness game/tool tree, retain **`usvfs_x86.dll`, `usvfs_x64.dll`, `usvfs_proxy_x86.exe`, and `usvfs_proxy_x64.exe`** together in the upstream-supported layout. Do not assume an x64 Rust launcher eliminates x86 artifacts. Dependency/runtime closure and actual export names are packaging gates; no binary inspection was performed. Upstream already has CMake install targets and both-architecture CI: reuse them rather than invent a native build system. [inject][inject] [DLL CMake][dllcmake] [workflow][ci]

Pin the full source revision plus artifact hashes; do not identify the build solely by `usvfsVersionString`. An unmodified source submodule or verified source archive can replace the proposed public fork pin. Choose master or release explicitly; do not pair master-generated bindings with an unverified older release asset. [version][version] [release][release]

Headers license usvfs **GPL-3.0-or-later**. Preserve its license, copyright notices, and dependency notices in `licenses/`; provide compliant corresponding source and build instructions for shipped binaries, including the exact upstream revision and packaging changes. Upstream packaging existing install targets is useful but not proof that mods' distribution is compliant. Binding generation does not remove licensing obligations; confirm the combined product's licensing before shipment. [license][license] [api][api] [notices][notices] [ci][ci]

## Testing: reuse upstream evidence, do not duplicate it

**No tests were authored or run for this evaluation. Do not test behavior that upstream already tests.** Read/reference the existing suite at the pinned revision; consume upstream evidence or its unchanged tests where release policy requires dependency validation. Do not translate them into Rust or create a second injection harness. The upstream workflow already runs its suites for x86/x64 and Debug/Release. [ci][ci]

| Existing upstream coverage | Source to reuse/reference, not reproduce |
| --- | --- |
| Directory-tree lookup, wildcard matching and shared-memory allocation | `test/shared_test/main.cpp` ([source][sharedtest]) |
| Hooking mechanics and initialization/injection cases | `test/thooklib_test/`, `test/tinjectlib_test/main.cpp`, `test/tinjectlib_test/testinject_*` ([hook tests][hooktest], [injection tests][injecttest]) |
| Missing-file errors, mapped opens/attributes, directory enumeration, object queries, multiple links, redirection-tree resize | `test/tvfs_test/main.cpp` ([source][tvfs]) |
| Layered mappings, nested creation, writes, copy/move/delete and physical postmortem state, including documented absence of COW | `test/usvfs_test_runner/usvfs_test/usvfs_basic_test.cpp`, its base runner, and `test/fixtures/usvfs_test/basic/` including cross-bitness logs ([scenario][basic], [fixtures][legacyfixtures]) |
| File visibility/deletion and handle-path queries (`BasicTest`), recursive enumeration (`BoostFilesystemTest`), canonicalization/new output (`RedFileSystemTest`), configured suffix skipping (`SkipFilesTest`) | `test/usvfs_global_test_runner/usvfs_global_test/usvfs_global_test.cpp`, fixture mapper and `test/fixtures/usvfs_global_test/` ([tests][globaltest], [mapper][fixture]) |

Minimal **new mods-only** checks, if this alternative is approved:

1. Configuration translation: enabled providers and winners, unchanged Mod Priority when selecting Output Target, intended flags and paths, explicit empty bypass lists, and Profile State/save routing decisions. Use a fake call recorder; do not test that upstream links files correctly again.
2. Wrapper ownership/error propagation: invalid input rejection, exactly-once release, no use after unload/disconnect, no resume after a mods-owned setup failure, and retained native error causes. Keep these colocated unit tests under #25/CODING_STYLE.md.
3. Packaging/loading seam: exact pinned artifact identity, expected architecture/artifact closure, trusted load paths and required exports. Check generated target ABI/layout only as a mods binding seam, not a reimplementation of Windows or bindgen tests.
4. Reuse existing mods tests for argument handling, Job lifecycle, cancellation and status mapping rather than duplicating them in a new native suite. Drop tests for abandoned MDPP/handshake/mutation semantics instead of keeping dormant protocols solely to satisfy them.

No Windows smoke probe is needed to establish the documented incompatibilities. If approval later requires a packaging smoke check, first state the uncovered question: **“Does the packaged mods adapter, using its generated bindings and selected upstream artifact set, configure the chosen Output Target without changing mods-computed winners and retain its Job/session until completion?”** Use an existing upstream fixture if it can answer that wiring question. Do not expand this into testing ordinary creation/deletion/injection again. Likewise, no probe can make the source-visible descendant failure behavior satisfy #30.

## Decision and change boundaries

**Preserve:** domain language; Mod Priority and effective winner computation; Output Target selection validation; Profile State ownership and generated inputs; environment/path validation; no shell or elevation; child stream/status behavior and Job lifetime; cancellation without restoration; GPL/source pinning; existing business-policy tests. Existing process/Job/value-type code may be reusable, but its launch ordering must be reviewed, not assumed compatible. [25][25] [30][30] [31][31]

**Drop from #30 if approved:** modified-usvfs-only pin and six-export ABI; exact 98-byte handshake, 50-hook manifest and capability mask; MDPP/MDMW wire requirements; serialized mutation slot; Rust two-phase open/publication; native synchronous completion effects; related malformed-frame/replay/deadline/staging-interruption tests. Remove rather than park their controller/protocol/mutation-worker paths. Existing filesystem publication helpers should remain only where a different retained owned operation needs them, not as a dormant execution broker. The read-only Rust worktree inspection found `provider_plan.rs` replacing `host.rs`, so disposition must follow responsibilities rather than blindly following #30's old filenames. [30][30]

**Revise/defer explicitly:** Steam COW and unchanged physical base on deletes/moves; durable execution Tombstones; mandatory destination routing over existing winners; transactional pending evidence for application writes; blanket pre-mutation unsupported-operation rejection; exact Job-before-injection order; fail-closed descendant/root hook proof; opaque hiding of real user files; virtual timestamps and timestamp-write rejection. These touch #31 and CONTEXT.md, not only #30's implementation details. Clearing skip/blacklist configuration can replace the old fork changes for ordinary `.mohidden` visibility, but not the broader “no unvirtualized fallback” claim. [30][30] [31][31] [api][api] [basic][basic]

**Scope estimate, not hours:** one binding-generation/artifact boundary, one small safe execution adapter, retained configuration projection and process supervision, removal of fork-specific protocol/mutation infrastructure, and explicit issue/domain-contract revisions. No new backend, broad rewrite, broker, or fork is proposed.

**Go/no-go gates:** owner acceptance of mutation/isolation limitations; exact Profile State hiding/timestamp disposition; accepted descendant failure behavior; narrow-export exception/ABI review; selected source-to-binary provenance and GPL packaging. If any original safety guarantee remains non-negotiable, the unmodified candidate is a no-go. Stop here until the owner decides.


## Sources

[api]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h
[parameters]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfsparameters.h
[paramimpl]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/usvfsparameters.cpp
[readme]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/README.md
[version]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs_version.h
[impl]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/usvfs.cpp
[kernel]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/hooks/kernel32.cpp
[ntdll]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/hooks/ntdll.cpp
[tracker]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/maptracker.h
[shared]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/sharedparameters.cpp
[inject]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_helper/inject.cpp
[ci]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/.github/workflows/build.yml
[dllcmake]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/CMakeLists.txt
[basic]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/usvfs_test_runner/usvfs_test/usvfs_basic_test.cpp
[fixture]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/usvfs_global_test_runner/usvfs_global_test_fixture.cpp
[globaltest]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/usvfs_global_test_runner/usvfs_global_test/usvfs_global_test.cpp
[tvfs]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/tvfs_test/main.cpp
[sharedtest]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/shared_test/main.cpp
[injecttest]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/tinjectlib_test/main.cpp
[license]: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/LICENSE
[30]: https://github.com/Reilley64/mods/issues/30
[25]: https://github.com/Reilley64/mods/issues/25
[31]: https://github.com/Reilley64/mods/issues/31
[tree]: https://github.com/ModOrganizer2/usvfs/tree/57f1ea5e6ad13f7435a7af184748e6c1312c5637
[master]: https://api.github.com/repos/ModOrganizer2/usvfs/commits/master
[release]: https://github.com/ModOrganizer2/usvfs/releases/tag/v0.5.7.2
[notices]: https://github.com/ModOrganizer2/usvfs/tree/57f1ea5e6ad13f7435a7af184748e6c1312c5637/licenses
[legacyfixtures]: https://github.com/ModOrganizer2/usvfs/tree/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/fixtures/usvfs_test/basic
[hooktest]: https://github.com/ModOrganizer2/usvfs/tree/57f1ea5e6ad13f7435a7af184748e6c1312c5637/test/thooklib_test
[bcpp]: https://rust-lang.github.io/rust-bindgen/cpp.html
[bfaq]: https://rust-lang.github.io/rust-bindgen/faq.html
[btutorial]: https://rust-lang.github.io/rust-bindgen/tutorial-4.html
