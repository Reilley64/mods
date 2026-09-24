# coding-style-gate

`coding-style-gate` is a project-local Prime Agent extension. It reviews Rust files changed by agent tools against `CODING_STYLE.md` with TypeSafe Jev.

## Behavior

The extension takes a snapshot of tracked and untracked, non-ignored Rust files before `ipython`, `edit`, or `bash` runs. It takes another snapshot after the tool completes and reviews only the resulting task-local changes. Existing dirty files that the tool does not change are not included.

By default, each snapshot covers every worktree returned by `git worktree list` for the session repository. This includes edits made through absolute paths after an agent changes its process directory without changing Prime's session root. Each watched worktree uses its own review settings and style file; the session root still controls which roots are watched and whether findings are advisory or enforced. Explicit `additionalRoots` can include unrelated Git repositories. Every additional root must be an absolute path to the repository root; nested directories and arbitrary filesystem paths are rejected.

Likely violations are appended to the tool result, so the agent sees them before its next action. In both modes, `agent_end` reviews the complete task-baseline-to-current diff. Advisory mode reports the final result without blocking. Enforce mode retains earlier violations after later clean edits and catches changes that are present when the final snapshot is taken.

Enforcement queues a task-bounded number of correction turns. If the code still fails, the extension asks the user to intervene instead of starting an infinite loop. A user can accept the exact current policy-and-code fingerprint with `/coding-style-gate override <reason>`. Any relevant code, policy, model, or threshold change invalidates that override.

The extension does not revert files. `rustfmt`, rustc, Clippy, and repository tests remain deterministic checks outside Jev.

## Setup

Install repository dependencies:

```text
bun install
```

Set the API key in the environment that starts Prime Agent:

```text
export TYPESAFE_API_KEY=...
```

Do not put the key in this repository or in a Prime conversation.

Prime discovers the extension from `.prime/agent/extensions/coding-style-gate/index.ts`. Use `/reload` after changing its source or configuration.


## Rubric format

Every `###` item in `CODING_STYLE.md` supplies five `####` fields:

- `Rule`: the normative requirement;
- `Violation`: the positive Noul decision boundary;
- `Compliant`: the negative Noul decision boundary;
- `Bad example`: representative input for the violation criterion;
- `Good example`: representative input for the compliant criterion.

The gate submits every item to Jev. Independent rubric questions that share a patch are sent together in one request, subject to TypeSafe's request token budget. Repository tools remain the authority for facts that `rustfmt`, rustc, Clippy, Cargo metadata, or tests can establish.

## Configuration

`.prime/agent/coding-style-gate.json` contains:

- `enabled`: enables tool and final checks;
- `mode`: `advisory` reports tool and final-task findings, while `enforce` also starts bounded correction turns and keeps the snapshot marked blocked until it passes or the user overrides it;
- `model`: pinned Jev model ID;
- `threshold`: default minimum Noul violation probability;
- `ruleThresholds`: calibrated thresholds for rubric boundaries with a distinct score distribution;
- `styleFile`: project-relative policy file;
- `tools`: tool names observed for filesystem changes;
- `timeoutMs`: timeout for each TypeSafe attempt;
- `maxConcurrency`: maximum number of file reviews in flight;
- `maxFollowUps`: maximum automatic correction turns before the gate asks the user to intervene.
- `worktreeScope`: `registered` watches every registered worktree; `session` watches only the session root.
- `additionalRoots`: up to 16 absolute Git repository roots to watch in addition to the selected worktree scope.

Start in `advisory` mode. Calibrate rules and thresholds with representative good, bad, and exception cases before switching to `enforce`.

## Commands

```text
/coding-style-gate status
/coding-style-gate check
/coding-style-gate reset
/coding-style-gate override <reason>
```

`check` reviews the complete task-local Rust diff. `reset` accepts the current filesystem state as the new task baseline without calling Jev. `override` accepts only the current code and policy fingerprint and records the reason.

## Data sent to TypeSafe

Each request contains:

- the changed Rust file path;
- a unified diff with 20 context lines around each hunk;
- rubric rules, decision boundaries, and examples extracted from `CODING_STYLE.md`;
- paths of Rust files that reference the changed module name, with no unchanged source text.

The extension does not upload the complete resulting file. It rejects patches larger than 100,000 characters rather than silently truncating them. It skips Rust symlinks and rejects a policy file that resolves outside the project.

Tracked Rust files and untracked, non-ignored Rust files in watched roots are eligible. Non-Rust files, unchanged dirty files, and untracked ignored files are not sent. A new or deleted Rust file is necessarily represented in full by its patch. Redaction is not a substitute for keeping secrets out of source code.


## Calibration

The labeled corpus lives in `calibration/cases.ts`. `calibration/comment-cases.ts` adds real comment excerpts and explicitly marked redundant-comment mutations. `calibration/comment-audit.json` preserves the source audit, including ambiguous examples excluded from calibration labels. Run the production model against every rubric item with:

```text
bun ./.prime/agent/extensions/coding-style-gate/calibration/run.ts
```

This is a live API command. It requires `TYPESAFE_API_KEY` and incurs TypeSafe usage. Normal tests use MSW and never call the live API.

The current `jev-1.13.0` calibration uses default threshold `0.86`, a `0.70` threshold for reason comments, and a `0.45` threshold for use-case-local module placement. See `calibration/comment-results.md` for the measured comment calibration and its limits. Re-run calibration when the model, rubric, prompt shape, evidence fields, or fixture corpus changes.

## Tests

Tests use MSW to intercept the official TypeSafe SDK. They do not call the live API:

```text
bun test ./.prime/agent/extensions/coding-style-gate/tests
```

## Persistent review receipts

The gate persists `coding-style-gate-receipt` custom entries in the Prime session
JSONL through `pi.appendEntry`. These entries survive session reloads and do not
add source patches or review prompts to model context.

Completed tool-result, agent-end, and manual `check` reviews record:

- schema version and timestamp;
- trigger, repository root, and tool name/call ID when applicable;
- review fingerprint, model, reviewed files, and finding count;
- finding file, rule ID, and probability;
- `cachedFiles`, distinguishing reused results from new requests.

`outcome: "reviewed"` with zero findings is an affirmative clean review, not a
skipped hook. Calls with no relevant Rust changes do not emit a review receipt.
A repeated review can emit another receipt while reusing its cached result.

Failures record `outcome: "failed"` and a stage, without provider error bodies or
credentials. Before-tool snapshot failures are remembered by tool-call ID and
reported explicitly in that tool's result. After-tool, baseline, and final
snapshot failures also produce explicit messages. A failed snapshot is never
represented as a clean review.

Interactive advisory notifications are also persisted as `coding-style-gate`
session messages. `/coding-style-gate status` still reports the current in-memory
report/error; use the session receipts for historical coverage. Receipts cannot
reconstruct reviews that happened before this change was loaded.

Reload the agent extensions (or start a new agent session) to activate changes to
the extension. Tests use the real extension callbacks with a mocked TypeSafe
endpoint; passing tests are not evidence of a live Jev review.
