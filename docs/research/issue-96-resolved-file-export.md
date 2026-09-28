# Resolved-file folder export (#96)

## Approved prototype gate exception

The owner approved implementing #98 as a bounded test-only Rust prototype before #25 closes. Production implementation (#99 onward) remains gated on #25; #99 has a direct native blocker. No production behavior change, migration, destination apply or new dependency/build target is authorized by the prototype. Earlier references below to #98 waiting for #25 are superseded only for this bounded prototype.

## Current owner decisions and status

The consolidated [#97 contract](issue-97-export-contract.md) records the current owner decisions. Profile State is included separately from Data, saves are opt-in, execution uses temporary INIs, and invalidation/private-routing settings are derived rather than injected during initialization. Exported INIs include invalidation settings and normal save routing. Migration is excluded. Archive-list priority is Custom → Fallout → bound-game default; explicit empty stops fallback, malformed/unreadable inputs fail, and write-back preserves canonical manager-controlled keys.

The report below records the inspected source behavior and research alternatives. Its earlier Profile State exclusions, canonical-invalidation proposals and migration suggestions are **historical, not current scope**. Later research sections explain the execution/export projection seams. No implementation, runtime trial or migration occurred. Research/grilling may proceed before #25; prototype and implementation remain gated. The owner approved the consolidated handoff; this is not implementation or runtime evidence.

## Answer and scope

Exporting ordinary resolved files is feasible without copying the Mod Environment, using usvfs, or constructing a deployment bundle. The useful set is **all winning enabled Data Mod files and effective Overwrite files**, including unique files, not just File Conflict rows. Copy their bytes beneath an output root at their Data-relative paths. Exclude Steam-base winners, losing files, disabled providers, Profile State and manager-private state.

**Archive invalidation needs a separate decision.** The current generated invalidation BSA is not an ordinary provider winner. Current invalidation also depends on settings in `Fallout.ini`. Adding the BSA to an export can supply an ingredient, but does not by itself enable invalidation at a destination. The owner expressly added automatic archive invalidation to this research; this report identifies the smallest choices without implicitly restoring Profile State export or destination modification.

Research snapshot: 2026-09-28, repository commit `2335e3e0e446ed847bc5a58bee80adc9116fa32e`. Read current [#95](https://github.com/Reilley64/mods/issues/95), [#96](https://github.com/Reilley64/mods/issues/96) and [#25](https://github.com/Reilley64/mods/issues/25) bodies and comments through `gh`; the API returned no comments on these three issues. The narrowed owner scope supersedes the earlier Profile State/Proton bundle. New Vegas on Proton is a use case, not a required export destination. Ordinary mod-contributed INI and plugin files remain eligible: the exclusion is **Profile State**, not those extensions.

Research only. No implementation, prototype, apply operation, game launch, API approval or frozen contract. No GitHub mutation was performed for this report. Research and grilling are permitted before #25 closes; prototype and implementation remain gated. No required manifest, integrity-hash format, archive format, apply helper or Proton acceptance gate is proposed. Recommendations below are inputs to #97, not approved behavior.

## Current semantics: three different things

### 1. Analytical effective files

- Provider rank is Base < Regular(Mod Priority) < Overwrite. Enabled Data Mods participate at their existing priority; Overwrite is highest. Snapshot resolution applies the base first, enabled mods next and Overwrite last. [Provider ranks][ranks]; [snapshot resolution][snapshot].
- Each ordinary file contributes to resolution, including files with no competitor. The snapshot retains `EffectiveResult::File` or `Absent`, not only conflicts. The selected provider retains its original path spelling. [Snapshot resolution][snapshot].
- Root `meta.toml` is provider metadata and is skipped by ordinary file collection. Nested `subdir/meta.toml` is ordinary content, not a suffix-based exclusion. Provider validation rejects case aliases of reserved root metadata and the reserved invalidation BSA name. [Collection][collection]; [provider validation][validation]. Do not add blanket exclusions for `.toml`, `.ini`, `.esp`, `.esm` or `.mohidden`.
- Canonical Tombstones can suppress a lower file or inclusive directory subtree. A sufficiently high file can survive a lower Tombstone. Structural file/directory collisions are invalid before absence is treated as a winner. [Tombstone resolver][tombstones]; [snapshot resolution][snapshot].
- Physical BSA files are opaque ordinary files. The export scope does not resolve their members or decide whether a loose file beats a member at runtime. This follows [CONTEXT.md](../../CONTEXT.md) and #95, not a new archive analysis feature.

**Do not export conflict rows.** `Projection::rows` skips paths with fewer than two unsuppressed non-base files. Thus a unique texture, a mod file that only overrides Steam Data, or a unique Overwrite file can be absent from the report but required in export. [Conflict row filtering][rows]. Likewise, do not use hypothetical disabled-mod inspection as the export view.

`PreparedExecution.winners` contains all analytical file winners, **including Steam Data**, and omits absences. `visible_files` adds physical paths and modification times. This is useful evidence of a reusable enumeration pattern, not a ready export contract: base filtering is still needed, absence information has been discarded, and execution preparation also reads Profile State and validates execution-specific state. [Preparation][preparation]. Exclude by provider ownership, not byte equality: a mod-owned winner identical to a base file is still a mod contribution.

### 2. Managed execution is not that analytical projection

Current `ViewConfiguration::new` sorts enabled non-base provider roots and `apply` recursively links them into Data. The supplied winners are validated but **do not restrict runtime visibility**. The Output Target remains at its normal rank; selecting a target is not a priority change. Profile mappings and the invalidation file are added separately. [Actual configuration][configuration].

Consequently provider-root metadata may be visible at runtime, and execution need not suppress entries covered by analytical Tombstones. Physical Steam files remain fallback. The owner adopted this behavior; older research sections recommending mandatory runtime filtering are historical. See the adoption/supersession at the top of [mapping research](issue-33-view-mapping-model.md), current configuration comments, [CONTEXT.md](../../CONTEXT.md), and [ADR-0005](../adr/0005-use-upstream-usvfs.md). Upstream execution does not promise copy-on-write, durable execution Tombstones or an opaque namespace.

**Recommendation for #97:** explicitly choose the analytical resolved-file set for this folder export, consistent with excluding manager metadata. Do not advertise a capture of every file observed by a managed process. Reusing a recursive runtime provider mapping would export losers and metadata and fail the requested set semantics. This is a decision to record, not permission to change either resolver.

Example: Low contributes `meshes/chair.nif`; High has a Tombstone for `meshes`; Overwrite contributes `meshes/table.nif`. The analytical export omits chair and includes table. Runtime recursive overlays may expose chair. If a separate destination already contains chair, copying the exported folder does not delete it.

### 3. A positive-file folder is not synchronization

An omitted file can mean a base-only path, disabled content, a losing provider or an analytical absence. A folder cannot encode those distinctions as deletion instructions. With a new empty output root, omission correctly represents the selected positive-file set. Copying that folder over an existing game or old export can leave unwanted files behind.

Recommend describing this as a positive resolved contribution, with no deletion, deployment-equivalence or reapply promise. If #97 instead demands reproduction of absences in an existing destination, stop for a scope decision: rejection or explicit deletion semantics are needed. Do not silently invent whiteouts, remove base files, or turn export into incremental sync.

## Paths and copying safety

These are proposed narrow safeguards, grounded in existing project policy, not a new sandbox claim.

| Concern | Evidence and consequence for #97 |
|---|---|
| Relative namespace | `DataRelativePath` normalizes separators to `/`, rejects absolute/drive paths, traversal, invalid Windows components, device aliases and trailing spaces/dots. Preserve this domain validation rather than treating arbitrary host paths as Data paths. [Paths][paths]. |
| Case equivalence | Identity uses simple Unicode case folding, without Unicode normalization or multi-character expansion. Original spelling is separate. Do not substitute ASCII lowercase or host filesystem comparison. [Identity][identity]; [path tests][paths]. |
| Merged directory spelling | Distinct winners can have paths such as `Textures/a.dds` and `textures/b.dds` across providers. Those share a logical parent but have different literal spellings. #97 must choose deterministic parent spelling or reject unrepresentable/ambiguous spelling; do not silently create two logical copies on a case-sensitive filesystem. Per-provider collision rejection does not settle merged output spelling. [Provider validation][validation]; [snapshot resolution][snapshot]. |
| File versus directory | Reject structural collisions; a higher provider or Tombstone must not legalize them. Validate all output destinations before starting publication. [Snapshot resolution][snapshot]. |
| Links and special entries | Existing `SafeDir` walks directories without following links; `open_regular` rejects reparse points, non-regular files and files with link count other than one. Retain this policy unless separately changed. Ordinary-byte export is not permission to dereference external links. [Safe filesystem][safe]. |
| Copy API defaults | Microsoft documents that `CopyFile` follows a source symbolic link and overwrites a destination symbolic-link target. A generic copy call is not sufficient admission or containment validation. Source and output components need checks/handle discipline. [Microsoft CopyFile][copyfile]. This is API evidence, not a recommendation to use that API. |
| Output overlap | Reject output equal to, inside, or enclosing the Environment Root or Game Installation. Include existing ancestors/aliases, not only lexical prefix strings. The project already has identity-based ancestor traversal. [Safe filesystem][safe]. This protects the unchanged-source promise and avoids recursively ingesting output. |
| Existing output | Smallest proposed policy: require a new output directory, refuse pre-existing targets, never merge/overwrite by default. Whether an existing empty directory is allowed is a #97 choice. Refusal avoids stale exports and arbitrary overwrite policy; it does not eliminate races. |
| Unsupported entries | Fail clearly rather than silently skip unsafe content. Empty-directory preservation is not needed to copy winning files, but should be stated explicitly if excluded. Keep private root metadata out while retaining legitimate nested content. |

A minimal conceptual layout is `<output>/<Data-relative-path>`, for example `<output>/meshes/item.nif` and `<output>/Example.esp`. This avoids requiring an extra `game/Data` wrapper. Final layout and option names belong to #97. No file needs a link back to the Environment Root; no source archive or VFS is required to read the resulting ordinary bytes.

## Source consistency and incomplete output

Existing execution revalidation rereads preparation and compares state, configuration bytes, lengths and modification times. It is not a byte snapshot of every content file. [Preparation][preparation]. Do not infer a cross-process export lock or atomic capture from it. A file can change while copied; same-size changes or timestamp restoration can evade metadata-only comparison. Microsoft also notes that last-write time is not fully updated until writing handles close. [Microsoft file times][filetimes].

For #97, choose a stated consistency level rather than promising a snapshot:

- Require the user to stop execution and other writers; refuse known pending operations. This prerequisite alone cannot prove there are no external writers.
- Fix the selected inventory before copying. Read admitted ordinary files through safe handles. Detect and report observable input disappearance, replacement, length/mtime or configuration changes, including changes that alter winners. Do not silently switch providers halfway through.
- If exact coherent capture under concurrent mutation is required, that is additional design work; neither a per-file checksum nor a final rescan alone proves one cross-file point-in-time snapshot. No required hash manifest follows from this concern.
- Choose direct output with visibly incomplete failure versus a temporary output followed by one publication attempt. A sibling temporary directory/final rename could keep the intended final path absent on failure, but naming, ownership, same-filesystem limits and cancellation behavior need approval. It must not become a retry/recovery framework.
- Disk-full, permission failures, changed input and cancellation must return failure and identify any retained partial output. Do not report success, merge into a prior completed export, or delete unrelated user files. Automatic cleanup versus retained partial output remains a #97 decision; #25's one-attempt/manual-cleanup policy is relevant precedent, not an already implemented export mechanism.

The application should own selection and policy; infrastructure should own filesystem reads/copies/publication. Existing domain paths, provider ranks and Tombstone resolution are stronger reuse candidates than the transport DTO of a conflict report. Existing snapshot/preparation and safe filesystem code are internal seams to evaluate, not a mandate to expose execution preparation as a public export API.

## Metadata: preserve bytes is not preserve gameplay

The requested minimum is file bytes and relative paths. Do not silently promise ACLs, owner identities, Windows alternate streams, creation times or Linux permissions. Microsoft documents that copy APIs can propagate attributes such as read-only status, while timestamps depend on filesystem representation and update behavior. These require intentional policy, not incidental API behavior. [CopyFile][copyfile]; [file times][filetimes].

Modification time deserves an explicit decision. Primary MO2 Gamebryo code sorts plugins by backing-file modification time when recovering actual order, while separately reading activation from `plugins.txt`; it writes `loadorder.txt` as manager state. [MO2 plugin lists][mo2plugins]. This is evidence that timestamp loss can matter for the New Vegas use case, not an export requirement to reconstruct plugin order. Options are best-effort original mtime preservation with documented limitations, strict preservation or bytes/paths only with an explicit warning. Do not retime plugin files into the advisory `loadorder.txt` order: current mods explicitly emits `LoadOrderNotEnforced`. [Current Profile State projection][profileconfig].

No option here copies activation state, saves or Profile State INIs. Ordinary mod-contributed plugins, INIs, BSA files and runtime DLLs under Data remain ordinary winners. Their presence does not prove destination compatibility; external root prerequisites, absolute paths inside files and native plugin dependencies can still prevent use. Export should preserve their bytes, not rewrite or certify them. No Proton compatibility investigation or launch is needed to establish folder-copy behavior.

## Added owner requirement: automatic archive invalidation

### What the project actually owns and generates

1. Environment initialization writes `cache/Fallout - Invalidation.bsa` from `empty_bsa_bytes()`. The generator produces a **36-byte empty BSA header**, magic `BSA\0`, version `0x68`, offset 36, flags `0x3`, followed by five zero fields. Cache validation compares against those exact generated bytes. It is project-generated support data, not copied Steam content. [Initialization and generator][generator]; [cache validation][bsavalidation].
2. Ordinary provider validation rejects a root file at that reserved name, and ordinary winners enumerate base/mod/Overwrite roots, not `cache`. Thus **exporting analytical winners alone will not include this BSA**. [Provider validation][validation]; [snapshot resolution][snapshot].
3. Managed execution explicitly maps the cache file to `Data/Fallout - Invalidation.bsa`, separately from provider overlays. [Current Profile State configuration][profileconfig]. Therefore a complete inventory of runtime support files cannot be inferred from `PreparedExecution.winners`.
4. Profile initialization patches `[Archive]` in `Fallout.ini` with `bInvalidateOlderFiles=1`, `SInvalidationFile=` and an `sArchiveList` beginning with the invalidation BSA once, retaining the remaining selected archive list. Selection prefers an imported `FalloutCustom.ini` archive list, then imported `Fallout.ini`, then the game's default. It strips conflicting managed archive keys from imported `FalloutCustom.ini`. [Profile initialization][profileinit]; [INI transformation][inipatch]. Current execution validates these keys; it does not make a loose exported BSA auto-configure another installation.
5. That same INI transformation also sets `bUseMyGamesDirectory=1` and `SLocalSavePath=__mods_saves\`. **Do not export the whole patched INI merely to obtain invalidation.** It would reintroduce excluded Profile State and manager save routing. Those save keys are not part of an invalidation-only recipe. [INI transformation][inipatch].

No `ArchiveInvalidation.txt` generation is present in this recipe. The empty `SInvalidationFile` value is part of the selected BSA-based approach, not a missing file to copy.

### Primary external corroboration, with limits

MO2's pinned FalloutNV plugin names the same `Fallout - Invalidation.bsa` and BSA version `0x68`. Its Gamebryo implementation sets `bInvalidateOlderFiles=1`; when invalidation is enabled, it adds the dummy BSA to the archive list at position zero if absent, creates it if absent and clears `SInvalidationFile`. [MO2 FalloutNV invalidation][mo2fnvinvalidation]; [MO2 invalidation preparation][mo2invalidation]. This corroborates **file plus configuration**, not a claim that a copied BSA alone works. MO2's destination edits are its own behavior and are not authorization for an export apply helper.

These are primary sources for the managers' implementations, not Bethesda engine source or a fresh real-game test. No runtime equivalence between mods' empty BSA and every invalidation approach is established here. Actual loose-file replacement under the eventual approved recipe remains platform/game validation evidence, separate from proving export bytes.

### Smallest choices for #97

| Option | What it provides | Limit / decision needed |
|---|---|---|
| Include the generated BSA as one explicit support-file exception | An ordinary Data-relative support file, not Profile State or base assets. Generation or copying validated cache bytes can produce the same bytes. | Does not enable destination settings. Decide whether always included or selected, reserved-name handling and whether generated support files are part of export's definition. |
| BSA plus documented invalidation-only settings | The smallest useful export support without reading/copying the user's whole Profile State or modifying a destination. Explain the three archive settings and retaining the destination archive-list tail. | Manual configuration is not fully automatic destination invalidation. A snippet is not a complete replacement `Fallout.ini`, and a generic folder has no authority to select the target INI. Decide whether this satisfies the owner's meaning of “support automatic archive invalidation.” |
| BSA plus generated configuration artifact | Could automate production of a narrow recipe while keeping other Profile State excluded. | A recipe file has no effect unless an identified consumer reads it. Its format/location, archive-list source, override precedence and consumer are unknown. Do not assume a Data INI, `FalloutCustom.ini`, or a new xNVSE requirement is automatically consumed. |
| Opt-in destination configuration | Could actually apply the required destination settings. | Changes the current no-apply/no-destination scope and needs explicit owner approval. Not recommended or authorized by this report. |

**Recommendation:** separate automatic *generation/export of invalidation support* from automatic *configuration of a destination*. Ask the owner which is required. Preserve the default “Profile State excluded” boundary; if automation must change an effective destination INI, the contract must expressly authorize that narrow exception or a later workflow. Do not resolve this tension by silently exporting Profile State, bundling a root prerequisite, or claiming success from BSA presence alone.

## Decisions and evidence still needed

For #97, in order:

1. Confirm the analytical positive-file set, metadata exclusion and absence disclaimer, rather than runtime-overlay equivalence.
2. Settle the invalidation automation meaning and explicit generated-file/configuration exception, if any. This is the main new scope question.
3. Choose root layout, merged directory spelling, mtime policy and empty-directory treatment.
4. Choose output admission, source-change guarantees, partial/cancelled-output handling and publication policy. Keep source and Game Installation unchanged.
5. Then define CLI/application interfaces, preview if any, errors and quiet success under repository conventions. This report freezes none of these; it does not add MCP exposure.

Later project-owned fixtures should cover unique and competing winners, Overwrite, disabled/base exclusion, root metadata versus nested ordinary files, opaque BSA bytes, ordinary mod INIs/plugins, Tombstone absences and higher restorations, case/structural collisions, links/reparse points, overlap, changed sources, existing output, disk-full and cancellation. If invalidation support is selected, assert the extra generated file and exact narrow configuration artifact without accidentally exporting Profile State or overwriting mod content. These are proposed acceptance subjects, not a new test harness or implementation authorization.

**Validation gaps:** this is static source research. No Rust checks, Windows filesystem/reparse/share-mode trials, export implementation tests, transfer tests, invalidation game test or Proton launch ran. Windows-specific behavior is unverified in this session, not passed. Cross-filesystem timestamp and Unicode behavior remain unknown until a bounded contract requires and validates them. No real playable-setup, plugin activation, load-order, save-migration or universal compatibility claim is made.

## Execution-time routing follow-up (research, not approved design)

Current initialization combines invalidation keys with manager-specific save routing in `environment/src/profile.rs:491–515`. Stored validation enforces those keys both in `environment/src/profile.rs:191–245` and `settings/src/layout.rs:76–124`. Current execution maps the canonical INIs directly (`execution/src/profile.rs:341–380`), so child edits can persist. Moving routing to execution therefore involves initialization, both validators, execution mapping and existing-environment compatibility, not an export-only transformation.

One candidate is transient routed INIs. That requires an explicit child-edit persistence policy, safe temporary ownership through the complete managed Job (including uncertain drain), and narrowly admitted temporary files. Existing capture-spool ownership is precedent, not a ready shared scratch area. Plain copy-back would leak private routing; silently discarding changes would alter existing behavior. Relevant seams: `environment/src/execution_preparation.rs:165–198,256–305`, `environment/src/snapshot.rs:122–176`, `dependencies/src/execution_adapter/native.rs:100–196,244–264`, and `execution/src/process.rs:286–327` (all below `src/infrastructure/` at the research commit).

Before choosing this machinery, research is checking whether canonical normal save paths plus execution-only VFS directory mapping could satisfy the owner intent without temporary INIs. Shared-save visibility/isolation must be checked; it is not assumed safe.

Existing environments already contain `__mods_saves` and initialization did not preserve the original imported routing. A conversion/default or a legacy compatibility/refusal policy is still an owner decision. Archive invalidation keys are separate from private save routing and may remain in the canonical/exported INI alongside the generated Data BSA.

This is static inspection only; no runtime evidence or implementation approval.

### Normal save-path mapping alternative

Static follow-up found no requirement for a private `__mods_saves` alias in the current mapping API. Canonical `bUseMyGamesDirectory=1` and `SLocalSavePath=Saves\` could use the existing execution-only mapping from `profile/saves` to the normal game-facing `Saves` directory. Named INI mappings could remain direct, preserving their current edit persistence and avoiding temporary INI lifetime/merge-back machinery. Current seams: `src/infrastructure/execution/src/profile.rs:341–380`, `configuration.rs:154–160`, and `usvfs/mod.rs:210–221` at the research commit.

This is **not closed save isolation**. Shared physical Saves-only entries can remain visible through fallback; writes/deletions to existing physical destinations are not guaranteed redirected. Using the normal shared directory increases practical overlap with ordinary saves compared with the private alias. The private alias also never guaranteed opacity. Do not promise shared-save protection or add a stronger sandbox silently.

Primary evidence: [pinned usvfs directory mapping](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/usvfs.cpp#L716-L801), [mapping flags](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L40-L86), and [MO2 normal New Vegas saves path](https://github.com/ModOrganizer2/modorganizer-game_falloutnv/blob/52ea004207cd3834d65290b2b0e34f7132858982/src/gamefalloutnv.cpp#L108-L111). Current #31 explicitly pins `__mods_saves`; a normal-path contract needs an explicit owner supersession. #30/#25 already accept non-opaque upstream mappings, but that does not itself approve changing the path.

**Pending owner decision:** accept normal-Saves mapping and its practical shared-save overlap risk, or retain the private alias through a runtime-only INI projection and settle that projection's write-persistence/lifetime contract. Existing environments need a subsequent explicit compatibility/conversion policy either way. No Windows execution or save mutation test ran; this is feasibility evidence only.

### Moving archive settings to derived execution/export INIs

The current archive-list selection can be reused in principle: last `[Archive] sArchiveList` assignment in canonical `FalloutCustom.ini`, then canonical `Fallout.ini`, then bound-game `Fallout_default.ini`, currently falling back to an empty tail. An explicitly empty value is currently present, not absent, and therefore stops fallback. See `src/infrastructure/environment/src/profile.rs:63–70,574–610`. Resolve from untouched canonical inputs before removing competing keys in derived copies. Keep original canonical bytes for reconciliation so injected/removed managed keys do not become false user changes.

Current execution preparation does not consume the bound-game default INI (`environment/src/execution_preparation.rs:51–62,165–198`), and its revalidation cannot detect changes to that default. Reuse a narrow bound-game default read from `game_platform/src/profile_sources.rs:85–91` when needed; do not call the whole initialization loader, which also reads unrelated user-profile INIs. Capture/revalidate whichever fallback bytes are actually consumed. Export uses the same derivation without executing or copying the whole default INI as a separate payload.

The existing `last_archive_list` helper turns decode errors into absence. Whether to retain that behavior, how to treat missing/unreadable defaults, and explicit empty-list fallback are open contract decisions; fail-clearly behavior is proposed rather than silently assuming an archive list. Canonical validators must stop requiring injected archive keys; derived-output validators can still check the controlled recipe. No migration, code edits or runtime checks were performed for this research.

## Sources

Local source links below are pinned to the inspected commit. External MO2 sources were fetched directly at the stable component commits recorded in [the existing provenance research](issue-33-mo2-profile-metadata.md); Microsoft pages were read directly on the research date. No source code was executed.

[ranks]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/domain/src/providers.rs#L1-L95
[snapshot]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/snapshot.rs#L649-L801
[collection]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/snapshot.rs#L822-L921
[validation]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/snapshot.rs#L397-L565
[tombstones]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/domain/src/provider_resolution.rs#L23-L100
[rows]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/application/src/conflicts/projection.rs#L445-L494
[preparation]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/execution_preparation.rs#L47-L274
[configuration]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/execution/src/configuration.rs#L42-L161
[paths]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/domain/src/paths.rs#L26-L157
[identity]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/domain/src/identity.rs#L1-L25
[safe]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/safe_fs.rs#L79-L270
[profileconfig]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/execution/src/profile.rs#L109-L383
[generator]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/lib.rs#L126-L145
[bsavalidation]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/lib.rs#L441-L523
[profileinit]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/profile.rs#L63-L100
[inipatch]: https://github.com/Reilley64/mods/blob/2335e3e0e446ed847bc5a58bee80adc9116fa32e/src/infrastructure/environment/src/profile.rs#L491-L614
[mo2plugins]: https://github.com/ModOrganizer2/modorganizer-game_gamebryo/blob/0076e5431bd7fffb4977c724a92027e6e4f11f2e/src/gamebryo/gamebryogameplugins.cpp#L92-L247
[mo2fnvinvalidation]: https://github.com/ModOrganizer2/modorganizer-game_falloutnv/blob/52ea004207cd3834d65290b2b0e34f7132858982/src/falloutnvbsainvalidation.cpp#L1-L16
[mo2invalidation]: https://github.com/ModOrganizer2/modorganizer-game_gamebryo/blob/0076e5431bd7fffb4977c724a92027e6e4f11f2e/src/gamebryo/gamebryobsainvalidation.cpp#L43-L143
[copyfile]: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfile
[filetimes]: https://learn.microsoft.com/en-us/windows/win32/sysinfo/file-times
