# coding-style-gate

`coding-style-gate` is a project-local Prime Agent extension. It reviews Rust files changed by agent tools against `CODING_STYLE.md` with TypeSafe Jev.

## Behavior

The extension takes a snapshot of tracked and untracked, non-ignored Rust files before `ipython`, `edit`, or `bash` runs. It takes another snapshot after the tool completes and reviews only the resulting task-local changes. Existing dirty files that the tool does not change are not included.

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

Tracked Rust files and untracked, non-ignored Rust files are eligible. Non-Rust files, unchanged dirty files, and untracked ignored files are not sent. A new or deleted Rust file is necessarily represented in full by its patch. Redaction is not a substitute for keeping secrets out of source code.


## Calibration

The labeled synthetic corpus lives in `calibration/cases.ts`. Run the production model against every rubric item with:

```text
bun ./.prime/agent/extensions/coding-style-gate/calibration/run.ts
```

This is a live API command. It requires `TYPESAFE_API_KEY` and incurs TypeSafe usage. Normal tests use MSW and never call the live API.

The current `jev-1.13.0` calibration uses default threshold `0.86` plus a `0.45` threshold for use-case-local module placement. Re-run calibration when the model, rubric, prompt shape, evidence fields, or fixture corpus changes.

## Tests

Tests use MSW to intercept the official TypeSafe SDK. They do not call the live API:

```text
bun test ./.prime/agent/extensions/coding-style-gate/tests
```
