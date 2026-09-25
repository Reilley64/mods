# Issue 33 acceptance evidence ledger

Status: incomplete. This ledger starts from a read-only acceptance audit. Existing test sources below are candidate evidence, not passing results. Grouped requirements must be split into individual handoff scenarios with linked results before sign-off. No controlled Windows 11 sign-off is recorded.

## Packaging checkpoint

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

## Native source inventory evidence

The published native source asset SHA-256 matched `961478a1e69cf6b0156e78970181ef6375974aaadd437af5fdfe8e905db85199`. A local archive audit verified all 16,347 declared files, all 66 source-map resource SHA-512 values, and the nested fork revision/file inventory. No missing mapped library-source asset was identified. This is inventory evidence, not a native rebuild result or an unconditional source-completeness certification.

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

## Acceptance matrix (pending scenario-level results)

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

## Sign-off blockers and suggested order

1. Do not close #33 based on unit-test presence. Link final clean-check and Windows jobs, then attach scenario-level evidence.
2. Resolve complete corresponding-source packaging before enabling the currently gated preview or publishing stable binaries. Include full Rust dependency source, exact pinned native source/shim/build material and notices; a registry reference is not source.
3. Complete stable ZIP/checksum and stable-first Winget human submission wiring. Preserve the settled aggregate Release Please model rather than redesigning release automation.
4. Obtain controlled Windows 11 + clean Steam evidence for both packaged executables, representative installation/config/conflicts/profile/save behavior, MCP tools/cancellation, mods-owned launch failure seams and uninstall. Record accepted upstream limitations, not silently waived former guarantees.
5. Retain blocked/manual states for unavailable Windows, publication, credentials or approval work. This audit made no repository edits, ran no tests, and did not inspect remote CI pass history.
