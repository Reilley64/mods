# Issue 33 acceptance evidence ledger

Status: **incomplete; no controlled Windows 11 sign-off or full acceptance is recorded.** The current summary below reconciles retained evidence. The following chronological checkpoints and grouped audit matrix preserve historical pending states; they are not the current status authority.

## Current status — non-execution acceptance continuation

**Partial acceptance only.** Native execution/save-routing and real stable distribution remain blocked. The owner waived the disconnected-rebuild check; it is skipped, not passed. Native usvfs was not changed, raw FFI remains bindgen-generated, and no speculative Rust workaround was applied. This checkpoint supersedes the older blanket pending statements for diagnostics, profile-source preservation, and ordinary interruption/refusal below.

### Adopted candidate ordinary runtime acceptance — failed

A single bounded ordinary functional check of candidate `3706f53ae3577ff6ac5daa9fb5b0478a1815f2a9` ran on the owner-confirmed disposable `prime` account with a new synthetic Game Installation and Environment Root. ZIP SHA-256 remained `4c565bb8f6f53048111dd1e69e1fd0eb25ef8892f7909bc77c6f94f5e9d01573`. Normal initialization and config readback passed. The subsequent marker-only managed command exited with `0xC0000005` and empty stdout/stderr; its expected marker was absent. This is failed managed-execution acceptance, not successful child launch or proof of a particular crash cause.

The sequence stopped immediately. No retry, cancellation scenario, synthetic-save write, debugger, crash/event inspection or fault investigation followed. Cancellation, descendant lifecycle and save routing remain unverified. Stable/Winget verification and publication remain blocked.

Postfailure preservation matched the complete baseline: all 311 real Game Installation Data files (paths, sizes and SHA-256), directory entries, measured standard save/profile absence markers, and package/executable hashes were unchanged. No test process remained and no forced termination or original-path restoration was needed. Unmeasured metadata is not covered.

Retained fixture: `C:/Users/prime/mods-issue-33/functional-3706f53-2b0083fe`. Reports: `/tmp/mods-33-synthetic-runtime-preflight.md` and `/tmp/mods-33-synthetic-runtime-initial.md`; before/after inventories and ordinary process results are listed in the latter. No product code changed. This result supersedes earlier statements that the adopted candidate had not been exercised; prior compilation/package/portable checks remain valid only for their stated scope.

### Owner adoption — recursive Data overlays and qualified diagnostics

The owner adopted the Rust prototype into the issue #33 candidate. Prototype commits `51efc0f`, `b74a2dd`, `88d559b` were integrated as `7fb99ae`, `ba1b28f`, `b3fe423`. Enabled non-base provider roots now map recursively in Mod Priority order; Overwrite remains last and the selected Output Target is marked at its ordinary rank. Per-file Data winner links are replaced, while validation, canonical Profile State file mappings, invalidation and recursive save routing remain in place. Native source, generated bindings, dependency pins and lifecycle code remain unchanged.

CLI and MCP plugin warnings now describe the analytical Data projection and leave runtime availability unestablished. Traces say "advisory plugin projection" with explicit basis, unobserved-runtime status and projected order/activation fields. Warning codes, MCP structured shape and canonical-state warnings remain unchanged. Installation and conflict analysis keep their original metadata/Tombstone semantics; runtime root metadata visibility and lack of Tombstone filtering are owner-approved.

Prototype evidence: 419 local Rust and 71 tooling tests, independent source review, x64 workspace and x86 dependency compile-only checks passed for the exact recorded prototype patch. These are not current-branch runtime results. The combined issue branch passed `bun run check` on macOS with 422 Rust tests and 80 tooling tests, including formatting, Clippy and dependency checks (`/tmp/mods-33-adopted-overlay-full-check.log`). Hosted Windows CI and the new candidate package/connected source rebuild subsequently passed at `3706f53` as recorded below; successful managed execution remains unverified. The original crash has not been replayed or established fixed. No design-adoption claim closes successful managed-execution/save acceptance. Publication remains disabled; #33 stays open.

All five integration style alerts were assessed: the phase-spacing finding received whitespace-only test separation; `profile.rs` comments document required non-obvious semantic contracts; parameter-order findings in CLI, MCP and the infrastructure configuration constructor are outside the application use-case rubric. No accepted finding is described as a clean automated gate verdict and no override was made. See `docs/research/issue-33-rust-overlay-prototype.md` for the exact evidence and historical limits.

### Adopted candidate verification — `3706f53`

[Windows CI 36228251263](https://github.com/Reilley64/mods/actions/runs/36228251263) passed for committed `3706f53ae3577ff6ac5daa9fb5b0478a1815f2a9`: 438 workspace tests (zero skipped), 35 i686 adapter tests and 80 tooling tests. Native release validation, pinned fetch and Rust checks passed; preview-ready and preview stayed skipped. The title check passed. Captures: `/tmp/mods-33-adopted-ci-{metadata.json,log}`.

The unchanged packaging script built an unpublished Windows 11 candidate from a new clean detached worktree at this revision. Root: `C:/Users/prime/mods-issue-33/adopted-3706f53-ba048617`.

- ZIP: `dist/mods-3706f53ae3577ff6ac5daa9fb5b0478a1815f2a9-x86_64-pc-windows-msvc.zip`, 114,314,773 bytes, SHA-256 `4c565bb8f6f53048111dd1e69e1fd0eb25ef8892f7909bc77c6f94f5e9d01573`. External checksum matched.
- Packaged source SHA-256: `6e0a690f03bcfa5edfab85273794a0c40bb6a7a7467b71744b324d08c3369014`. All 265 tracked source files, `SOURCE-REVISION.txt`, all seven native headers, runtime bytes and notices matched their exact inputs. Exact 25-file package layout passed.
- Packaged `mods.exe` SHA-256 `e28186b8c982d5040abdae7cdf800fe034ae29ffd258a2bd3d1fc24a99b9c092`; `mods-mcp.exe` SHA-256 `977f9600b43b9afbebc1f66bc461e016c863f292d0c87798bf4c3955f9e1081e`. Each executable's help/version invocation exited 0. Versions remain `0.0.0`; no stable aggregate version is claimed.
- Fresh extracted source with initially empty Cargo home and target rebuilt both Rust executables and their shim using `cargo build --release --frozen --target x86_64-pc-windows-msvc --package mods --package mods-mcp --bins` (exit 0, 3m05s). Networking stayed connected; native usvfs was not rebuilt. No missing build input, substitute dependency or failed attempt occurred. Byte reproducibility is not claimed.

Evidence: candidate root `packaging.log`, `verification-consumer.log`, `dist`, `extracted`, `consumer` and fresh build directories; local report `/tmp/mods-33-adopted-candidate.md`. No managed/native execution, game/save access, package lifecycle or protocol smoke was part of this build slice. A separate bounded portable-copy lifecycle check passed for this exact ZIP on the existing provisioned host. Evidence: `C:/Users/prime/mods-issue-33/portable-3706f53-10f3f4ac`, local report `/tmp/mods-33-adopted-portable-lifecycle.md`. A fresh disposable extraction matched all 25 package files and executable hashes. Both help/version pairs, CLI `config list --log-level off`, and MCP `tools/list` plus `mods_config_list` passed against the existing initialized fixture; all six processes exited 0 with empty stderr. MCP emitted exactly two JSON-RPC responses and returned a complete config result. The fixture inventory (including logs) and all 311 Game Installation Data files were unchanged. The installed inventory stayed identical with no reparse entries; only the new disposable installation was removed after process exit. Source ZIP and evidence remain unchanged. Two scratch path-normalization comparisons stopped before product launch; their evidence is retained and both copies were safely removed. No product command was retried. This proves only manual portable extraction/read-only startup/removal, not clean-OS, stable/Winget, managed-execution or save-routing acceptance. No publication occurred. These checks replace only the pending current-revision CI/package/source-build gates; they do not establish that the earlier native failure is fixed or that stable installation/Winget/runtime acceptance is complete.

### Owner supersession — Data runtime projection

The owner removed the runtime requirements to exclude provider-root `meta.toml` and apply existing Tombstone suppression. Those files may be visible through ordinary recursive Data overlays. Installation and conflict analysis keep their existing metadata/Tombstone rules; this change is not permission to stop validating inputs or alter canonical metadata. The analytical projection need not exactly match runtime visibility for these entries.

This resolves the two design obstacles identified by the MO2 comparison and isolated mapping-plan prototype (`prototype/issue-33-data-plan`, local commit `80920e5`). It permits further Rust-side whole-provider Data-overlay design, but is not a completed implementation or verified crash fix. Preserve Mod Priority, enabled-only participation, Overwrite, independent Output Target selection, Profile State, invalidation and save routing. Native source/pins remain unchanged and raw FFI remains bindgen-generated. See `docs/research/issue-33-view-mapping-model.md` for the scope and superseded analysis.

### Fresh local validation and reviewed scope

- Clean baseline `c195642ab4f43c7f7903c3887f54202ff17410a1`: `bun run check` passed on macOS with **415 Rust tests and 80 tooling tests** (749 assertions). `/tmp/mods-33-current-check.log` was refreshed by this continuation; it now belongs to this baseline, not the older runtime candidate. The older `/tmp/mods-33-final-local-check.log` records 414 tests and must not be cited as a 415-test run.
- After the two test-only changes below, `bun run check` passed with **417 Rust tests and 80 tooling tests** (749 assertions), including formatting, Clippy and dependency checks. Evidence: `/tmp/mods-33-final-acceptance-check.log`. This run covers the working-tree test changes atop `c195642`, not a new committed Windows artifact. No production code, dependencies, interfaces, release pins or publication gates changed.
- `src/infrastructure/game_platform/src/profile_sources.rs`, existing `profile_sources_are_read_from_known_folder_capabilities`: now compares physical before/after directory entries and file bytes for disposable profile/game fixtures, including an existing save fixture and default INI. Metadata preservation and real-user Known Folder acquisition are not asserted.
- `src/infrastructure/environment/src/lib.rs`: new `logs_only_root_is_available_and_preserved_by_initialization` proves eligibility, publication and existing-log preservation. New `publication_failure_before_cache_keeps_manifest_staged_and_refuses_initialization` uses an ordinary owned cache-file obstruction to verify partial publication, absent canonical manifest, preserved staged manifest and later `ManualCleanupRequired`. This is one manifest-last failure checkpoint, not all checkpoints, process termination, power loss or recovery.
- Focused crate tests passed (70 environment and 25 game-platform tests). Independent manual review found no blockers in the exact two-file test diff. The advisory TypeSafe style gate returned HTTP 402 (no available API credits); it supplied **no verdict**, and was neither disabled nor overridden.
- Read-only standards review at `c195642` cross-checked all 149 tracked Rust files: all 56 production unsafe blocks are confined to nine infrastructure files and have local safety proofs; all nine application use cases have the required Dependencies/Output/Error/function order and instrumentation. No concrete violations were found in these two checks. This is not blanket style approval or certification of the native implementation. Reports: `/tmp/mods-33-standards-acceptance.md`, `/tmp/mods-33-assertion-work.md`, `/tmp/mods-33-assertion-review.md`.

### Retained Windows CI, now checked at individual-test level

[Run 36112545582](https://github.com/Reilley64/mods/actions/runs/36112545582) has successful `check` and skipped `preview`, PR head `23d5b9ae1a5ffad556de593eb619381228de4f39`. Raw logs identify checkout merge `5cf6d55` of that head into `86e9563`. They show **431 workspace tests passed, zero skipped**, and **31 i686 execution-adapter tests passed**. Individual PASS records include profile acquisition/cancellation/symlink rejection, environment/settings reparse rejection, and packaged single-session ownership on both architectures. This reuses hosted Windows Server 2022 evidence; it is not Windows 11 successful managed execution, a main-push preview result, or Windows validation of the new test-only changes. Captures: `/tmp/mods-33-retained-windows-ci.log` and `/tmp/mods-33-retained-windows-ci-metadata.json`.

### Credited automated non-execution evidence

The fresh local suite verifies the following existing colocated seams. These close the named automated scenarios, not every historical grouped matrix row or platform-specific branch.

| Scenario | Colocated evidence (paths under `src/`) | Limit |
|---|---|---|
| Profile import/seed/empty-list/no-save policy | `infrastructure/environment/src/profile.rs`: `stages_import_seed_empty_absent_and_never_saves`, `stages_exact_save_routing_and_removes_conflicts_from_imports` | Configuration/staging policy, not actual save interception |
| Initialization stage/cancel/refusal | `infrastructure/environment/src/lib.rs`: `staged_initialization_has_no_canonical_mutation`, `cancellation_before_publication_preserves_the_complete_stage`, `pending_operation_takes_precedence_and_refuses_initialization`, plus new assertions above | No crash/power-loss guarantee |
| Install interruption and subsequent refusal | `infrastructure/environment/src/transactions.rs`: `a_mid_publication_failure_keeps_stage_and_backups_then_refuses_later_mutation`, `cancellation_preserves_staging_without_canonical_mutation`, `postcommit_cancellation_is_not_observed` | Ordinary synthetic failure and cancellation; no restoration/retry |
| Settings publication/cancel/refusal | `infrastructure/settings/src/manifest_writer.rs`: `failed_staged_validation_preserves_operation_state`, `final_cancellation_preserves_the_validated_stage`, `cleanup_failure_after_commit_returns_success_and_leaves_refusal_state` | Mods-owned publication only |
| Pending reads/dry-run never repair | `infrastructure/environment/src/snapshot.rs`: `preview_rejects_unfinished_work_as_invalid_without_mutation`, `unfinished_work_precedes_missing_canonical_layout_without_mutation`; `infrastructure/settings/src/lib.rs`: `unfinished_work_blocks_read_without_mutating` | No repair or cleanup claimed |
| Disposable logs/cache and invalid/partial roots | `infrastructure/settings/src/lib.rs`: `disposable_cache_and_logs_never_change_settings_behavior`, `invalid_flat_manifests_fail_unchanged`, `missing_or_partial_root_is_reported_as_folder_uninitialized` | Settings behavior, not package removal |
| Presentation diagnostic sessions | Both `presentation/{cli,mcp}/src/diagnostics.rs`: `off_creates_no_log_file_or_root`, `normal_session_creates_one_uuid_file_with_boundary_events`, `configured_log_level_filters_captured_project_events`; CLI `runner.rs`: `appender_setup_failure_warns_without_changing_the_command_result` | Packaged MCP setup-failure evidence is recorded separately below |
| MCP admission/cancellation/progress | `presentation/mcp/src/server.rs`: `busy_admission_precedes_validation_and_releases_without_queueing`; `cancellation.rs`: `queued_progress_is_dropped_but_committed_response_finishes`, `cancellation_suppresses_response_while_transport_drains`; `application/src/installation/install_archive.rs`: `progress_reports_real_installation_checkpoints` | Protocol/application seams, not native-child lifecycle |
| Archive, install and conflict policies | Existing real ZIP/7z/RAR/FOMOD adapter fixtures; transaction replacement tests; conflict projection/whole-query failure tests; `presentation/mcp/src/install_output.rs`: `nonempty_install_plans_match_preview_and_installed_contracts` | Supplement existing Windows CLI/MCP scenarios; no native interception claim |

The supporting 21-row source audit is `/tmp/mods-33-remaining-scenario-audit.md`. Its description of `current-check.log` as historical 82467db evidence is superseded by the fresh-run identity above. Source presence alone is not a test pass; non-Windows local tests do not establish Windows-only behavior.

### Fresh Windows packaged diagnostics — six passing scenarios

On 2026-09-26 UTC, the approved Windows host ran three CLI and three MCP scenarios from verified candidate `82467db4679587d698a2a6f953bfdae1543bfe62`. Fresh hashes matched `mods.exe` `9dffa75bbad027f58012f7af17cb896a45547f758675517a7ce7c9ad3164fa1f`, `mods-mcp.exe` `62809fa88d26fe18f6615782ec4a72061d43158d62206c64c63ec8c8c7cd1fc0`, and ZIP `3a2fedb31e029b234b18f0c7e4508988422ce1679941d0878d049fbe9c32ac31`.

Each mode used a separate fresh empty root. CLI `config list` and MCP `mods_config_list` intentionally returned `environment_not_initialized`; MCP also successfully served `tools/list` (eight tools). Results:

- `info`: expected UUID JSON diagnostic files and boundary records. CLI had started/failed records; MCP had request started/failed and lifecycle started/completed records.
- `off`: no log path or diagnostic identifier, unchanged operation results.
- Owned regular file obstructing `environment/logs`: unchanged obstruction, nonfatal warning, unchanged operation results. CLI emitted exactly one sink warning; MCP emitted two (lifecycle/request). All MCP stdout remained exactly two valid JSON-RPC responses, with no trailing output.

All CLI exits were the expected 1; all MCP servers exited 0. All six task-owned PIDs exited without timeout or cleanup. No game/Steam/save access, native mapping, managed execution, existing-environment mutation or system change occurred. Enabled coverage used info only; successful initialized CLI config and native-execution diagnostic boundaries are not claimed.

Retained host evidence: `C:/Users/prime/mods-issue-33/diagnostics-safe-23e620af`, including `identity.json`, six scenario argv/status/stream captures, literal MCP requests/responses, `assertions.json`, `evidence-inventory.json`, diagnostic JSONL files and `process-completion.txt`. Local report: `/tmp/mods-33-diagnostics-acceptance.md`.

### Bounded measurements and second owner-approved archive move

The first corrected timing attempt produced three conflict-list and three conflict-inspect samples, then stopped at a normal replacement preview rejection (`environment_invalid`, phase `settings_load`). Read-only inspection found that the reserved `Data/Fallout - Invalidation.bsa` existed again (83 bytes). Install-state validation rejects that filename; conflict scanning uses a different policy. The old diagnostic did not identify the precise rejecting branch. No product regression or relationship to the native crash was established.

The owner explicitly approved another move. Only that file was moved, without replacement, to `C:/Users/prime/mods-issue-33/owner-approved-backup-3ea490229474/Fallout - Invalidation.bsa`. Source and backup SHA-256 matched `ee54ebdd90c485bce3b3c0151e193d40b37b3278a14decabc133f9afa8487f57`; `record.json` retains original metadata. Full Data hash inventories bracketed this move: 312 files before, 311 after, with only the approved file removed and every other file unchanged. The earlier backup remains untouched. This is an intentional Data change, not historical no-change evidence or a save inventory.

After the move, three normal and three FOMOD replacement dry-run previews passed with exit 0, `outcome = "preview"`, expected choices/winners and zero warnings. No install commit ran. All six sample inventories and final inventory matched the 25-entry owned canonical baseline (excluding expected logs); fixture hashes were unchanged, temp was empty and no mods/mods-mcp processes remained.

| Read-only operation | Three wall-clock samples (ms) | Result |
|---|---|---|
| Conflict list with content comparison | 1311.9129 / 84.1802 / 93.4032 | Three successful samples before the second move |
| Inspect SmokeA conflicts with content comparison | 84.5038 / 86.7629 / 82.4363 | Three successful samples before the second move |
| Normal replacement preview | 346.3320 / 347.6372 / 332.4133 | Three successful samples after the approved move |
| FOMOD red-choice replacement preview | 338.2776 / 327.2513 / 346.4949 | Three successful samples after the approved move |

Measurements used the same verified `82467db` executable/ZIP identities recorded above on Windows `10.0.26200.0` x64, PowerShell 7.6.6, 12 logical processors. Each sample includes fresh process startup and info logging, but excludes SSH/preflight/inventory. There was no warmup, cold-cache guarantee, invented numeric threshold or inferred cause for the slower first query. The archive fixtures are only 288/287/661 bytes; these observations do not establish large-workload, install-commit, native execution or game performance.

Evidence: `C:/Users/prime/mods-issue-33/safe-smoke-20260925-02/performance-20260925-02` (query samples and stopped preview), `C:/Users/prime/mods-issue-33/approved-preview-3ea490229474` (move inventories, successful preview argv/streams/status/timings and canonical comparisons), and the new backup metadata. Local reports: `/tmp/mods-33-performance-retry.md`, `/tmp/mods-33-preview-settings-diagnosis.md`, `/tmp/mods-33-approved-archive-move-and-preview.md`. Two pre-product orchestration errors (ZIP-member assumption and later runner quoting) are retained separately; neither is a product defect or successful sample. No further remote work remains active.

### Current committed candidate — connected rebuild passed

At exact commit `bc0c7e6b5931d934c6e47eacddffc99840eb74f9`, [Windows CI 36221889613](https://github.com/Reilley64/mods/actions/runs/36221889613) passed: 433 workspace tests (zero skipped), 31 i686 adapter tests and 80 tooling tests. Native release validation, pinned fetch and Rust checks passed; preview-ready and preview stayed skipped. Title check passed. No release workflow was enabled.

A new clean detached worktree on the approved Windows 11 host ran the unchanged `scripts/package-windows.ps1`. Package build and verification passed. Candidate root: `C:/Users/prime/mods-issue-33/current-bc0c7e6-9aeee20b`.

- ZIP: `dist/mods-bc0c7e6b5931d934c6e47eacddffc99840eb74f9-x86_64-pc-windows-msvc.zip`; 114,264,713 bytes; SHA-256 `d21a1313f1e90973667c38d36a737bf10a618b986fe31205270bd100de6f2004`. External `SHA256SUMS` matched.
- Packaged `mods-source.tar.gz` SHA-256: `394592624c4923f9c23ee41bad876517b8393f257c0b1d4b6a8bcbf9436c10aa`. All 260 tracked source files matched `git archive` of the exact revision. `SOURCE-REVISION.txt` and all seven required native headers matched. Vendoring/source replacement configuration was retained.
- Exact 25-file package layout passed: both executables, exactly four native runtimes, root license/copyright/build instructions/source archive, eight upstream license files and seven native-release notices. Runtime and notice bytes matched the verified inputs; no metadata marker or external checksum was incorrectly shipped inside the ZIP.
- Both packaged executables returned exit 0 for `--help` and `--version`. Presentation versions remain `0.0.0`, not stable aggregate-version evidence.
- Fresh source extraction used initially empty Cargo home and target directories. `cargo build --release --frozen --target x86_64-pc-windows-msvc --package mods --package mods-mcp --bins` passed in 3m09s with networking connected. This compiled both Rust packages and their shim from packaged source without a missing input or substitute dependency; it did not rebuild native usvfs. No byte-identical reproducibility or disconnected result is claimed.
- Packaged `mods.exe` SHA-256: `b65de8d40a686a59cd81c4eacee79312d69efac9d4270660f1141acb978c4b2e`; packaged `mods-mcp.exe`: `62aeea60da0f7dc44b9046f2912ac005bb5fc5476584eb7a1ace3438aa21ef6a`. Rebuilt executable hashes differ and are retained in the report; byte reproducibility is not an acceptance claim.

Tools were already installed: PowerShell 7.6.6, Git 2.55.0.windows.3, bsdtar 3.8.8, nightly Rust 2026-08-21, LLVM 23.1.2, VS 2022 17.14.37710.0 and SDK 10.0.26100.0. The exact native ZIP/source hashes remain pinned and unchanged. The included native source is the same previously audited 16,347-file/66-resource archive; no new native rebuild, original collector report, or unconditional source-completeness certification is inferred.

Evidence under the candidate root: `packaging.log`, `verification-consumer.log`, `verification-consumer-resume.log`, `dist`, `extracted`, `consumer`, `consumer-cargo-home` and `consumer-target`. Local report `/tmp/mods-33-current-candidate.md` records commands, hashes and scratch verification corrections. CI captures are `/tmp/mods-33-waiver-ci-{metadata.json,log}`. The worktree was clean before/after. Earlier 82467db Windows runtime scenarios remain evidence for that older artifact; only help/version was run from this new package. No game/managed execution, install/remove trial, source/native edit, networking change, publication or Winget action occurred.

### Owner supersession — disconnected rebuild check waived

The owner approved removing and skipping the disconnected Windows Rust/native rebuild check. Keep networking connected on `prime@192.168.86.155`; no disconnected environment, isolation method or offline recovery path is required for #33. This supersedes all older disconnected-rebuild gates in this ledger and `docs/distribution.md`. Record the check as **waived/skipped**, not passed.

Complete corresponding-source packaging, notices and source/hash verification remain required. Existing connected and origin-blocked build evidence keeps its original limits. The original collector report remains unavailable; no replacement was fabricated. Its recovery is no longer an acceptance blocker solely for the skipped disconnected trial. Any future use of the native validator must still satisfy its genuine input contract. All four mandatory helper archives were recovered and hash-verified; details are in `/tmp/mods-33-build-input-recovery.md`.

This waiver does not change native code, generated bindings, release pins, publication gates, stable lifecycle or Winget requirements. No host networking was changed. #33 remains open.

The earlier local-checkpoint pending-CI statement is also superseded: pushed commit `4226ada6c236e279ed71448ce3824830d5a078c7` passed [Windows CI 36221269020](https://github.com/Reilley64/mods/actions/runs/36221269020), with 433 workspace tests, 31 i686 adapter tests and 80 tooling tests; preview stayed skipped. These results cover that checkpoint, not a new runtime package or this later documentation-only waiver.

### Distribution provenance and remaining gates

Read-only inspection of original native [run 36086098511](https://github.com/Reilley64/usvfs-rs/actions/runs/36086098511), exact revision `eb4949fb2439fe5b98901e2fb1afceee752a6133`, confirms successful packaging required a genuine matching collector report. The workflow uploaded only three packaged files, not its `usvfs-stage` or `reports/source-collection-status.json`. Logs do not contain that report's bytes; earlier locally retained collector reports bind different source hashes. The sole nonexpired release-preparation artifact does not recover the missing stage. This is an evidence-retention gap, not proof that library source is missing. No substitute collector report was created. Details: `/tmp/mods-33-collector-provenance.md`, `/tmp/mods-33-remaining-distribution-audit.md`.

Still required before full acceptance:

1. Successful managed execution, x86/x64 child/descendant lifecycle, cancellation/output and actual save routing. The native crash remains unresolved; no Rust binding defect or justified Rust-only fix was found. No fault replay or native edit was performed in this continuation.
2. Complete corresponding-source and notice verification with accurately identified build evidence. The disconnected Rust/native trial is waived as recorded above; neither host disconnection nor recovery of the collector report solely for that trial is required. Do not relabel connected/origin-blocked evidence as disconnected evidence.
3. Clean stable ZIP install/run/remove and broader representative-workload measurements. Small-fixture conflict and preview timings are now recorded above; installation-commit and native/game performance remain unmeasured. Do not infer clean removal or stable-artifact behavior from candidate extraction/help or these previews.
4. Approved Release Please/version outcome, real stable ZIP URL/hash, real Winget manifest/validation/install/remove and human-controlled initial submission. Aggregate remains `0.0.0`; publication gates remain disabled. No release, dispatch, merge, submission, fabricated manifest or credentials claim was made.
5. Final artifact-specific runtime sign-off. Windows CI and candidate packaging/connected source rebuild passed at adopted revision `3706f53` as recorded above. Earlier broad CLI/MCP runtime scenarios still identify `82467db`; do not relabel those as new-package runtime results. Keep #33 open and PR #103 partial/draft. No stable publication occurred.

## Previous status — 2026-09-25 reconciliation

- Reviewed HEAD: `23d5b9ae1a5ffad556de593eb619381228de4f39`. The worktree was clean before this documentation reconciliation. Only this ledger changed between runtime candidate `82467db4679587d698a2a6f953bfdae1543bfe62` and that HEAD. This establishes unchanged-code traceability, not a new-head package: the repaired ZIP remains an **82467db artifact**, SHA-256 `3a2fedb31e029b234b18f0c7e4508988422ce1679941d0878d049fbe9c32ac31`.
- Current-head checks reported successful by the completion audit: [Windows CI](https://github.com/Reilley64/mods/actions/runs/36112545582/job/107999068031), [repository tools](https://github.com/Reilley64/mods/actions/runs/36112545789/job/107999068663), and [title](https://github.com/Reilley64/mods/actions/runs/36112543814/job/107999064853). Preview was intentionally skipped. Follow-up inspection of the run’s step metadata confirmed successful native-release validation, pinned-native fetch, x86 adapter compile/tests and Rust checks; these steps were not skipped. Raw step logs were not re-audited, and CI does not establish Windows 11 runtime acceptance. The earlier repair check recorded 415 Rust tests and 79 tooling tests; it is not a newly run HEAD check.
- Candidate packaging, checksum/layout, both executable help/version checks, and representative Windows 11 CLI initialization/settings/install/conflict scenarios already have passing evidence. The repaired MCP request and subsequent bounded scenarios passed on Windows; the old pending-replay statement below is superseded by “Fixed Windows MCP replay.” These results must not be repeated or left wholly pending merely because the initial matrix predates them.

### Reconciled scenario evidence

Existing Windows observations were performed by the agent through the `prime` account on the approved Windows 11 host (Steam build `1510068`). Paths below are relative to `C:/Users/prime/mods-issue-33`. They identify retained evidence, not new execution by this documentation reconciliation.

| Scenario | Candidate, result and retained evidence | Limit |
|---|---|---|
| Fresh owned initialization; five settings list/get/set | `35072e9`; exit 0, manifest provenance and quiet CLI mutation success; `safe-smoke-20260925-02/evidence` CLI argument/stream/status captures | After approved archive move; existing user INI/plugin import not covered |
| Normal archive preview and commits | `35072e9` CLI captures above; `82467db` MCP requests 5–7 in `safe-smoke-20260925-02/evidence-82467db`; preview unchanged, commits installed | Representative `normal-a.zip` / `normal-b.zip`, not all archive edge cases |
| FOMOD missing choice, red preview/commit, blue replacement | Same CLI evidence; `82467db` MCP requests 8–11; no publication for incomplete/preview, actual red/blue content, replacement modlist preserved | Representative `fomod.zip`, not interruption/failure coverage |
| Inactive and active conflict list/inspect/explain | Same CLI evidence; `82467db` MCP requests 12–17; SmokeB priority 1 wins over SmokeA priority 0, `different_sha256` | Fixture enablement changed only owned modlists; no runtime interception claim |
| MCP inventory/config/strict arguments | `82467db` requests 18–21 and 23–25; eight schemas/annotations, structured config success, invalid enum/unknown field/null rejected | Inventory is not successful execution coverage |
| MCP execution rejection | `82467db` request 22; empty program rejected, status 126 | No child launched; successful execution remains blocked |

Fixture hashes are recorded in the retained fixture inventory and `/tmp/mods-33-windows-safe-smoke-retry.md`; the fixed replay verified the same bytes. All 16 repaired MCP sessions drained streams and exited 0. The exact failed ID 5 request is preserved alongside its successful replay. Reports: `/tmp/mods-33-windows-mcp-fixed.md`, `/tmp/mods-33-windows-safe-smoke-retry.md`, `/tmp/mods-33-native-source-audit.md`, `/tmp/mods-33-final-local-check.log`, and `/tmp/mods-33-completion-audit.md`. Local `/tmp` reports are supporting session records, not published artifacts; retained Windows paths above and below locate the underlying captures.

### Remaining acceptance gates

- Successful managed execution remains blocked by the native access violation. Matching-symbol evidence localizes null traversal state but does not establish its cause or preceding branch. No speculative normalization, omitted mapping, native fix or pin change is justified or applied. Prepared Debug builds below add no runtime evidence; the requested null-transition observation was not run.
- Still missing: applicable profile import, per-presentation diagnostics off/setup-failure, interruption/pending-state refusal, execution/descendant lifetime/cancellation/output, save routing, representative performance and clean install/remove evidence. Existing test sources and grouped rows still need complete scenario-level reconciliation, including precise automated evidence and unsafe/local-proof/declaration-order review coverage. Do not require duplicate runtime work where sufficient automated evidence exists, or infer that all gaps require game launch.
- Previous 311-file Data comparisons use the **post-approved-move** baseline. The absent standard save path neither proves save routing nor covers every save location. No fresh Data/save inventory was taken for this reconciliation or the Debug build preparation.
- Source inventory passed. The pinned native ZIP also contains exact-source x86/x64 rebuild reports recording `succeeded-with-origin-blocking`, matching package identities and four rebuilt output hashes; these are retained release evidence, not a newly witnessed build. Online diagnostic native builds also exist. Neither origin blocking nor these diagnostic builds proves disconnected Rust/native rebuild closure. The older empty-Cargo-home consumer rebuild is network-connected and belongs to `35072e9`, not the repaired ZIP. The native supported validator still needs genuine collector-stage evidence and its documented provisioned inputs; no substitute report or harness was fabricated.
- Shared packaging, disabled stable publication and stable-first Winget update wiring already exist. Their existence is not a release outcome. Preview/stable/Winget publication gates remain disabled; aggregate version is still `0.0.0`. Approved Release Please/version outcome, actual stable URL/hash, real Winget manifest/human-controlled initial submission and clean installation/removal remain unproved and cannot be completed under the current no-publication boundary. Credentials were not checked and are not a proven blocker.
- Local preview-readiness repair composes the existing repository-tool checks into CI, forces Rust checks on main pushes, and requires successful same-revision Rust/tool checks plus a main-push event before readiness. Preview, stable and Winget remain unconditionally disabled. Focused policy tests passed (9 tests, 59 assertions); `bun run check:tools` passed (80 tests, 749 assertions); `git diff --check` and bounded read-only review passed. This is local/static validation, not a hosted workflow run; actionlint was unavailable. Reports: `/tmp/mods-33-preview-guards.md` and `/tmp/mods-33-preview-review.md`.
- Keep #33 open and PR #103 partial/draft. These records do not waive any applicable handoff criterion. No game/runtime/remote work or tracker changes were performed for this reconciliation.

## Historical packaging checkpoint

- Base revision: `86e9563bc5422f9afce47046fdc9df17c51a08d1`; packaging changes are not yet committed.
- Shared candidate packaging script: `scripts/package-windows.ps1`; instructions: `docs/distribution.md`. Publication remains disabled.
- Local `bun run check` passed on macOS after stable workflow additions: 414 Rust tests and 79 tooling tests (735 assertions), plus formatting, Clippy and dependency checks. `git diff --check` passed. These results are not a Windows packaging run.
- Packaging review found one missing-header blocker, fixed and re-reviewed with no remaining blockers. Stable/Winget review found no concrete in-scope blockers. Reviews do not replace platform execution.
- Pinned-toolchain offline Cargo vendoring and extraction of the hashed native source headers were checked on macOS. All seven headers matched fork revision `eb4949fb2439fe5b98901e2fb1afceee752a6133`, and the vendored build resolves its expected include path.
- Blocked: Windows PowerShell packaging, MSVC/LLVM compilation, clean disconnected consumer/native rebuilds, packaged runtime smoke, Windows 11 Steam scenarios, public assets and Winget installation/removal.
- Final evidence must identify the committed revision and link retained logs/artifacts; the local checks above ran against uncommitted work, not a clean final checkout.

## Committed Windows CI evidence

Candidate revision: `35072e9dbed1308b6fda68c825f776b1cf79a0d2` ([draft PR #103](https://github.com/Reilley64/mods/pull/103)).

- [Windows CI](https://github.com/Reilley64/mods/actions/runs/36100347155): passed, including native release validation, pinned native fetch, x86 adapter tests, formatting, Clippy, dependency checks and workspace tests.
- [Repository tools](https://github.com/Reilley64/mods/actions/runs/36100347346): passed.
- [Conventional Commit title](https://github.com/Reilley64/mods/actions/runs/36100347280): passed.
- Preview publication was skipped by its intentional gate. These jobs do not prove packaged-binary behavior or a disconnected source rebuild.
- A separate Windows 11 host has the pinned Rust toolchain, Visual Studio C++ tools, Windows SDK and Steam Fallout: New Vegas build `1510068`. Candidate packaging validation is in progress in an isolated user workspace; game files and saves have not been changed.

## Windows 11 candidate packaging evidence

Candidate revision: `35072e9dbed1308b6fda68c825f776b1cf79a0d2`. The isolated Windows 11 `10.0.26200` workspace ran the unchanged `scripts/package-windows.ps1` from a clean checkout.

- Candidate ZIP SHA-256: `d760379881f469d1e1acfbea86cc04e036c68c6a291e6d1e1cd3b0b0729d72ae`; external `SHA256SUMS` matched.
- Fresh extraction verified both executables, exactly four native runtimes, notices and packaged source.
- Both executable help/version invocations returned exit 0. Their presentation versions are `0.0.0`; this is not a stable aggregate-version assertion.
- Packaged source rebuilt successfully with an initially empty Cargo home and fresh target directory, using `cargo build --release --frozen --target x86_64-pc-windows-msvc --package mods --package mods-mcp --bins` and the included local native ZIP. All seven required headers and pinned native source hash were verified.
- Network remained connected. This is **not** disconnected rebuild proof, and it does not rebuild native usvfs.
- Logs remain under `C:/Users/prime/mods-issue-33`: `packaging.log`, `smoke.log`, `consumer-rebuild.log`, and `final-evidence.log`. The candidate remains local to the approved host; no publication occurred.

## Windows initialization blocker

The first packaged CLI initialization returned `game_install_invalid` (exit 1) for `C:/Games/Steam/steamapps/common/Fallout New Vegas`. The selected owned environment contains only diagnostic logs; no manifest/profile/mods were published. No retry or cleanup was performed.

Read-only inspection found `Data/Fallout - Invalidation.bsa` (83 bytes). `src/infrastructure/game_platform/src/steam/validation.rs` explicitly rejects that reserved filename. Required executable/default INI and path directories were present with no reported links. This observed reserved archive is sufficient to fail the current validation contract; no product fix is justified by this observation.

Before/after content-hash inventories of all 312 Steam Data files were byte-identical. The standard save directory for the test user was absent both times; other save locations were not asserted covered. Evidence remains under `C:/Users/prime/mods-issue-33/safe-smoke-20260925-01`.

Downstream settings/install/conflict/MCP acceptance is blocked pending an approved clean installation or explicit owner action on the conflicting existing archive. The archive was not moved, deleted or modified.

## Owner-approved reserved archive move

After the first initialization failure, the owner approved moving only the conflicting 83-byte archive. It was moved to `C:/Users/prime/mods-issue-33/owner-approved-backup-20260925/Fallout - Invalidation.bsa`; `record.json` alongside it records the original path and metadata. SHA-256 before and after: `ee54ebdd90c485bce3b3c0151e193d40b37b3278a14decabc133f9afa8487f57`. The backup is retained, not deleted or restored.

A fresh `safe-smoke-20260925-02` environment initialized successfully (exit 0). Subsequent safety comparisons use a new baseline **after** the owner-approved archive move; they must not imply that the original Steam Data inventory was never changed. Further CLI/MCP observations are in progress.

## CLI acceptance and MCP defect repair

On candidate `35072e9`, the `safe-smoke-20260925-02` run passed CLI settings list/get/set; normal archive preview and commit; FOMOD incomplete choices, preview, commit and replacement; and inactive/active conflict list/inspect/explain. Ordinary successful mutations were quiet, previews left owned mod state unchanged, and replacement preserved the modlist. Only disposable environment modlists were edited to enable fixture providers. Steam Data/save inventories before and after the run were identical (311 Data files after the approved archive move).

MCP advertised exactly eight tools with input/output schemas and annotations. Config list/get/set passed with structured results. The first normal install preview failed with JSON-RPC `-32603`, `tool output violated its contract`; the commit request was not sent. The environment remained empty with no pending state. Full failed request/response and CLI logs remain under the `-02` evidence directory. That failed server session's stderr/exit was not captured; no clean exit is inferred from it.

Source diagnosis identified missing `kind: "archive_candidate"` on candidate `proposed_winner`. Fix `82467db4679587d698a2a6f953bfdae1543bfe62` adds only that production field and a colocated nonempty-plan regression covering preview and installed output schemas. The test failed for both outputs before the fix and passed afterward. Full local `bun run check` passed: 415 Rust tests and 79 tooling tests. Manual Standards and Spec re-review passed; the advisory Jev coding-style service returned no verdict and was not overridden. Windows package replay is pending; local tests alone do not close the observed acceptance defect.

## Fixed Windows MCP replay

Candidate `82467db4679587d698a2a6f953bfdae1543bfe62` passed a fresh full Windows package build. ZIP SHA-256: `3a2fedb31e029b234b18f0c7e4508988422ce1679941d0878d049fbe9c32ac31`. Source revision, fresh extraction, checksum/layout and both help/version checks passed. [Windows CI](https://github.com/Reilley64/mods/actions/runs/36102669901) and [tooling CI](https://github.com/Reilley64/mods/actions/runs/36102669882) passed for this revision.

The exact previously failing MCP request (same ID 5, original environment and fixture bytes) now returned `preview` with both candidate-reference discriminators. It did not publish mod state. Subsequent normal commits, FOMOD incomplete/red preview/red commit/blue replacement, inactive and active conflict queries, config tools, eight-tool schema/annotation inventory, and strict invalid-argument cases passed. Replacement preserved the modlist. All 16 MCP sessions drained stdout/stderr and exited 0; protocol responses remained JSON-RPC.

`mods_exec` was covered only by an empty-program rejection (`program_unsupported`, status 126). No successful execution, descendants, cancellation or save routing is claimed. All 311 Steam Data files matched the after-approved-move baseline before and after these observations; the standard save path remained absent. No pending state or running MCP server remained. The reserved archive backup was unchanged.

Evidence: `C:/Users/prime/mods-issue-33/packaging-82467db.log`, `dist-82467db`, `extracted-82467db`, and `safe-smoke-20260925-02/evidence-82467db` (literal requests/responses, session streams/statuses, fixture/state observations and inventories). The prior empty-Cargo-home consumer rebuild applies to the older candidate, not this ZIP. Disconnected and native source rebuild evidence remains missing. The observed candidate-discriminator defect is resolved by real Windows replay, not only unit tests.

## Controlled execution blocker

The owner approved controlled x86/x64 read-only tools, descendants and cancellation (not game launch). On candidate `82467db`, a fresh environment initialized successfully. The first and only managed-execution command targeted the verified x64 `C:/Windows/System32/whoami.exe` with no arguments and an owned workspace cwd.

The controller `mods.exe` crashed with unsigned exit `3221225477` (`0xC0000005`, access violation); stdout/stderr were empty. Windows Application Error event 1000 identified packaged `usvfs_x64.dll`, fault offset `0x60210`, report `c81582db-a053-4386-b8b9-1bcbe372b42e`. The execution diagnostic ended after effective plugin configuration without completion. This is not successful child execution or graceful fail-closed evidence. Root cause remains unconfirmed.

The test stopped immediately: no x86/MCP execution, descendant or cancellation tests ran. No retry, cleanup, debugger, system changes or native repair occurred. Final process inspection found no test/game processes. All 311 Steam Data hashes matched the post-approved-move baseline, and the standard save path remained absent.

Evidence is retained at `C:/Users/prime/mods-issue-33/execution-smoke-20260925-01/evidence` (exact argv, status, streams, Windows crash events, PE/hashes, process and file inventories). Successful execution acceptance remains blocked pending focused diagnosis and an approved repair/retest plan.

## Debugger diagnosis evidence

The SSH account has an enabled administrator token. Registering the existing WinDbg package for that account succeeded but did not remove the WindowsApps CDB access denial. The owner then approved standalone Debugging Tools installation. The Microsoft-signed cached SDK installer installed only `OptionId.WindowsDesktopDebuggers` with quiet/no-restart options; exit 0, no reboot. Standalone CDB `10.0.26100.7705` works. No WindowsApps permissions were changed.

Analysis of the verified original dump localized the crash to directory mapping (`usvfsVirtualLinkDirectoryStatic+0x8e` calling the faulting code at DLL RVA `0x60210`). Export-nearest `userDisconnected+0x7370` is not a reliable source function name. Matching private symbols were unavailable.

One owner-approved debugger replay then logged two directory entries: Overwrite to Game Installation Data (flags 4), followed by an identity mapping of `Data\Music` (flags 0). Both identity-mapping inputs were verbatim-prefixed Windows paths. The first access violation followed the second entry and matched the original RVA/read-address `0x60`. This confirms the input and boundary, not the root cause. The first mapping's return was not independently logged; reaching entry 2 is the observation.

The debugger saved a full dump and quit without resuming after the fault. Live `.ecxr` was unavailable; current stopped registers/stack were captured instead. Debugger timing/heap/stdio differences are recorded, so exact non-debugger equivalence is not claimed. No further replay occurred. Final checks found no test/debugger processes; all 311 Data hashes were unchanged and the standard save directory remained absent.

Evidence: `execution-smoke-20260925-01/debug-standalone-parent` and `debug-replay-directory-20260925-01` beneath the existing isolated workspace. Replay dump SHA-256: `2f0fe712a791ca677666b742116abeea1e8ea9e67064adf4b1a97f0977655e32`. Read-only review of the native mapping input requirements is in progress; no native/source repair is approved or claimed.

Read-only mapping-contract review found no documented prohibition on verbatim-prefixed paths or identity directory mappings, so neither was treated as an established mods defect. It identified different native path-decomposition code in parent traversal versus insertion as an unconfirmed lead. No speculative normalization, mapping omission or native patch was applied. Further diagnosis requires an approved symbol-enabled native diagnostic build/comparison plan; it must not substitute rebuilt symbols for the original DLL or silently change the release pin.

## Symbol-enabled native comparison

The owner approved a separate symbol-enabled diagnostic build and one comparison. The exact archived fork/upstream source was built with its existing Release configuration, which already enables full PDB output. Both architectures were built solely to satisfy the unchanged four-artifact interface. A separate controller from clean `82467db` embedded the real diagnostic artifact hashes. Original package/source/pins stayed unchanged. This was an online build, not offline source-closure evidence.

The diagnostic DLL's CodeView GUID/age matched its private PDB. The single comparison reproduced the same second directory mapping, read address `0x60`, instruction and DLL RVA `0x60210`. Matching symbols identify `assertPathExists` at `src/usvfs_dll/usvfs.cpp:591`, inlined `DirectoryTree::exists`, called by `usvfsVirtualLinkDirectoryStatic` at line 726. This source localization applies to the matched diagnostic binary; its PDB was not substituted for the original DLL.

Subsequent offline inspection of the same full dump recovered `current = NULL`, iterator component `Games` at position 7, destination parent Data, and verbatim-prefixed Data/Music paths. The prior branch/returned node was not recoverable, so the suspected path-decomposition mismatch remains unproved. No normalization, skipped mapping, null-check patch or other native/product fix has been applied.

Evidence is retained in `C:/Users/prime/mods-issue-33/native-symbols-20260925-01`, including build logs, matching PDBs, separate controller and comparison dump (SHA-256 `c52a8f09341df690705a06ce90b7b7660edb88bb4b4529f75e12fbc504cbe2ea`). All 304 archived fork files and original package hashes remain unchanged. All 311 Steam Data hashes match; standard saves remain absent and relevant processes are gone. Additional controlled runtime comparisons require owner approval.

## Isolated Debug build preparation — builds only

Separate x64/x86 native Debug DLL/proxy builds and an x64 Rust debug controller from clean `82467db4679587d698a2a6f953bfdae1543bfe62` succeeded in `C:/Users/prime/mods-issue-33/debug-builds-20260925-02`. The native source identities remained fork `eb4949fb2439fe5b98901e2fb1afceee752a6133` and upstream `57f1ea5e6ad13f7435a7af184748e6c1312c5637`. Native builds used the existing Debug configuration; the Rust controller used its normal dev profile and a fresh target directory. No production source, workflow or release pin changed.

All five binary/PDB CodeView GUID/age pairs matched (age 1): `mods.exe`/`mods.pdb`, both `usvfs_x64.dll`/`usvfs_x64.pdb` and `usvfs_x86.dll`/`usvfs_x86.pdb`, and both architecture-specific `usvfs_proxy_*.exe`/`usvfs_proxy_*.pdb` pairs. Symbols are retained under `diagnostic/pdb` and native `install-*/pdb`; matching records are in `verification/pdb-matches.json`. These PDBs belong only to the new Debug binaries, never the prior Release/package DLLs.

All 304 archived fork files in the fresh extraction and prior Release workspace were unchanged. All 25 original extracted-package files and 38 prior Release installed/staged files matched inventories. Independent before/after hashes also preserved the original controller, x64 DLL, candidate ZIP, corresponding-source archive and reserved archive backup. Build scripts did not access Steam/save paths; no fresh Data/save hash claim is made.

Native resolution was cache-assisted with network available, not enforced offline or proven zero-network. The Rust controller used `--offline --locked` with the existing Cargo home; this is not a fresh disconnected source-closure rebuild. Debug CRT files were observed present, but no loader test ran. No product executable, debugger target, game or runtime comparison was launched. Debug CRT/assertions/heap/layout/timing differ from Release, so no byte-identical replay or new root-cause finding is implied.

Evidence: the workspace build logs/statuses, `build-manifest.json`, `output-hashes.json`, verification files and final process inventory; local report `/tmp/mods-33-debug-builds.md`. The subsequent `/tmp/mods-33-debug-null-transition.md` records source reads only: the requested live transition observation was **not run**. No fresh binary identity, process or game/save inventories were collected by that source-read task. The missing last-assignment/branch evidence and native root cause remain unresolved; no speculative fix was applied.

## Native source inventory evidence

The published native source asset SHA-256 matched `961478a1e69cf6b0156e78970181ef6375974aaadd437af5fdfe8e905db85199`. A local archive audit verified all 16,347 declared files, all 66 source-map resource SHA-512 values, and the nested fork revision/file inventory. No missing mapped library-source asset was identified. This is inventory evidence, not a native rebuild result or an unconditional source-completeness certification.

The verified pinned native ZIP (SHA-256 `bcee5841ff291f7cdd68b128020a0358e21a4eaafc70c58efab8c0ab6c447a8a`) contains `release-evidence/rebuild-inputs.json` binding the exact source SHA above and original run `36086098511`. Its `source-rebuild-status.json` records `succeeded-with-origin-blocking` for x86 and x64. Package-identity arrays match, and four rebuilt output hashes are recorded. This corrects earlier broad claims that native rebuild evidence was absent. These cached asset reports do not prove a disconnected host build, independently witnessed execution, or byte-reproducible PE outputs; byte reproducibility is explicitly not claimed. Audit details: `/tmp/mods-33-static-readiness.md`.

The existing native `packaging/rebuild-sources.ps1` requires collector-stage inputs not contained in the released source archive: a genuine collection-status report, provisioned helper tools/archives and bootstrapped vcpkg. The downloaded release-preparation artifact contains the native ZIP, source archive and release manifest, but not that collector-stage report. The existing native rebuild path therefore still needs its documented inputs; no prior-run report has been fabricated and no replacement harness has been added.

## Authority and supersessions

Read via `gh`: issues [33](https://github.com/Reilley64/mods/issues/33), [25](https://github.com/Reilley64/mods/issues/25), [32](https://github.com/Reilley64/mods/issues/32), including comments (none), and the [authoritative acceptance gist](https://gist.github.com/Reilley64/76e35e2187eb5f0aff543c228a6de39f).

Apply these explicit replacements before using the old handoff:

- Two presentation-owned binaries in a virtual workspace; `mods-mcp.exe` starts stdio directly. No root Rust package or `mods mcp start`.
- Tokio/Tokio Util are permitted in application. Direct CancellationToken replaces OperationControl.
- Mods-owned mutations make one ordered publication attempt. Failure/cancellation leaves partial work for inspection; later mutations refuse with manual_cleanup_required. No restoration, retry, resume, rollback, roll-forward, or cancellation settlement. Read-only work does not repair pending state.
- Direct ordered FOMOD choices only. Reviewed Choice Files, images, decorative metadata and public evaluator traces are deferred.
- Colocated Rust unit tests only. Do not add a Rust integration/built-binary test target or new injection harness.
- CLI ordinary mutation success remains quiet. IMPORTANT: the final #32 supersession selects structured mutation success and output-schema advertisement for all eight MCP tools. It overrides older bodyless MCP wording in #25/#33. Do not write a smoke assertion requiring empty MCP config-set/install results. Include annotations and actual committed facts.
- Diagnostics are dependency-default tracing-subscriber JSON and synchronous tracing-appender files, UUID session files and boundary records. No OTel schema mapping, custom validation/redaction/retention, producer identity, sink/filesystem hardening, exact deletion or exporter work.
- No MCP rate limiter. Preserve nonqueueing environment_busy guard. Valid UTF-8 child output is complete and unchanged; binary output remains byte count/SHA-256. environment_invalid has no details, including read tools.
- ADR-0005/#25 accepts unmodified upstream usvfs behavior. Modified-fork semantic guarantees, COW, durable execution Tombstones, opaque namespaces, virtual timestamp enforcement, exact handshake and fail-closed descendant interception are superseded. Keep mods-owned configuration, trusted artifact loading, ownership, supervision and failure mapping. Source packaging still covers the exact pinned usvfs-rs integration/shim and upstream source.

## Historical grouped acceptance matrix (initial audit snapshot)

This matrix preserves the initial audit chronology, including then-pending work subsequently evidenced above. It is not the current gap list. Use “Current status” and the later dated/candidate checkpoints to resolve stale pending claims; untouched rows do not imply completion. Full scenario-level sign-off remains incomplete.

Paths below are relative to repository root. `src/` prefixes are significant. Each final evidence row should record commit, scenario, fixture identity, actual result, linked CI/log artifact, platform and operator. Split these grouped rows into individual handoff bullets for final sign-off.

| Applicable scenario group | Existing candidate evidence | Remaining evidence/blocker |
|---|---|---|
| Windows x64 pinned build; fmt/Clippy/tests; graph; two binary composition roots; framework separation; unsafe reviews; declaration order | `.github/workflows/ci.yml`, `package.json`, `tests/dependency-graph.test.ts`, `scripts/check-dependency-graph.ts`, colocated Rust tests | Clean final-revision `bun run check` and all Windows jobs with URLs; static review record for unsafe/local proofs and Dependencies/Output/Error/function order. Windows-2022 CI is not Windows 11 game evidence. |
| Init default/selected root, publish manifest last, partial-root handling, logs-only eligibility | `src/infrastructure/environment/src/lib.rs` initialization/staging tests; CLI runner root tests; settings partial-root tests | Link passing Windows tests; record clean Steam initialization including logs-only root. |
| Explicit/env/Steam/registry precedence; App ID/build/exe validation; containment | game_platform `resolution.rs`, `steam/validation.rs`, `separation.rs`, `bound_game.rs` | Real Steam installation discovery and observed build evidence; record no import-source modifications. |
| INI/plugin import, missing INI seed, empty/missing lists, never import saves | environment `profile.rs`: `stages_import_seed_empty_absent_and_never_saves`, save-routing cases | Controlled import fixture before/after inventory. |
| Five settings/provenance/shadowing; game-dir publication; effective nonpersistent override; unknown MODS variables | settings `lib.rs`, `config_source.rs`, `manifest_writer.rs`; application settings tests; CLI `output.rs` | Passing tests and package smoke list/get/set, quiet CLI mutation success. No invented MODS_ENVIRONMENT/MODS_LOG/RUST_LOG setting. |
| Disposable cache/log independence; schema rejection; one-attempt publication/cancellation; read-only refusal without repair | settings `lib.rs`; environment `transactions.rs`, `snapshot.rs`, `execution_preparation.rs`; application install cancellation tests | Interruption/failure evidence and pending-state before/after inventory. Never label these recovery tests. Cross-process mutation remains unsupported/unlocked. |
| ZIP/7z/RAR/content-sniffed FOMOD; unsafe formats/paths/semantics rejection; hardened XML; expansion safeguards | archive `adapter.rs`, `index.rs`, `path.rs`, `fomod.rs`, `zip.rs`, `seven_zip.rs`, `rar.rs`, `extract.rs`, `source.rs` | Link fixture tests; representative normal/FOMOD package smoke on Windows. Do not duplicate dependency parser internals. |
| Derived/override mod name, new disabled highest priority, replace preserving state, complete/incomplete choices, dry-run, precedence/warnings, conditional xNVSE reads | application installation tests; domain installation tests; environment transaction tests; CLI runner/output and MCP install_output tests | Recorded normal/new/replace/incomplete/complete/dry-run evidence, unchanged state on dry-run, actual structured MCP mutation output. Replacement is ordered one-attempt publication, not guaranteed rollback on failure. |
| Deterministic loose-file/tombstone conflicts; enabled priority/Overwrite; opaque file extensions/no BSA inspection; hypothetical disabled providers; optional SHA; unavailable content; whole-query I/O failure; no paging/filter/mutations | application `conflicts/projection.rs`; environment `conflict_scan.rs`; domain provider-resolution tests; CLI/MCP conflict output tests | Link passing tests and representative overlapping-mod smoke. Conflict Tombstone modeling is not a promise of runtime deletion interception. |
| Exact native artifacts/x86+x64 ABI, trusted loading, safe wrapper ownership and setup failures | `native/usvfs-release.json`, `native/fetch.tests.ps1`, `tests/native-release.test.ts`; execution `usvfs/mod.rs`, `configuration.rs`, `process.rs`; CI x86 adapter step | Final pinned release + consumer x86/x64 Windows job evidence. Ship both DLLs/proxies. Runtime requires x86 and x64 VC++ redistributables. Do not use upstream version label as source identity. |
| Mods-computed read winners and Output Target; profile named-file mapping; invalidation BSA; save route | execution `configuration.rs`, `profile.rs`; `docs/research/issue-31-execution-composition.md` | Controlled Windows 11 Steam/game/tool configuration smoke and save route observations. Do not assert opaque hiding, synthetic timestamp enforcement or no physical Steam writes by arbitrary managed applications. Reuse upstream interception evidence rather than repeat its suite. |
| Direct exec separator/quoting/env/cwd, no shell/detach/elevation/fallback, root status 32-bit, 125/126/127, Job drain, Ctrl+C/cancellation, CLI streams | execution `launch_inputs.rs`, `windows_inputs.rs`, `managed.rs`, `process.rs`; application execute tests; CLI commands/runner/error tests | Actual x86/x64 tools and descendant lifetime smoke, root status and cancellation evidence. Fail-closed checks cover reported mods-owned setup/lifecycle/artifact failures, not unreported descendant hook failure. |
| Eight MCP tools/version/schemas/annotations; strict args and safe field paths; missing root; complete unpaged output; guard/no limiter | MCP `contract.rs`, `inputs.rs`, `server.rs`, `settings_output.rs`, `install_output.rs`, `error_output.rs` | Packaged stdio client smoke of all eight tools and JSON-RPC-only stdout. Capture structured mutation success and no environment_invalid details. |
| MCP cancellation response/progress linearization; increasing progress; child EOF/private concurrent capture; valid unchanged text/binary metadata; cleanup supervision | MCP `cancellation.rs`, `lifecycle.rs`, `execution_output.rs`; execution `child_output.rs`; application install progress test | Windows controlled cancellation and child output evidence; no redaction/size-cap requirement. |
| UUID default JSON diagnostics, off creates none, setup warning nonfatal, no product log commands/exporter | both presentation `diagnostics.rs`; CLI runner `appender_setup_failure_warns_without_changing_the_command_result`; ADR-0003 | Per-presentation packaged session/off/setup-failure evidence. Test project boundary behavior only, not old OTel/redaction/retention promises. |
| CLI data/warnings/error/cancellation statuses, no JSON mode, quiet mutation success; exclusions | CLI commands/output/publication/error tests; MCP advertised inventory tests | Archive help and tool inventory; ensure no excluded advertised feature. Document warnings for accepted upstream limitations. |
| Bun shared check; Conventional Commit squash title; aggregate release beginning 0.1.0 | package scripts; `tests/pr-title.test.ts`, `tests/internal-release-tags.test.ts`, Release Please workflow/config and ADR-0004 | Actual clean-check URL, title-check URL, approved release PR/version outcome. Static config is not proof of release publication. |
| Tagless SHA preview; stable ZIP/checksum; both EXEs/runtime/notices/full corresponding source | CI disabled preview recipe, native README/pins/notices, native release tests | Preview is explicitly `if: false` pending verified combined source. The candidate script includes vendored Rust source and pinned native source/headers; actual Windows rebuild proof is missing. A disabled stable package/checksum publication workflow is now prepared; it has not executed. Need reproducible source closure including Rust and native static dependencies/build inputs before enabling any binary publication. |
| Stable Winget Reilley64.Mods URL/hash, silent install/uninstall, stable-first human-controlled WingetCreate submission, no self-update | Disabled stable-first Winget update workflow and deduplication policy prepared; real first manifest and Windows validation remain pending | Build/validate manifest only after real stable URL/hash. Record human-controlled submission, install both executables and uninstall on clean Windows target. No fabricated URL, checksum, PR, approval or installation evidence. |
| ZIP install/run CLI + stdio MCP/remove without altering managed Steam Data | Packaging requirement; not yet linked runtime evidence | Controlled clean Windows 11 target, published ZIP hash, prerequisites, both binaries, source/notices, install/remove before-after Data snapshot. This package lifecycle requirement must not be generalized into the superseded arbitrary managed-write protection guarantee. |
| Representative performance and complete acceptance ledger | No numeric product threshold; handoff explicitly defers tuning | Record measured scenarios and environment; no invented pass/fail numeric target. Follow up only on observed problems. |

## Historical sign-off blockers and suggested order

These recommendations and the read-only audit disclaimer below describe the initial audit, not this reconciliation. Existing workflow wiring and later CI/package results supersede their stale pending wording; current unresolved gates are listed above.

1. Do not close #33 based on unit-test presence. Link final clean-check and Windows jobs, then attach scenario-level evidence.
2. Resolve complete corresponding-source packaging before enabling the currently gated preview or publishing stable binaries. Include full Rust dependency source, exact pinned native source/shim/build material and notices; a registry reference is not source.
3. Complete stable ZIP/checksum and stable-first Winget human submission wiring. Preserve the settled aggregate Release Please model rather than redesigning release automation.
4. Obtain controlled Windows 11 + clean Steam evidence for both packaged executables, representative installation/config/conflicts/profile/save behavior, MCP tools/cancellation, mods-owned launch failure seams and uninstall. Record accepted upstream limitations, not silently waived former guarantees.
5. Retain blocked/manual states for unavailable Windows, publication, credentials or approval work. This audit made no repository edits, ran no tests, and did not inspect remote CI pass history.
