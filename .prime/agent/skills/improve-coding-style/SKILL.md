---
name: improve-coding-style
description: Scan existing Rust for coding-style violations, present verified fix batches as a visual HTML report, then fix the batches you pick.
disable-model-invocation: true
---

# Improve coding style

Find existing Rust that breaks the coding style and repair it in behavior-preserving **batches**. The coding-style gate reviews new edits. This skill is the retroactive pass over code that predates a rule or got past the gate.

Terms used below:

- A **candidate** is a possible violation from any scan source. It is not a verdict.
- A **finding** is a candidate you verified against the rubric.
- A **batch** is the findings for one rubric item in one cohesive area, small enough to review as one change.

The rubric is the set of area files under `docs/coding-style/`; `CODING_STYLE.md` indexes them and holds the `Authority` order. Each `###` item has `Rule`, `Violation`, `Compliant`, and two examples. An item with an `Applies to` list covers only files that match one of its globs. A candidate becomes a finding only when the item covers the file, the code meets the `Violation` boundary, and no `Compliant` clause covers it. Resolve conflicts with the `Authority` order. A rule ID is the `##` section title and the `###` item title as one kebab-case slug. The gate uses the same IDs as keys in `ruleThresholds` in `.prime/agent/coding-style-gate.json`.

## Process

### 1. Scope

Start from a clean branch. Run `git status`. If the worktree has changes that you did not make, stop and ask. `docs/agents/ticket-scope.md` allows one writer per worktree.

Decide where to look before you look:

- If the user named a direction (paths, crates, rubric sections, or rule IDs), use it and skip the rest of this list.
- Otherwise, read `git log -p -- docs/coding-style CODING_STYLE.md`. Code written before a rule existed is the most likely to break it, so check recently added or changed rules first.
- Then walk back `git log --oneline` to find hot spots, the files that keep changing. Put these first.
- Do a repository-wide sweep only when the user asks for one.

The scope is handwritten Rust from `git ls-files '*.rs'`. Generated bindings and vendored sources are out of scope. Record the base commit OID.

Read `CODING_STYLE.md`, every area file that covers the code in scope, `CONTEXT.md`, the ADRs in `docs/adr/` for the area, and `docs/agents/ticket-scope.md`.

### 2. Scan

Collect candidates from three sources:

1. **Deterministic tools.** Run `bun run format:check`, `bun run lint`, and `bun run dependency:check`. They are the authority for the facts they check. Their failures become their own first batch.
2. **Jev range review (optional).** This uses live OpenRouter credit and needs `OPENROUTER_API_KEY`, so ask before you run it. It reviews only the lines that a commit range changed, plus 20 lines of context. Code that the range did not touch is not reviewed.

   ```text
   bun ./.prime/agent/extensions/coding-style-gate/calibration/review-range.ts <base-oid> <head-oid> [rule-id] [exact-path ...] > "$TMPDIR/coding-style-range-<timestamp>.json"
   ```

3. **Rubric sweep.** Spawn read-only reviewer subagents in parallel. Give each one area file from `docs/coding-style/` (combine small files) and the files in scope that its items cover. Use the brief in [REVIEWER.md](REVIEWER.md).

Verify every candidate yourself. Open the code, quote the `Violation` clause that it meets, and test each `Compliant` clause. Drop each false positive with a one-line reason, and keep the dropped list for the report. These false-positive classes recur in this repository:

- The application use-case rules (declaration order, parameters, focused orchestration, and use-case-local modules) cover only `src/application/`, and within it only use-case entry points. Port declarations, parent `mod.rs` re-exports, and test fixtures are outside them.
- A call to an inherent adapter factory method that constructs a port is not a callable-port invocation.
- An explicit `match` or `if let` stays when a branch has side effects or does more than one conversion, such as keeping the first error and continuing. A branch that only converts or defaults a value is a real finding under "Prefer Option and Result combinators".
- Comments that carry a safety proof, compatibility rationale, caller contract, or external constraint stay.

The scan is complete when a reviewer has read each file in scope against each rubric item that covers it, and you have verified or dropped each candidate.

### 3. Present batches as an HTML report

Group the findings into batches. Write the report to `<tmpdir>/coding-style-review-<timestamp>.html`, where `<tmpdir>` is `$TMPDIR` or `/tmp`. Open it with `open <path>` and tell the user the absolute path. [REPORT.md](REPORT.md) gives the format.

Each batch card has the rule ID, files and line ranges, a before/after snippet, a risk class, and a strength:

- **Risk.** `mechanical` batches change layout, imports, names, or comments. `structural` batches change signatures, module placement, control flow, or error handling.
- **Strength.** `Strong` is a clear breach. `Worth fixing` is a real breach with a small payoff. `Borderline` is near the boundary; name the `Compliant` clause that almost covers it.

End the report with the dropped candidates and a top recommendation. Do not edit code yet. Ask the user: "Which batches would you like to fix?"

### 4. Fix loop

For each chosen batch:

1. Freeze a scope contract as `docs/agents/ticket-scope.md` describes. By default, these stay unchanged: behavior, public APIs, error and cancellation semantics, file formats, resource limits, FFI ownership, and user-visible messages. A finding that needs one of these changes is a follow-up, not a style fix.
2. If a structural batch has more than one reasonable shape, such as how to split a use case or where a module goes, use the `grilling` skill to settle the shape with the user. Edit mechanical batches directly.
3. Make the edits. The coding-style gate reviews each edit. Fix each gate finding, or record its disposition with the file, rule, and reason.
4. Run focused checks for the affected crates: `cargo nextest run -p <crate>`, `bun run format:check`, and `bun run lint`. Run `bun run check` once before the pull request. Code behind `cfg(windows)` also needs `cargo check --workspace --target x86_64-pc-windows-msvc`, or CI if that target is not available.
5. Run one repair and re-review cycle for the remaining blockers. If blockers remain, stop and ask the user.

Use the `refactor:` type for commits and pull request titles.

Handle these cases as they come up:

- **The rule is wrong in practice.** The compliant form reads worse, or a valid pattern keeps tripping the rule. Offer to amend the rule in its area file instead of forcing the code. A changed item keeps all five fields, and it needs new calibration fixtures and an explicit threshold (see the gate README).
- **Jev was wrong.** For a confirmed Jev false positive or miss, offer a paired regression fixture in `.prime/agent/extensions/coding-style-gate/calibration/per-rule-cases.ts`.
- **A fix contradicts an ADR.** Flag it as `docs/agents/domain.md` describes. Do not override the ADR silently.

### 5. Record

Write `docs/research/<slug>.md` for the pull request. Include the base OID, coverage by area with exclusions, the cleanup done, follow-ups not done, a disposition for each gate finding, and the check results. `docs/research/repository-style-cleanup.md` is a complete example.
