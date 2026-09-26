# MO2 integration comparison (#33)

Read-only source comparison of MO2 v2.5.2 and development revision `efe2a02d5dc641946baaa8db1440800f38d07837` against mods at `bc0c7e6`. No production code, native code, generated bindings, release pin, or runtime state changed.

## Decision

No documented initialization or mapping API violation was established. This is not a correctness proof or a crash diagnosis. The material difference is view construction: MO2's actual launch path supplies ordered recursive directory overlays, whereas mods supplies resolved winning file links with nonrecursive directory containers and an independent Output Target. MO2 success does not validate that different construction. Conversely, the observed native crash alone does not establish a native-library defect.

Other differences include explicit native logging, parameter defaults, skip/blacklist policy, and long-lived versus per-execution controller/DLL ownership. Neither universal separator normalization nor a prohibition on identity mappings is established by MO2's actual launch path. Do not change paths, omit mappings, or copy recursive overlay policy without establishing compatibility with the Mod Environment's required Virtual Game View.

Recommended next step: review the project-owned view-construction requirements and their compatibility with upstream-supported mapping patterns. Treat any proposed mapping-model change as an explicit scope decision, not a proved crash fix. No runtime fault replay, speculative patch or library downgrade is justified by this comparison alone.

Version provenance is recorded in [issue-33-mo2-usvfs-version.md](issue-33-mo2-usvfs-version.md). Detailed source-linked reviews follow.

## Initialization and lifecycle

Read-only static review. No execution, debugger, fault reproduction, memory tracing, native edits, pin changes, or speculative fixes. Local worktree: `/Users/reilley/Repositories/mods/.worktrees/issue-33-distribution`. Read `CONTEXT.md` and the issue-33 provenance report. Local references below are relative to that worktree. Raw Rust bindings are generated; this comparison concerns wrapper call policy, not handwritten replacement bindings.

## Exact comparison identities

- Official stable MO2 **v2.5.2** (tag), using its documented usvfs v0.5.0 dependency; not the mods upstream revision.
- Official development MO2 **efe2a02d5dc641946baaa8db1440800f38d07837**. Its development dependency selection is not proof of any downloaded binary's identity.
- mods fork **eb4949fb2439fe5b98901e2fb1afceee752a6133**, based on upstream **57f1ea5e6ad13f7435a7af184748e6c1312c5637**. The latter is the API/source reference below.
- `src/usvfsconnector.cpp` is identical between the two MO2 refs except line 33: stable includes `<usvfs.h>`, development includes `<usvfs/usvfs.h>`. All connector line citations apply to both refs.

## Findings

### Connection and parameters

Both MO2 refs allocate opaque parameters through `usvfsCreateParameters`, set instance `mod_organizer_instance`, debug=false, configured log level, dump type/path and spawn delay, initialize logging, call `usvfsCreateVFS`, then immediately free parameters. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L124-L155) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L124-L155)

mods instead loads the packaged DLL with a constrained `LoadLibraryExW`, resolves typed entry points, allocates opaque parameters, sets only the instance name, and calls the same `usvfsCreateVFS`. It checks null allocation and the create result, and retains parameters until disconnect. [Pinned shim](https://github.com/Reilley64/usvfs-rs/blob/eb4949fb2439fe5b98901e2fb1afceee752a6133/rust/usvfs-sys/native/barrier.cpp#L23-L99). Rust supplies `mods-<controller PID>` and gates entry to one session (`src/infrastructure/execution/src/usvfs/mod.rs:75–118`). Neither constructs the deprecated public structure by hand.

The omitted parameter setters do **not** imply uninitialized values: pinned upstream's constructor supplies debug=false, Debug log level, no dumps, empty dump path, zero delay, and zeroed names. [Constructor](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/usvfsparameters.cpp#L4-L12). Parameter ownership uses the supported create/free API ([header](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfsparameters.h#L38-L64)). Different instance names and settings are **application policy**, not established API violations. Retaining parameters longer than MO2 does is not a documented violation.

The public contract allows only one controller connection and says CreateVFS resets the VFS before use. [Contract](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L88-L104). mods' atomic gate plus non-Send/non-Sync owner follows that restriction more explicitly than the reviewed MO2 connector code. This does not prove all upstream state is safe to unload/reload.

### Logging and configuration

MO2 explicitly calls `usvfsInitLogging(false)` before CreateVFS and starts a separate log-reader worker. Its worker consumes nonblocking messages and writes a file. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L67-L89) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L67-L89); [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L136-L185) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L136-L185).

The pinned mods shim neither resolves nor calls `usvfsInitLogging`/`usvfsGetLogMessages` (complete resolved table at [lines 71–81](https://github.com/Reilley64/usvfs-rs/blob/eb4949fb2439fe5b98901e2fb1afceee752a6133/rust/usvfs-sys/native/barrier.cpp#L71-L81)). This is a confirmed integration difference, **not a proven documented prerequisite violation**: the public header declares logging initialization without a must-call-first condition, and the pinned ConnectVFS/DisconnectVFS implementation installs a null logger when needed. [Header](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L207-L225); [implementation](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/usvfs.cpp#L448-L490). This is not a blanket proof that every logging path tolerates omission; no runtime conclusion follows.

MO2 after CreateVFS clears and repopulates executable blacklist, clears and repopulates skipped suffixes, clears and repopulates skipped directories, then clears force-load libraries. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L157-L178) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L157-L178). Before each mapping update it clears virtual mappings; cancellation clears them again. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L196-L232) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L196-L232). Forced libraries are reset/repopulated separately. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L285-L295) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L285-L295).

mods after CreateVFS clears blacklist, suffix list, directory list in that same relative order, and deliberately leaves them empty; then applies the resolved mods namespace. It does not explicitly clear mappings or force-load libraries. [Shim clear sequence](https://github.com/Reilley64/usvfs-rs/blob/eb4949fb2439fe5b98901e2fb1afceee752a6133/rust/usvfs-sys/native/barrier.cpp#L103-L110); `configuration.rs:145–156` and `usvfs/mod.rs:60–72,196–203` under `src/infrastructure/execution/src/`.

Classification: list contents are **application policy** (mods owns winner/tombstone rules and must not accidentally import MO2 hiding conventions). No public rule requires repopulating any list. No public rule found requires these clears in a particular order. Missing explicit mapping clear is **not by itself a violation**, because mods creates a fresh/reset VFS for each view, unlike MO2's reused connector. The reset guarantee alone should not be overread as a complete proof about every auxiliary list/force-load state; that specific residual-state behavior is **not established by this comparison**. [List semantics](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L156-L205).

### Owner, threads, and child lifetime

MO2 stores one connector as an OrganizerCore member: [stable](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.h#L578-L578), [development](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/organizercore.h#L593-L593). Mapping and forced-library updates are sequential in launch preparation: [stable](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L1977-L1979), [development](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/organizercore.cpp#L2027-L2029). This is an application-owned connector, **not evidence of a global single-native-thread contract**: log reads explicitly run on another thread, mapping updates process Qt events every ten entries, and no mutex/thread-identity enforcement appears in this connector. No claim of complete MO2 reentrancy safety is made.

mods uses a process-wide atomic session gate and `PhantomData<Rc<()>>` to prevent moving/sharing the view across threads, plus exclusive mutable configuration and launch (`usvfs/mod.rs:41–52,93–118,120–164`). This is defensive serialization policy consistent with one connected VFS. The reviewed upstream public header does not state that one fixed OS thread is mandatory for all controller operations.

MO2's connector destructor disconnects **before** asking its log worker to stop, then quits/joins that worker. It does not free/unload a dynamically owned DLL in that destructor. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L188-L194) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L188-L194). Its process wait logic uses Jobs to monitor children, but can fall back to monitoring only the first root when assignment fails; it also supports force-unlock rather than requiring child termination. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/processrunner.cpp#L355-L424) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/processrunner.cpp#L355-L424); [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/processrunner.cpp#L242-L255) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/processrunner.cpp#L242-L255). Its all-usvfs wait loops enumerate connected processes, exclude the controller, and wait again; locking can be disabled. [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L297-L341) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L297-L341); [stable v2.5.2](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/processrunner.cpp#L950-L976) / [development efe2a02](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/processrunner.cpp#L950-L976). Therefore MO2 is not evidence that every controller teardown waits for every child, nor that a Job is an upstream API prerequisite.

mods launches the root suspended, assigns it to a kill-on-close Job before resume, and retains the VirtualGameView until normal Job drain (`process.rs:68–146,229–230`, `managed.rs:51–109`). Its emergency Drop bounds cleanup and retains/leaks the view if drain cannot be established (`process.rs:286–327`). The shim disconnects, frees parameters, then FreeLibrary; failed disconnect prevents unload, while the Rust session gate remains poisoned ([shim](https://github.com/Reilley64/usvfs-rs/blob/eb4949fb2439fe5b98901e2fb1afceee752a6133/rust/usvfs-sys/native/barrier.cpp#L199-L213), `usvfs/mod.rs:175–193`). This per-execution load/unload and stricter child-retention policy differs materially from MO2's long-lived connector. Whether repeated successful disconnect/unload/reload covers all upstream process-global resources remains **unknown**, not validated by MO2's behavior. No native-library defect is inferred.

## Bounded conclusion

No documented upstream initialization/configuration requirement violation was established in these reviewed paths. Confirmed differences are logging setup/consumption, explicit versus default parameter settings, list policy, long-lived versus per-execution controller/DLL ownership, and child supervision policy. Absence of a demonstrated violation is not a correctness proof, ABI attestation, or fault explanation. A crash location cannot identify which integration layer caused the problem.

**Recommended next step:** include these classified differences and exact refs in the parent static contract review. No native modifications, runtime investigation, or pin changes are recommended by this report.


## Mapping callers

## Conclusion and scope

**No documented invalid API usage is established by this comparison.** There are material caller-policy differences. MO2's success is not proof that mods' different view construction is correct, nor evidence that these differences cause a native fault. This review read source only. It performed no builds, native execution, debugger work, tracing, reproduction, normalization experiments, or repository edits.

Compared official MO2 `v2.5.2` and development snapshot `efe2a02d5dc641946baaa8db1440800f38d07837`, through `OrganizerCore::fileMapping` into `UsvfsConnector::updateMapping`. Read local `CONTEXT.md`, the #33 provenance report, configuration/profile construction and Rust boundary, and pinned fork shim. Stable and development are different usvfs provenance cases as recorded in the existing report; this is not a binary-equivalence claim.

## Caller construction and connector boundary

| Topic | MO2 stable / development | mods worktree | Assessment |
|---|---|---|---|
| Data files versus directories | Active regular mods are directory mappings. Stable maps each mod root to primary/secondary Data directories [S1]. Development maps existing per-game `getModMappings()` subdirectories to their destination directories [D1]. Both connector versions send **every directory mapping** with `RECURSIVE`, optionally `CREATETARGET`; file mappings get flags `0` [S2,D2]. | Already-resolved winning files become individual file links. Parent directories are explicit nonrecursive links. Selected Data Output Target is a nonrecursive directory link [L1,L3]. | Intentional different abstraction: MO2 supplies ordered directory overlays; mods owns effective winners and tombstone policy. The header exposes recursion as a flag, not a caller obligation [U1]. Do not replace mods' winner policy with recursive overlays merely to resemble MO2. |
| Mapping order | Enabled mods in ascending priority-map order [S3,D3]; local-save mappings; Overwrite; enabled file-mapper plugin mappings [S1,D1]. Connector preserves vector order [S2,D2]. | Clear bypass lists; Data creation target; Data parent directories and profile containers; winning Data and profile file mappings; recursive save target [L1]. | Supported policy distinction. mods applies explicit read winners after its nonrecursive output target so target choice does not change priority. |
| Output target | A selected enabled mod receives `CREATETARGET` in its ordinary priority position. Overwrite still maps last, but is creation target only when no custom target was selected. Invalid disabled custom target is rejected [S1,D1]. | Enabled selected Data Mod, otherwise Overwrite, supplies Data creation target. Overwrite keeps its independent highest winner rank [L1, local CONTEXT.md]. | Same broad separation of read priority and new-file routing, implemented differently. Header documents creation including copy/move destinations and replacement of a previous target [U1]. |
| Identity mappings | Main Data construction does not explicitly filter equality. Connector does not filter equality either [S1,D1,S2,D2]. A **different overload** explicitly skips identity *file* mappings [S4,D4]. | No winner source/destination inequality filter; base winners can be identity file mappings. Profile construction deliberately emits identity directory containers [L1,L2]. | Do not cite the alternate overload's inequality test as a universal MO2/API restriction. No identity prohibition appears in the inspected public header [U1]. This does not establish runtime equivalence or guarantee all identity cases work. |
| Path conversion | Stable explicitly uses `QDir::toNativeSeparators` for primary Data, ordinary mod roots and Overwrite; secondary Data paths use `absolutePath`. Development uses `QDir::absolutePath/absoluteFilePath` without that explicit conversion in the main builder. Connector only converts QString to wide strings; it does **not** normalize separators [S1,D1,S2,D2]. Stable Fallout NV file mapper constructs paths using `/` [G1]. | Absolute/NUL checks, then `OsStr::encode_wide`, reject empty/NUL, append terminator. No separator or path-prefix rewriting [L1,L3]. Shim passes source/destination straight through [F1]. | An explicit conversion difference is not proof of invalid usage. In particular, saying “MO2 always normalizes to backslashes at the boundary” is false. No proposed speculative normalization changes. |
| Profile/save mapping | Core optionally appends game-feature save mappings, then appends enabled mapper-plugin results. The stable Fallout NV mapper emits individual `plugins.txt` and `loadorder.txt` files, including an Epic variant destination [G1]. Stable Gamebryo save feature emits directory+creation-target from profile saves to My Games `__MO_Saves` [G2]; connector makes it recursive. | Eight named Profile State files, with five INI files to Documents and three state files to Local App Data; identity directory containers; saves to `__mods_saves`, recursive+creation-target [L2,L3]. | Product policy differs. mods always isolates its one Environment's state; MO2 supports toggled local saves/settings and game/plugin-supplied mappings. Stable plugin detail is pinned separately from core. Development core delegates to plugins; its exact plugin snapshot is not established by the core commit alone. |
| Error boundary | Both shown MO2 connector versions call mapping APIs without checking the returned BOOL [S2,D2]. | Shim checks BOOL, contains exceptions, returns status; Rust stops on failure [F1,L1,L3]. | Defensive reporting difference, not evidence that either call set violates the API. |

## Important qualification: the alternate file-mapping overload

Both core versions also contain a recursive `fileMapping(dataPath, relPath, base, directoryEntry, createDestination)` overload [S4,D4]. It skips archive/base-origin files and identity file links, creates directory mappings, and recursively descends. **The examined launch caller instead calls `fileMapping(profileName, customOverwrite)`** [S5,D5]. That main builder does not call this alternate overload. Its identity skip therefore cannot be promoted to a requirement of the actual launch caller or usvfs contract.

## Contract versus product policy

The pinned public header explicitly says a file destination's containing directory must exist at least virtually. mods deliberately emits containers before files [L1,L2,U1]. This is consistent with the documented requirement; this static comparison does not prove every possible input satisfies all native preconditions. The header describes `RECURSIVE`, `CREATETARGET`, and zero-flag file links; it does not document a required MO2-specific map order, prohibition of identity links, or mandatory `toNativeSeparators` conversion [U1].

The local profile comment makes a stronger statement about directory sources not needing to exist. Treat that as a local implementation rationale, not as an independently verified public-header guarantee in this report. Likewise, the plain directory API prose says it recursively links files, while its explicit `RECURSIVE` flag is the documented control. This report does not turn that prose ambiguity into a finding of misuse.

No required Data winner, profile file, save mapping, or output mapping should be silently omitted to mimic the alternate MO2 helper. The single-environment isolation and output-target behavior are explicit product requirements. FFI declarations are bindgen-generated; no native or pin changes were made or recommended here.

## Exact source references

Local paths are relative to `/Users/reilley/Repositories/mods/.worktrees/issue-33-distribution` and describe the inspected working tree rather than an assumed remote commit:

- L1: `src/infrastructure/execution/src/configuration.rs:54-162` (winner/parent construction, output selection, apply order and validation).
- L2: `src/infrastructure/execution/src/profile.rs:339-386` (profile files, identity containers, saves, archive mapping); controlled INI save routing is declared near lines 121-127.
- L3: `src/infrastructure/execution/src/usvfs/mod.rs:196-249` (mapping flags, calls and wide-string encoding).

- S1: https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L2020-L2099
- D1: https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/organizercore.cpp#L2070-L2155
- S2: https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L196-L233
- D2: https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/usvfsconnector.cpp#L196-L233
- S3: https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/profile.cpp#L604-L615
- D3: https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/profile.cpp#L602-L613
- S4: https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L2102-L2144
- D4: https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/organizercore.cpp#L2158-L2200
- S5: https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L1970-L1982
- D5: https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/organizercore.cpp#L2020-L2032
- Priority container type (both snapshots use `std::map<int, unsigned int>`): https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/profile.h#L408 and https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/profile.h#L408
- G1: https://github.com/ModOrganizer2/modorganizer-game_falloutnv/blob/52ea004207cd3834d65290b2b0e34f7132858982/src/gamefalloutnv.cpp#L311-L327
- G2: https://github.com/ModOrganizer2/modorganizer-game_gamebryo/blob/0076e5431bd7fffb4977c724a92027e6e4f11f2e/src/gamebryo/gamebryolocalsavegames.cpp#L34-L62
- Stable plugin pin authority: https://github.com/ModOrganizer2/modorganizer/releases/download/v2.5.2/Mod.Organizer-2.5.2-commits.txt
- U1: https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L40-L86
- F1: https://github.com/Reilley64/usvfs-rs/blob/eb4949fb2439fe5b98901e2fb1afceee752a6133/rust/usvfs-sys/native/barrier.cpp#L63-L126 (resolves the upstream file/directory exports, passes flags/paths, returns failures).

**Recommended next step:** incorporate these supported caller differences and the explicit “no documented misuse established” conclusion into #33's static review. If full development profile parity is required, first establish the separately built game-plugin revisions. No runtime or native remediation follows from this comparison alone.
