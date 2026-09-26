# Issue 33: Rust Data-overlay prototype

## Owner adoption

The owner adopted this design into the issue #33 candidate after the isolated prototype, diagnostics update and source/compile reviews. Commits `51efc0f`, `b74a2dd` and `88d559b` were integrated as `7fb99ae`, `ba1b28f` and `b3fe423`. Runtime root-metadata exclusion and Tombstone filtering remain waived; installation/conflict analysis keeps its existing rules. Native usvfs and generated bindings remain unchanged.

This supersedes the historical "do not adopt/merge" recommendations below: those paragraphs record the experiment's earlier status. Adoption is a design decision, not proof of successful managed execution, crash repair or release readiness. Current-branch CI, candidate build and runtime evidence must identify their actual revisions. Publication remains disabled.

During integration, an advisory phase-spacing finding prompted only additional blank lines between test phases; non-whitespace Rust content remains identical to the reviewed prototype. Other advisory findings were assessed explicitly: profile comments carry necessary analysis/runtime contracts; the CLI/MCP presentation functions and infrastructure configuration constructor are outside the application use-case parameter-order rule. No gate override was used.

## Frozen scope and owner decision

This isolated prototype starts at `origin/main` `86e9563` on
`prototype/issue-33-rust-overlays`. It changes only `configuration.rs`, the safe
`usvfs/mod.rs` adapter, and this note. The public `ViewConfiguration::new`
signature is unchanged. No native source, ABI, generated binding, dependency,
pin, composition, environment, or profile projection changes are included.

The owner superseded runtime provider-root `meta.toml` exclusion and runtime
Tombstone filtering. Whole-root runtime mappings can therefore expose entries
that the analytical projection excludes. Installation and conflict rules and
all existing configuration input validation remain in force.

## Mapping plan

- Clear the existing executable/suffix bypass lists before mapping.
- Sort enabled non-base Data providers by `ProviderIdentity::rank` ascending.
  Overwrite is last. Steam Data remains physical fallback, not a mapped root.
- Recursively link each provider root to Data. Mark only the chosen Output
  Target with the existing recursive `create_target` call at its ordinary rank.
  Default target is Overwrite. Selection does not promote a provider's read rank.
- Emit no per-file Data winner links or Data-only parent directory identities.
- Preserve caller Profile State directory links as nonrecursive, then all named
  file mappings (including the caller-supplied invalidation mapping), then the
  existing recursive save target. Caller order and paths remain unchanged.
- Stop immediately on the first adapter error.

Winners are still checked for enabled/existing provider identities, duplicate
comparison keys, and valid joined mapping paths. Provider identity/name and all
root, profile, Data destination, and save path checks remain. Even an empty
winner list maps enabled provider roots. The existing analytical visible-file
computation and profile plugin projections remain unchanged; this is a mapping
prototype, not a replacement analytical namespace implementation.

The private directory-link seam now accepts recursion. The Windows safe wrapper
selects the existing bindgen `LINKFLAG_RECURSIVE` constant; it does not add target
flags to ordinary directory calls or change raw declarations. Safety proofs and
session ownership remain in place. Merge, read, and write behavior remain owned
by upstream usvfs, not a local simulator.

## Evidence and limits

- `cargo test -p infrastructure-execution configuration::tests`: seven recorder tests passed. They cover unsorted providers, disabled/base exclusion, default/low/high target rank, unchanged caller Profile State/invalidation/save calls, empty winners, early failures, invalid references, duplicate winners/providers/names and invalid mapping paths.
- `bun run check` on macOS: **418 Rust tests and 71 tooling tests passed**, plus formatting, Clippy and dependency checks. These counts belong to this prototype's `86e9563` base, not the separate issue branch. Log: `/tmp/mods-33-rust-overlay-full-check.log`.
- Independent manual standards/scope review: **PASS**, no blockers or code follow-ups. Report: `/tmp/mods-33-rust-overlay-review.md`. The advisory TypeSafe gate had no verdict because API credits were unavailable (HTTP 402); no override was used.
- Windows compile-only checks passed for both targets: `cargo check --offline --locked --package infrastructure-execution --all-targets --target x86_64-pc-windows-msvc` (29.85s) and the same command for `i686-pc-windows-msvc` (9.95s). No tests or product/native DLL entry points ran. Standard Cargo build scripts compiled the unchanged shim and generated bindings from unchanged pinned headers.
- Windows verified exact base `86e9563bc5422f9afce47046fdc9df17c51a08d1` and Rust patch SHA-256 `733861a24e45ddf14a577ea0cfd7c4ba62451c085c84573fd0c25342c90a6397` before and after checking. Only the two permitted Rust paths changed. Existing native archive hashes, source marker and all seven cached dependency headers matched the pinned corresponding-source archive. No dependency download, installation or native source change was needed.

Windows evidence: `C:/Users/prime/mods-issue-33/overlay-check-7bf809fd`, with successful `resume3.log`, exact `overlay.patch`, fresh detached `repo` and target directory. Local report `/tmp/mods-33-rust-overlay-windows-check.md` records tool versions, hashes and three corrected scratch-verification assumptions. All corrections occurred before Cargo and changed no project code; no Cargo compilation failed.

These tests inspect project-owned plans and compile Windows code; they do not establish upstream runtime behavior, performance, design adoption, or crash repair. No native mapping, game launch, debugger or fault replay ran. The separate issue worktree's existing files and uncommitted evidence were preserved byte-for-byte. No source or pin change was applied outside this isolated prototype.

## Decision and handoff

The Rust design can express the approved ordered-overlay policy through the existing generated ABI, without changing Profile State/save composition or native usvfs. Source review, recorder tests and both Windows typechecks passed. That supports keeping the prototype as a concrete design candidate, not accepting a repair or merging it into the product.

Capture this code only on `prototype/issue-33-rust-overlays`; do not merge or publish it as a release. Successful managed execution remains a separate unverified acceptance requirement. No further repair cycle was needed for this bounded implementation.

Next: review the isolated implementation before deciding whether to carry the design forward. Do not describe compilation or recorder results as proof that the original crash is fixed.

## Design review follow-up: spacing and plugin diagnostics

The design-candidate review found one minor standards issue: phase spacing in configuration validation and recorder fixtures. Seven blank lines were added; all non-whitespace source content is identical to `51efc0f`. `cargo fmt --all -- --check`, the seven focused configuration tests and `git diff --check` passed. Independent bounded re-review confirmed the original blocker resolved. Reports: `/tmp/mods-33-overlay-spacing-tests.log`, `/tmp/mods-33-overlay-spacing-review.md`. Earlier full-suite and Windows compile evidence belongs to the pre-spacing patch; it was not rerun for this whitespace-only correction.

### Diagnostic policy decision (implemented; runtime acceptance pending)

Keep the existing analytical plugin projection and qualify every diagnostic surface. It is advisory analysis, not an observed game/usvfs view. It may differ from runtime visibility in membership, activation sources and ordering after the runtime metadata/Tombstone waiver. Do not call it "effective plugin configuration" or imply that the runtime ignores an entry absent from that projection.

The current consumer uses the computed plugin list for tracing and warnings only. Provider-root and canonical Profile State mappings do not depend on that list. Execution does not rewrite canonical plugin lists from the projection. Therefore explicit, complete qualification is sufficient for this consumer; a second namespace scan is not needed merely to produce these diagnostics. A future consumer that selects runtime plugins or rewrites Profile State would require a new design review.

Recommended common explanation:

> Plugin diagnostics use the analytical Data projection, not an observed runtime view. Mappings use canonical Profile State; this computed list does not change those files. Projected plugin order is advisory and is not enforced through virtual timestamps.

Per-entry messages should say:

- Missing from analytical Data: identify the source list and name; state that runtime availability is not established. Do not say the game will ignore the plugin.
- Unlisted plugin: state that the name is absent from `loadorder.txt` and backing-file modification time determines its position in the analysis. Do not claim runtime order.
- Duplicate list entry: state that analysis uses the first occurrence and does not change the canonical file.
- Tracing: label the list "advisory plugin projection", qualify order/activation as projected, and identify `basis=analytical_data` and `runtime_observed=false` rather than implying observed visibility.
- Post-run Profile State validation warnings remain separate; they are not analytical plugin-availability warnings.

Preserve existing warning variants/codes and MCP response shape. Equivalent qualifications appear in CLI text, MCP text and trace events, rather than only renaming a heading while leaving absolute claims elsewhere. The separately approved diagnostic-only update applies these qualifications to CLI and MCP warning text and native tracing. Existing warning codes, variants, MCP structured schema, empty structured warnings array, child output, and status are preserved.

Implemented diagnostic-only scope: existing warning rendering in CLI `runner.rs`, MCP `execution_output.rs`, and tracing in dependencies `execution_adapter/native.rs`; clarify projection-only contracts in `profile.rs` and `execution_preparation.rs`; update focused presentation assertions and the existing MCP contract-delta note. No algorithm, canonical-state, mapping, native, binding, pin or public-schema change. `/tmp/mods-33-plugin-diagnostic-policy.md` records the reviewed call chain and rejected second-projection alternative.

### Diagnostic update validation

- Focused CLI checks: three tests passed using the existing fake execution port. MCP renderer check: one test passed, covering all warning variants and the unchanged structured result.
- Correct prototype checkout: `bun run check` passed with **419 Rust tests and 71 tooling tests**, plus formatting, Clippy and dependency checks. Evidence: `/tmp/mods-33-diagnostic-update-prototype-full-check.log`. An earlier command ran in the repository root; `/tmp/mods-33-diagnostic-update-full-check.log` is discarded as evidence for this prototype.
- Independent bounded review passed with no blockers: `/tmp/mods-33-diagnostic-update-review.md`. The three live style-gate findings were inspected, not overridden. The `profile.rs` documentation states externally required analysis/runtime/canonical-state contracts; it does not excuse tangled logic. The CLI dispatcher and MCP renderer signatures are unchanged presentation functions, so the application use-case parameter-order rule does not apply.
- Windows compile-only checks passed: `cargo check --offline --locked --workspace --all-targets --target x86_64-pc-windows-msvc` (50.15s) and `cargo check --offline --locked --package infrastructure-dependencies --all-targets --target i686-pc-windows-msvc` (17.97s). Both compiled test targets without running them. These results now cover the Windows-only tracing change as well as the overlay adapter.
- Exact base `86e9563bc5422f9afce47046fdc9df17c51a08d1`, combined seven-Rust-file patch SHA-256 `41c52675510fad2f2f53e8d12a30f59101e14ed3d8b926e422f697235ac9db93`, and final unchanged diff were verified. Pinned native archives, source marker, cached dependency revision and seven headers matched. No generated binding, native source or dependency was manually changed.

Windows evidence: `C:/Users/prime/mods-issue-33/diagnostic-check-4812a048`; local report `/tmp/mods-33-diagnostic-update-windows-check.md`, full output `/tmp/diagnostic-check-4812a048-output.txt`. No tests/product/native entry points, mapping calls, game launch, debugger or fault replay ran on Windows. All remote commands finished.

The approved diagnostic-only edit is complete in the isolated prototype. Next: review the candidate for any further adoption decision, keeping successful managed execution as a separate unverified requirement. No merge, release publication or crash-repair claim follows from these checks.
