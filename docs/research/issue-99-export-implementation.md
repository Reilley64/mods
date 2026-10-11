# Issue #99 implementation evidence

Status: implementation and bounded review, prepared for a draft PR at the owner’s request. This report does not close #99 or assert release readiness.

## Contract and scope

Implements the owner-approved #99 scope and current #95 supersessions. Contract: `1feddf41da1b2eeff9d7f5c4eb7ea1ace0767f09:docs/research/issue-97-export-contract.md`. The prototype was not copied wholesale.

Includes application resolved export, narrow infrastructure ports, canonical normal save routing, derived execution/export INIs, child-edit preservation and temporary-file ownership. No CLI exposure, end-user placement documentation, migration, destination apply, game launch, dependency, build target, platform harness. The owner subsequently requested PR publication.

## Implementation and conservative choices

- Application `export_environment` prepares an inventory, applies case/priority/structural policy, then optionally publishes. Dry run creates no output and reserves nothing.
- The environment adapter reuses analytical winners, excludes base assets, captures recognized profile files, and includes saves only on request. Nested ordinary metadata-like names and opaque BSAs remain Data files.
- Export derives INIs without source/cache mutation. Derived files inherit canonical modification times. The support BSA inherits cache BSA modification time.
- Export stages in a sibling, checks copied content and source/configuration inputs, and publishes once without replacement on Windows. Errors and cancellation retain staging with a typed path.
- Canonical validators and initialization require normal `Saves\` routing and do not require or inject archive invalidation. Execution copies use private routing and invalidation.
- Archive selection uses untouched Custom, then Fallout, then required bound-game default input. Explicit empty stops fallback. Consumed default bytes are revalidated.
- INI preservation keeps original managed keys and other valid edits. Absent optional INIs remain absent unless a child creates valid content. Child deletion of an existing INI fails and retains temporary files. Concurrent canonical changes fail.
- Temporary INIs survive owner drop during uncertain drain. Cleanup follows known managed Job completion and successful preservation. Publication has no rollback or retry.

## Acceptance evidence

Colocated tests cover:

- Export inventory, base exclusion, directory spelling, structural conflicts, save opt-in, selected-save link rejection and dry-run source/cache immutability.
- Ordinary and derived staged bytes, canonical/cache timestamp provenance, source mutation, overlap, existing output and stop-in-place cancellation.
- Shared INI derivation, canonical/private routes, explicit empty archive values, duplicate/relocated/preamble managed keys, optional creation, malformed edits, deletion/conflict retention, fallback revalidation and UTF-16/CRLF preservation.
- Bounded injected mid-copy write failure: one real chunk remains, the final destination is absent, the source is unchanged, and the report carries `RetainedExport` plus the I/O cause.
- Bounded injected timestamp-set failure: copied bytes remain, later payload files are absent, the final destination is absent, and `RetainedExport` identifies the stage.
- Bounded injected later canonical rename failure: the first canonical edit remains, the later canonical file is unchanged, managed keys retain original values, exactly two renames are attempted, and `RetainedExecutionInis` identifies retained temporary files. There is no rollback or retry.

Failure hooks are `cfg(test)` calls into each file's colocated test module. They replace only the failing operation and run the production orchestration against real temporary files. They are not a platform harness or general fault framework.

Existing domain/provider/snapshot tests exercise the analytical winner and Tombstone resolution reused by export.

## Checks

Before repair, `cargo fmt --all && bun run check && git diff --check` passed: 403 Rust tests, 2 release-version tests, 119 tooling tests, workspace Clippy and dependency checks.

Bounded-repair validation passed:

- `cargo test -p infrastructure-environment export --lib`: 7 passed.
- `cargo test -p infrastructure-environment derived_profile --lib`: 6 passed.
- `cargo fmt --all && bun run check && git diff --check`: exit 0; 406 Rust tests, 2 release-version tests, 119 tooling tests, workspace Clippy and dependency checks.

## Final independent re-review

- Spec: no remaining concrete correctness or local acceptance-test blockers. Injected failure cases cover project-owned policy, not actual Windows behavior.
- Standards re-review found one remaining Phase spacing blocker at `src/infrastructure/environment/src/export.rs:324–325`; no other repair-surface regression was reported. The user then approved the final spacing-only repair.
- The approved repair adds the blank line after `.keep()` and before `copy_and_publish(...)`. Direct inspection, `cargo fmt --all -- --check`, and `git diff --check` passed. No behavior changed; the full test results above precede this whitespace-only edit.
- The spacing-only incremental style check reported `export.rs` / Use-case parameters. The later full gate report repeats 29 findings, including the previously reviewed guard-clause finding. All are covered by the fixed or accepted dispositions below; the guard-clause acceptance is disposition 31. These dispositions are not a clean automated result or an enforce-mode override.
- Final deterministic checks passed as recorded above. The automated semantic gate is not clean, and no override was used.

## Unavailable evidence and limits

- This host is macOS. Windows native execution and publication code was not compiled or run here. The Windows no-replace race test remains unrun.
- macOS tests validate the complete prepublication stage. Final publication on non-Windows returns unsupported and retains that stage; this is not evidence of successful Windows publication.
- Real Windows Job/usvfs lifetime, reparse behavior, modification-time behavior and in-game invalidation remain unavailable.
- Injected write/time/rename failures prove project-owned failure policy, not actual disk exhaustion or OS-specific failure behavior.
- This report accompanies the draft PR; it is not complete acceptance evidence for #95. No issue is closed by the PR.

## Style-gate dispositions

Paths below are under `src/`. Accepted means a manual disposition, not an automated clean result or enforce-mode clearance. No override was used. Independent review rejected the original spacing acceptances in items 9, 21, 24 and 32. The bounded repair fixes those findings.

1. domain/src/profile_state.rs — Focused use-case orchestration: accepted false positive. This is a shared domain Profile State capability used by initialization, validation, derivation and preservation, not an application use-case file or generic utility.
2. Same file — Guard clauses: fixed. Non-Fallout derivation returns early before Fallout-specific routing/archive work.
3. Same file — Narrow custom implementations: accepted. This is a bounded lossless game-INI transformation; comments, duplicate keys, key spelling and newline preservation are the concrete gap. It reuses existing infrastructure codecs and adds no general parser/dependency. The rationale is recorded beside profile_ini_valid.
4. Same file — Phase spacing: added guard/transformation and preservation/output boundaries. Remaining warning accepted: key loops are cohesive parsing/editing phases, with separate derivation, restoration and serialization blocks.
5. Same file — Readability before secondary cleanup (earlier local finding): accepted after the guard fix. append_ini_section owns the nontrivial insertion algorithm needed to avoid duplicate section behavior; it is not a performance cleanup or terse replacement for a clearer established implementation.

6. infrastructure/environment/src/profile.rs — Use-case parameters: accepted false positive. Infrastructure initialization/validation helpers are not application entry points; the primary application initialize_environment signature is unchanged.
7. Same file — Narrow custom implementations: accepted. Existing duplicated patch algorithms were removed; canonical_ini only connects existing byte codecs with shared domain policy. No generic infrastructure was introduced.

8. infrastructure/environment/src/derived_profile.rs — Use-case parameters: accepted false positive. These are infrastructure owner methods; cancellation remains last in cancellable operations. preserve deliberately takes no cancelled token after known Job drain so valid child edits are not silently discarded.
9. Same file — Phase spacing: fixed in the bounded repair. Blank lines separate fallback computation/output, input reads/staging, temporary acquisition/owner construction, owner/staged writes, and staging/error decoration. Canonical-check, child-edit and cleanup boundaries also remain separate.
10. Same file — Narrow custom implementations: accepted. TempDir's default destructive Drop conflicts with the approved uncertain-drain/retention contract. The small owner uses TempDir and SafeDir and changes only cleanup ownership; it is not a new runtime or recovery framework.
11. Same file — Language-neutral review priorities (earlier local finding): accepted after removing expect-based access and adding retained-path error causes. The owner is explicit about retained state and does not ignore preservation errors.

12. infrastructure/environment/src/execution_preparation.rs — Use-case parameters and Use-case declaration order: accepted false positives for both rules. Added methods are infrastructure adapter operations, not primary application use cases; cancellation is last.
13. Same file — Phase spacing: added a boundary between INI revalidation and complete execution-input recapture. Remaining warning accepted; each method is one short infrastructure validation phase followed by output.

14. infrastructure/dependencies/src/execution_adapter/native.rs — Guard clauses: fixed. Unknown drain returns before preservation rather than nesting the success path.
15. Same file — Preserve causes at owned boundaries: fixed. Drain-query and supervision failures are attached to retained-INI diagnostics; underlying errors are not replaced with a marker alone.
16. Same file — Choose the narrow conditional form: accepted. A boolean guard checks the known-empty condition and subsequent if-let branches attach the two possible error reports. This avoids a larger result-state match.
17. Same file — Phase spacing: added binding-revalidation and post-preservation stream phases; launch/supervision/write-back/state-check phases are separate. Residual warning accepted; error-context construction and attached causes form one error-reporting phase.
18. Same file — Use-case parameters and Use-case declaration order: accepted false positives for both rules. This is the existing native infrastructure composition method, not the application execute_program use case. Its existing signature is retained.

19. application/src/export/export_environment.rs — Focused use-case orchestration: fixed. The meaningful case/spelling/structural-conflict algorithm now lives in private export_environment/inventory.rs.
20. Same file — Cohesive orchestration: accepted. The extracted function owns the substantial collision/priority algorithm, not a one-expression helper. The use case still reads prepare → plan → optionally publish → output.
21. Same file — Phase spacing: fixed in the bounded repair. Blank lines now separate preparation, inventory planning, publication and output.
22. Same file — Narrow custom implementations: accepted. The entry point only orchestrates project ports and export policy; it reimplements no maintained generic abstraction.
23. Same file — Use-case parameters and Use-case declaration order (earlier local findings): accepted false positives. Dependencies, Output, Error, instrumented async function are in required order. Dependencies are first by value and CancellationToken is last.
24. application/src/export/export_environment/inventory.rs — Phase spacing: fixed in the bounded repair. Eligibility filtering and directory indexing are separate, as are identity/rank selection and completed-map conflict checking.
25. Same file — Use-case parameters: accepted false positive. plan_inventory is a private policy algorithm, not an application entry point, and has no infrastructure dependencies or cancellation behavior.
26. application/src/export/types.rs — Use-case parameters: accepted false positive. This file declares callable port types and transport-free data, not a use case. Both callable types put CancellationToken last.
27. application/src/export/mod.rs — Use-case declaration order: accepted false positive. It contains module declarations and intentional re-exports, not a primary use case.
28. application/src/execution/mod.rs — Use-case declaration order: accepted false positive. Only a typed diagnostic re-export was added; the use-case declaration order remains in execute_program.rs.
29. application/src/execution/types.rs — Custom primitive justification (earlier local finding): accepted false positive. RetainedExecutionInis is ordinary typed error data (PathBuf plus Display), not a synchronization/runtime primitive.

30. infrastructure/environment/src/export.rs — Use-case parameters and Use-case declaration order: accepted false positives for both rules. This is infrastructure implementing application-owned callable ports; the application entry point is separate and compliant.
31. Same file — Guard clauses: accepted. Entry-type branching handles genuinely different continuing paths (directory traversal vs file capture); source-derived versus ordinary copy also has two real paths. Cancellation and unsupported states exit early.
32. Same file — Phase spacing: fixed, including the user-approved final spacing-only repair. Data/profile/support/save capture, snapshot output, and the final cancellation guard/publication are separated. Re-review caught the missing boundary after `.keep()` before `copy_and_publish(...)`; the earlier complete-fix claim was incorrect. After explicit user approval, that blank line was added and verified by direct inspection, formatting check and diff check.
33. Same file — Narrow custom implementations: accepted. The adapter composes existing resolution, safe filesystem, hash, tempfile and standard time APIs for the exact export contract; no new generic filesystem/runtime abstraction.
34. Same file — Callable port invocation: accepted false positive. prepare_export_port constructs closures; application invokes them with .call tuples. The local publish(...) function is an infrastructure implementation, not an application-owned callable port.
35. Same file — Test public behavior: accepted limited private seam coverage. Dry-run tests use the public application/port seam. Private capture/copy tests cover project-owned stop-in-place, revalidation and retained-stage policy while native publication is unavailable; they do not retest dependency behavior.

36. infrastructure/environment/src/safe_fs.rs — Phase spacing: fixed setter/re-read validation boundary; residual accepted because the setter's handle clone and set_times call are one operation.
37. Same file — Import placement and use: fixed accidental duplicate test imports. New FileTimes/SystemTime imports are module-scoped; no block import was added. Remaining warning accepted; existing io::Error convention is unchanged.
38. Same file — Narrow custom implementations: accepted. The new operation delegates to std::fs::FileTimes and verifies the contractually required mtime; it does not replace a general filesystem library.
39. infrastructure/environment/src/export_publication.rs — Phase spacing: added path validation/FFI boundary. Remaining warning accepted because both wide-path buffers form one acquisition phase.
40. Same file — Rustdoc format: accepted false positive. The only ordinary production comment is the immediate // SAFETY proof on the unsafe call, required by the Unsafe Rust rule; it is not an item summary. The function is a private FFI boundary.

41. infrastructure/environment/src/derived_profile.rs — Test public behavior: accepted. The new test calls ExecutionInis::preserve, observes real canonical and temporary files, and injects only the later rename failure. It covers ordered publication and no rollback/retry, not a dependency suite. Export failure tests likewise run real staging/copy/error reporting and replace only the failing write/time operation.

42. infrastructure/environment/src/derived_profile.rs — Use-case declaration order: accepted false positive. This file defines infrastructure input and lifetime-owner types, not an application use-case entry point. It has no required Dependencies/Output/Error use-case declaration group.

The automated gate continues to report likely findings. These manual dispositions are not a gate override. Deterministic checks are separate evidence.
