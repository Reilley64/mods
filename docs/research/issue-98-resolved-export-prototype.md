# #98 resolved export prototype evidence

Status: owner-approved extra cleanup completed; focused and full checks pass. Spec and manual standards reviews pass for the bounded prototype. Automated style findings remain explicitly dispositioned, not declared clean. See the final validation/disposition section; earlier blocked verdicts are historical.

This is a disposable, `#[cfg(test)]` Rust proof colocated inside `src/infrastructure/environment/src/profile.rs`. It changes no production behavior, public API, CLI, dependency, build target, or real environment. The owner lifted #25 only for this prototype; #99 production remains gated and has a direct native blocker. This document does not propose migration or destination apply.

## Questions exercised

- A positive-file plan can use domain `DataRelativePath` identity, provider rank, `resolve_effective_file`, and `TombstoneIndex` to choose enabled winners, unique files, and Overwrite. The scenario excludes base, disabled and losing paths, drops a tombstoned lower file while retaining a higher restoration, excludes an actual root `meta.toml` contribution while retaining nested `meta.toml` and opaque physical BSA, and uses one highest-priority directory spelling. It is an in-memory resolved fixture, not full snapshot admission or a new production resolver.
- Canonical bytes remain unchanged. Archive input is selected from Custom, Fallout, then a required bound-game fallback only if needed; an explicit empty list stops fallback. The small derived-copy proof reuses existing profile decode/encode, key manipulation, and archive-normalization helpers. It checks ordinary `Saves\` in exported INIs and `__mods_saves\` in execution copies, with invalidation support selected independently from canonical bytes. It does not import unrelated user INIs or the full default payload. Existing `last_archive_list` silently treats decode errors as absence; this fixture explicitly decodes each consumed input first. The existing decode policy accepts Windows-1252 byte sequences, so “malformed” here means decoding rejects malformed BOM/UTF-16 or a required fallback is missing, not every non-UTF-8 byte.
- The write-back fixture keeps canonical managed keys while retaining a child's non-managed edit. It refuses a concurrent canonical change. It is intentionally narrow: optional INI creation/deletion and the full production policy for duplicate/relocated controlled keys need later #99 decisions; the fixture refuses duplicate/relocated identities; no implicit migration or generic merge engine is claimed.
- The scratch-only staged copy uses ordinary filesystem writes and `FileTimes` to check copied/derived bytes and original mtime. It leaves partial output in place on cancellation and makes one final rename attempt after checking whether the final folder exists. This `exists` + `rename` pair does **not** prevent a racing creator from being replaced on all platforms; race-safe no-replace publication remains #99 work. It is not a native Windows path/reparse or disk-full test.
- The temporary INI lifetime scenario is **only a labelled model fixture**: root exit without known descendant drain retains the file; known drain plus successful preservation permits removal; either uncertain drain or preservation failure retains it. It proves no Windows Job, usvfs, actual descendant supervision, safe `temp` ownership, or atomic profile publication.

## Validation

Observed in this worktree:

- `cargo fmt --all && cargo test -p infrastructure-environment export_prototype --lib` — first run compiled, 3 passed and 2 failed (the Custom archive keys were not stripped in export and reconciliation test expected canonical line order rather than controlled-key values). Corrected the fixture and reran: 5 passed.
- `cargo fmt --all && cargo test -p infrastructure-environment export_prototype --lib && cargo fmt --all -- --check && cargo clippy -p infrastructure-environment --tests -- -D warnings` — 6 passed; format and scoped clippy passed.
- `cargo fmt --all && cargo test -p infrastructure-environment export_prototype --lib && cargo fmt --all -- --check && cargo clippy -p infrastructure-environment --tests -- -D warnings && git diff --check` — 6 passed; format, scoped clippy, and diff whitespace passed after the nested save-subtree fixture.

- Repair run: `cargo fmt --all && cargo test -p infrastructure-environment export_prototype --lib && cargo fmt --all -- --check && cargo clippy -p infrastructure-environment --tests -- -D warnings && git diff --check` — 9 passed after inlining, typed Rootcause errors with an IO cause-tree assertion, profile/saves nesting, Custom reconciliation, and metadata/relocation scenarios; fmt, scoped clippy, whitespace passed. The test named for duplicates actually exercises a second wrong-section relocation; same-section duplicates are rejected by the implementation but lack a direct regression scenario. A preceding repair compile failed while replacing generic `?` conversions with typed `.context(...)`; corrected before the green run.

The initial red assertions and final green runs are scenario observations, not a strict pre-implementation TDD cycle. Parent ran `bun run check` after the repair: passed, including format, workspace clippy, dependency/release-version checks, 398 Rust tests and 102 tooling tests. That run did not clear the standards blockers then recorded below; the owner subsequently approved the additional two-issue cleanup described in the final disposition.

## Open fixture details and blocked evidence

- Follow-up: optional INI creation/deletion during child execution, and duplicate/relocated manager-controlled keys during reconciliation. The small fixture refuses ambiguous identities and has a narrow positive Custom-key restoration example; production behavior requires a deliberate policy.
- Follow-up: determine timestamp source for generated invalidation BSA versus canonical-derived INIs. The scratch copy uses the profile source mtime for a derived INI and the generated BSA cache file as its BSA source; it proves chosen source mtime can be preserved, not which production source should be chosen.
- #99 still needs production source admission, stopped-writer and structural-conflict checks, source/configuration revalidation, overlap and symlink/reparse containment, errors/disk full, actual safe temporary ownership, ordered write-back publication, and CLI/dry-run integration. Do not infer a cross-file snapshot or closed save isolation from this prototype.
- macOS cannot validate Windows Job/usvfs process lifetime, real native filesystem behavior, or real-game loose-file invalidation. No Proton apply/launch or destination modification was performed.

## Initial coding-style-gate disposition (historical; final disposition below)

- `export_prototype.rs` / **Rootcause lower-layer results**: fixed by typed `PrototypeError` and `rootcause::Result` inside the colocated module; removed `Box<dyn Error>`.
- `export_prototype.rs` / **Preserve causes at owned boundaries**: fixed decode/encode through `.context(...)`, filesystem failures through Rootcause context, and a missing-source IO cause-tree assertion.
- `profile.rs` / **Pre-MVP test placement**: fixed; the entire prototype is now a colocated `#[cfg(test)]` module; deleted the separate file.
- `export_prototype.rs` / **Focused use-case orchestration**: nonapplicable; this is infrastructure test-only fixture behavior, not an application use-case entry point or production helper module.
- `export_prototype.rs` / **Rustdoc format**: fixed module documentation (`///` before `mod export_prototype`); the remaining internal comment records why existing optional decode behavior needs guarding, not an item API summary.
- `export_prototype.rs` / **Phase spacing**: unresolved after relocation to `profile.rs`. The single standards re-review found missing separation between admission, staging/copying and final publication in `stage_once`; this is a real blocker, not an accepted false positive. The initial pass verdict was corrected. Stop for owner approval before another code repair.
- `export_prototype.rs` / **Narrow custom implementations**: accepted for this intentionally bounded, disposable project-owned export/INI seam proof. It reuses domain resolution and profile INI helpers and does not add production framework/dependencies.
- `export_prototype.rs` / **Test public behavior**: nonapplicable to this authorized pre-production prototype, which has no public export seam yet. Assertions cover project-owned bytes, paths, and retention decisions, not dependency internals.
- `export_prototype.rs` / **Use-case parameters**: nonapplicable; these are private infrastructure fixture functions, not application use cases or ports.

The gate findings were advisory; this disposition does not claim a formal gate override or a clean style review.

## Historical single re-review handoff: stopped for owner approval

Spec axis: no remaining bounded-prototype blockers. Standards axis: the reviewer corrected an earlier pass to FAIL for two issues in `src/infrastructure/environment/src/profile.rs`: `stage_once` phase spacing (admission→staging and copying→publication), and a redundant `if !fallout` inside the `else` of `if fallout` in `derived_ini`. Per the frozen-scope rules, one repair/re-review cycle is complete; another code edit requires owner direction. No commit or tracker closure has occurred.

### Current advisory findings, all in `src/infrastructure/environment/src/profile.rs`

| Rule | Disposition and reason |
|---|---|
| Test public behavior | Accepted/no violation: this approved test-only prototype has no public export API; scenarios assert project-owned outcomes at the practical fixture seam, not dependency internals. |
| Narrow custom implementations | Accepted/no violation: disposable plan/reconcile/staging scenarios address #98's specific questions and reuse existing domain/profile helpers; no production framework or dependency is added. |
| Guard clauses | Accepted/no violation: input rejection and exclusions exit early; directory/file branches perform distinct continuing actions. |
| Choose the narrow conditional form | **Unresolved blocker:** `derived_ini` nests `if !fallout` inside the `else` of `if fallout`, where it is guaranteed true. The reviewer corrected the earlier acceptance after checking this hunk. |
| Phase spacing | **Unresolved blocker:** missing blank lines at responsibility changes in `stage_once`; rustfmt passing is not sufficient evidence. |
| Focused use-case orchestration | Nonapplicable: only infrastructure `#[cfg(test)]` helpers, not an application use case or production helper module. |
| Use-case parameters | Nonapplicable: private fixture functions are not use cases or application-owned ports; no use-case dependency bundle is involved. |

This is an explicit finding disposition, not a claim that the advisory gate is clean or overridden.

### Findings from another worktree

The gate watches registered worktrees and also reported `/Users/reilley/.herdr/worktrees/mods/worktree-silver-meadow-4a2f/` changes. Those are outside this session's source changes and writer authority; no approval or clean review of that code is claimed. Do not modify it as #98 remediation.

- `src/application/src/shortcut/create_shortcut.rs`: Preserve causes at owned boundaries; Language-neutral review priorities; Use-case parameters.
- `src/application/src/shortcut/types.rs`: Use-case parameters.
- `src/application/src/shortcut/mod.rs`: Use-case declaration order.

Each is deferred to the owning worktree/session, not accepted as a correct implementation here.

Additional out-of-worktree findings observed while recording this handoff (same exclusion; no review/approval here):

- `worktree-silver-meadow-4a2f/src/infrastructure/execution/src/launch_inputs.rs`: Focused use-case orchestration.
- `worktree-silver-meadow-4a2f/src/application/src/shortcut/create_shortcut/name.rs`: Phase spacing; Language-neutral review priorities.
- `worktree-silver-meadow-4a2f/src/application/src/shortcut/create_shortcut.rs`: Phase spacing.

## Final owner-approved cleanup and validation

The owner explicitly approved one additional cleanup pass for the two remaining standards issues, rechecking, and committing. Only the redundant `if !fallout` inside its matching `else` and the `stage_once` phase spacing were changed in Rust during this pass. The standards reviewer verified both fixes and reported PASS for the bounded prototype; spec re-review had no remaining prototype blockers.

Final command: `cargo fmt --all -- --check && cargo test -p infrastructure-environment export_prototype --lib && bun run check && git diff --check` — exit 0. Nine focused prototype tests passed; the full suite passed 398 Rust tests, two release-version tests and 102 tooling tests, plus format, workspace Clippy and dependency checks. These results do not cover the unavailable Windows/native/game evidence listed above. No production behavior or public API changed. Capture is on the current non-main branch; no production merge or #99 work is authorized.

### Final local advisory disposition

Every entry below concerns `src/infrastructure/environment/src/profile.rs` only. The original separate prototype file was removed. This is an explicit acceptance record, **not a clean automated style-gate result or override**. The gate is advisory in this worktree.

| Rule | Final disposition and reason |
|---|---|
| Rootcause lower-layer results; Preserve causes at owned boundaries | Fixed in the first repair: typed Rootcause context, cause-preserving propagation and an IO-cause assertion. |
| Pre-MVP test placement | Fixed: prototype behavior, helpers and tests are colocated in one `#[cfg(test)]` module in `profile.rs`. |
| Choose the narrow conditional form | Fixed in the owner-approved cleanup: the redundant nested boolean condition is removed. |
| Phase spacing | The confirmed `stage_once` violations are fixed and manually rechecked; the gate's remaining generic finding is accepted as not establishing another violation. Admission, staged copying, publication and output now have explicit phase breaks; reconciliation also separates guards, input acquisition and output. |
| Test public behavior | Accepted: no public export API is authorized yet; these bounded prototype seams assert project-owned bytes/paths/retention, not dependency internals. |
| Narrow custom implementations | Accepted: disposable contract-specific plan, INI and scratch-copy scenarios reuse existing domain/profile helpers and std filesystem APIs; no production framework or new dependency. |
| Guard clauses | Accepted: rejection paths exit early; optional saves and mutually exclusive transformations do not have an exiting opposite branch requiring inversion. |
| Rustdoc format | Accepted: the prototype module uses `///`; the ordinary comment above archive selection explains why a decode guard is necessary with the existing optional-reader behavior, not a missing item API summary. |
| Focused use-case orchestration | Nonapplicable: infrastructure-only test module, not an application use-case file or reusable production helper. |
| Use-case parameters | Nonapplicable: private fixture functions, not application use cases or application-owned ports. |

### Other-worktree findings remain outside this handoff

All paths in this table are relative to `/Users/reilley/.herdr/worktrees/mods/worktree-silver-meadow-4a2f/`, which this task did not edit, stage, commit or review. Each finding is deferred to that worktree's owner; no correctness acceptance or clean-review claim is made for it. Do not expand #98 to remediate it.

| File | Reported rules |
|---|---|
| `src/application/src/shortcut/create_shortcut.rs` | Preserve causes at owned boundaries; Language-neutral review priorities; Use-case parameters; Phase spacing |
| `src/application/src/shortcut/create_shortcut/name.rs` | Phase spacing; Language-neutral review priorities |
| `src/application/src/shortcut/types.rs` | Use-case parameters |
| `src/application/src/shortcut/mod.rs` | Use-case declaration order |
| `src/infrastructure/execution/src/launch_inputs.rs` | Focused use-case orchestration |
| `src/infrastructure/execution/src/shortcut.rs` | Phase spacing; Narrow custom implementations; Use-case parameters |
| `src/infrastructure/dependencies/src/create_shortcut.rs` | Phase spacing; Use-case parameters; Narrow custom implementations; Dependency direction and composition roots; Use-case declaration order |
