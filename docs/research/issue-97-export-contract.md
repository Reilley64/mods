# Resolved-file export and derived Profile State contract (#97)

## Approved prototype gate exception

The owner approved implementing #98 as a bounded test-only Rust prototype before #25 closes. Production implementation (#99 onward) remains gated on #25; #99 has a direct native blocker. No production behavior change, migration, destination apply or new dependency/build target is authorized by the prototype. Earlier references below to #98 waiting for #25 are superseded only for this bounded prototype.

Status: accepted by the owner. This handoff does not authorize implementation before the gate below is satisfied. Research and grilling may proceed while #25 is open; #98 and all implementation remain gated on #25 closing. No migration work.

## Goal and limits

Export a Mod Environment's resolved mod files and Profile State into an ordinary folder. New Vegas on Proton is a motivating use case, not a required destination or the whole goal; this does not itself add other game support. No destination installation is modified. No real Proton launch or deployment helper is required for acceptance.

Use the current analytical positive-file resolution, not a capture of every runtime-visible file: enabled winners, unique files and effective Overwrite, honoring analytical Tombstones. Exclude Steam-base files, losers, disabled providers and manager-private/transient metadata. Physical BSAs are opaque. An omitted path does not delete anything at another installation. Upstream runtime mappings remain non-opaque under ADR-0005.

## CLI and layout

```text
mods [--environment PATH] export OUTPUT [--include-saves] [--dry-run]

OUTPUT/
  Data/
    <resolved Data-relative files>
    Fallout - Invalidation.bsa
  profile/
    <recognized Profile State files>
    saves/                         # only with --include-saves
```

Relative OUTPUT resolves from the shell/startup working directory. Require a new output folder; refuse pre-existing output, including an empty directory. No overwrite, archive-format, apply or migration option. Normal success is quiet. Errors and retained-partial-output paths go to stderr. CLI-only delivery over a reusable application use case; MCP exposure is deferred.

`--dry-run` shows planned output paths, providers, file counts and byte totals, including profile files and the generated BSA, without creating output. It reserves nothing and cannot guarantee unchanged sources. Documentation, not a README among game files, explains placement and limitations. A `profile/` folder is packaging, not a claim that every contained file has one runtime destination; explain Documents INIs, Local AppData state and optional saves separately.

## Profile payload

Include recognized files when present: `Fallout.ini`, `FalloutPrefs.ini`, `FalloutCustom.ini`, `GECKCustom.ini`, `GECKPrefs.ini`, `plugins.txt`, `loadorder.txt`, `Plugins.fnvviewsettings`, and `modlist.txt`. Saves are opt-in, including their ordinary-file subtree. `modlist.txt` is provenance, not runtime activation. Do not manufacture absent optional files merely to fill the layout. Do not add unknown/private files or a manifest/database.

Ordinary mod-contributed INIs and plugins remain Data files, not excluded by extension. Export does not sort plugins, reconstruct load order or rewrite arbitrary paths inside mod files. Preserve modification times rather than treating `loadorder.txt` as authority to retime files. Export is not a universal portability or playable-setup guarantee.

## Canonical and derived INIs

Canonical Profile State has normal relative `Saves\` routing, not the manager-private route. Initialization must not inject archive-invalidation settings. Preserve user-authored archive inputs; “do not inject” does not mean deleting every existing archive setting.

Derive separate copies for two purposes:

| Purpose | Save route | Archive invalidation |
|---|---|---|
| Stored Profile State | Normal `Saves\` | Original/user-authored values; no init-time injection |
| Managed execution | Private `__mods_saves\` in temporary INIs | Apply manager-owned invalidation settings |
| Export | Normal `Saves\` in exported INIs | Apply manager-owned invalidation settings |

Execution keeps the private route to avoid increasing overlap with ordinary shared saves. This does not establish closed save isolation or strengthen upstream injection guarantees. Export is read-only toward the source and never applies to a game installation or prefix.

Read archive-list inputs from untouched canonical INIs, in order: last case-insensitive `[Archive] sArchiveList` assignment in `FalloutCustom.ini`, then `Fallout.ini`, then the current bound Game Installation's `Fallout_default.ini`. Explicit empty is a value and stops fallback. Read the game default only when needed, without importing unrelated user INIs. Fail on unreadable/malformed relevant input or a missing/unreadable required fallback; do not silently reinterpret decoding failure as absence. Capture/revalidate consumed fallback bytes as configuration input.

In derived copies, use the existing recipe: `bInvalidateOlderFiles=1`, empty `SInvalidationFile`, and `sArchiveList` with `Fallout - Invalidation.bsa` prepended exactly once, retaining the selected remaining list. Remove competing managed archive settings from derived `FalloutCustom.ini` so they cannot override the recipe. Apply the current necessary private-routing overrides to execution copies only, including suppressing conflicting routing keys in derived INIs. Do not rewrite unrelated settings or lose canonical archive-list precedence by stripping keys before selecting the list.

Export the generated invalidation BSA as an explicit support-file exception, not a provider winner or copied base-game asset. This decision does not independently change when the cached BSA is generated. The archive INI list is not an inventory of every BSA in Data. BSA-only export is insufficient; the INI recipe must accompany it. Destination placement remains documented/manual.

## Execution INI lifetime and persistence

Keep original canonical bytes separate from the derived baseline and the child-written temporary INIs. Temporary backing files remain alive throughout all managed processes, including descendants and uncertain drain. Do not tie their lifetime merely to root-process exit or an unrelated capture spool.

After managed execution finishes, preserve permitted child-written changes, excluding manager-controlled routing and archive keys. Preserve the original canonical values/absence of those keys even when the child changes their execution-copy values. Never write the private route or injected invalidation overrides back into canonical Profile State. Preserve other valid edits after unsuccessful child exit as well as successful exit; do not silently change existing persistence into discard-on-failure.

Revalidate canonical inputs before write-back. If they changed concurrently, retain temporary files and report a conflict rather than overwrite them. Make one ordered publication attempt. If preservation fails, execution fails even if the child succeeded; retain/report temporary INIs. Do not retry, roll back or revert already-published work. Remove temporary files only after successful preservation and known process completion. If shutdown or preservation is uncertain, retain/report them. Safe temporary ownership/admission must remain narrow; do not blanket-ignore `temp` contents or add a recovery framework.

Migration, conversion commands and migration recommendations are explicitly excluded. Do not add silent conversion during export or execution. No legacy private-routing compatibility is promised by this contract. This post-MVP work supersedes #31's init-time private-routing requirement for the new behavior without changing ADR-0005's upstream limitations.

## Paths, copying and publication

- Copy ordinary bytes, not links to source paths. Reuse domain case/path semantics; do not substitute host-filesystem comparisons.
- Choose one spelling per merged directory from its highest-priority contributing provider. Reuse that choice throughout the output. Preserve each winning file's spelling. Reject structural file/directory conflicts. A per-export in-memory map is an implementation suggestion; no persistent “proper names” registry is approved.
- Reject links/reparse points, unsupported entries, unsafe destination paths and output overlapping the Environment Root or Game Installation, including source-containing destinations. No silent skipping or external dereferencing. Account for both staged and final output locations.
- Preserve file modification times, including files whose export bytes are derived INIs. Fail before final publication if required times cannot be preserved. Do not claim preservation of ACLs, creation times, alternate streams or unrelated metadata.
- Require stopped writers. Fix the planned inventory, detect observable input/configuration changes and fail rather than silently switching providers. Do not promise a coherent cross-file snapshot under concurrent external mutation or assume an MVP cross-process lock.
- Write into a temporary sibling folder and publish the final folder only after successful copying/validation. Never replace an output that appears before publication. One publication attempt, no retry/recovery framework.
- Cancellation stops in place before publication. Retain/report partial output; no rollback, revert or automatic cleanup. Disk-full and other copy/validation failures do not publish completed output. Once final publication succeeds, the export is complete; later cancellation does not remove it.

## Frozen compatibility and non-goals

Preserve domain case/path identity, Mod Priority, the reserved invalidation name/recipe, opaque BSA handling, upstream usvfs and process-supervision limitations, CLI quiet-success conventions and ordinary file bytes (except explicitly derived profile INIs). Keep application policy transport-free with narrow infrastructure ports. No new dependencies, native fork, build target, stronger sandbox or platform harness is authorized here.

Non-goals: destination apply/deployment; native Linux manager/VFS; root patch installation; bundled game assets; generalized path rewriting; automatic Proton setup; Steam Cloud synchronization; built-in sorting; incremental sync; rollback/history integration; archives/manifests/databases; MCP delivery; migration. Preserve normal source isolation during export; approved execution INI write-back is a separate execution behavior, not permission for export to mutate source state.

## Expected modules and bounded delivery

Known affected areas, subject to the existing code seams:

- Domain paths/provider resolution and application export planning/policy, not conflict-report rows.
- Environment snapshot/safe filesystem/profile initialization/validation and execution preparation.
- Settings profile-layout validation, which currently duplicates injected-key requirements.
- Execution profile derivation/mappings, temporary-file ownership and INI persistence.
- Native execution adapter composition and lifetime integration; existing process/view ownership as needed for uncertain drain.
- CLI parsing/composition/output/errors and user documentation.

#98 proves the highest-risk derivation/lifetime/export behavior after #25 closes, without a Proton apply workflow. #99 owns shared export and the explicitly approved initialization/execution lifecycle changes; #100 exposes/documents CLI; #101 validates the approved behavior. Do not let ticket boundaries omit the execution changes or create unapproved migration work.

## Acceptance and evidence

1. Winning, unique and Overwrite files have correct bytes/relative destinations; disabled/losing/base/private state is absent. Include Tombstone omissions/higher restorations, nested ordinary metadata-like names and opaque BSAs.
2. Two payload folders contain the recognized profile files and opt-in saves; generated BSA and derived invalidation INIs agree. No unrelated user INIs or entire default-INI payload are imported.
3. Init does not inject invalidation/private routing. Execution applies both only to derived copies. Test list precedence, explicit empty values, missing/malformed fallback, optional files and encoding/newline preservation.
4. Write-back retains original managed-key values and other valid child edits, detects canonical changes, and reports failures without leaking overrides or silently losing changes. Cover unsuccessful child exit, partial publication, cancellation and uncertain process drain/temporary lifetime.
5. Path/case/spelling, links/reparse points, source/output overlap, source changes, pre-existing/racing output, timestamp failure, disk-full, partial output and stop-in-place cancellation follow the contract. Test project-owned behavior, not dependency internals.
6. CLI syntax, dry-run inventory/no-output creation, save opt-in, quiet success and actionable failure/partial-path reporting match the contract. Documentation explains manual placement, invalidation, timestamp/load-order limits and absence of sync/deployment guarantees.

Current environment: documentation/source review, local path/reference checks and `git diff --check` can run here. No code is implemented; no behavior test result follows from research. Later checks must use the repository's own commands (`bun run check` and scoped project tests) on supported environments. Windows usvfs, real process lifetime, filesystem/reparse/timestamp behavior and in-game invalidation evidence unavailable here remain explicitly blocked until obtained; no new harness is authorized as a substitute. A Proton transfer/apply/launch is not a required acceptance gate.

Before implementation, attach a concrete in-scope validation plan and satisfy #25. Repair collected blockers once and re-review under `docs/agents/ticket-scope.md`; report follow-ups separately.

## Handoff review

A bounded read-only source/decision consistency review found no blockers. Implementation fixture details remain: optional INI creation/deletion and duplicate/relocated managed keys during reconciliation; and the timestamp source for generated BSA versus derived profile files. These are recorded for #98/#99 validation, not migration or broader recovery work. No runtime tests ran. The owner approved this consolidated handoff after review.

## Evidence

See [#96 research](issue-96-resolved-file-export.md), [ADR-0007](../adr/0007-derive-execution-and-export-profile-inis.md), [#95](https://github.com/Reilley64/mods/issues/95) and [#97](https://github.com/Reilley64/mods/issues/97). Research source pins describe current implementation, not completed support for this contract.
