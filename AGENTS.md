## Session naming

At the start of a new session inside Herdr (`HERDR_ENV=1`), name the current worktree workspace from the first substantive user message, before starting task work:

1. Read the Herdr skill. Use `HERDR_WORKSPACE_ID` to inspect the calling workspace and confirm its linked worktree matches the current Git worktree. If the caller ID or matching linked worktree is unavailable, skip naming.
2. Choose a short task label, such as `style-gate-worktree-scope`. Use the user's task, rather than injected context or harness messages, as the source.
3. Run `herdr workspace rename "$HERDR_WORKSPACE_ID" "<task-label>"`, then verify the label with `herdr workspace get "$HERDR_WORKSPACE_ID"`.

Set the label once per session; later messages retain it unless the user requests a rename. Change only the Herdr label, preserving the Git branch and checkout path. If naming fails, report it briefly and continue the task.

## Agent skills

### Issue tracker

Before starting issue work or writing a PR description, read `docs/agents/issue-tracker.md` for assignment and closing-reference requirements.

### Triage labels

Triage uses the five default canonical labels. See `docs/agents/triage-labels.md`.

### Domain docs

Domain documentation uses the single-context layout. See `docs/agents/domain.md`.

### Coding style

Rust implementation and review must follow `CODING_STYLE.md`.

Coding-style gate dispositions live in `.prime/agent/coding-style-dispositions.json`, which is git-ignored and never committed. When a PR relies on dispositions, list each accepted finding in the PR description with its file, rule, and reason.

### Ticket scope

Ticket implementation and review remediation use a frozen scope contract and bounded repair cycle. See `docs/agents/ticket-scope.md`.

## Orchestrating subagents

Keep the parent responsible for the final result. Delegate bounded, substantive tasks when parallel work saves time or improves quality; handle a short lookup inline. Launch independent tasks in parallel, and keep dependent steps in order. Give each child a unique name, the context it needs, and a clear deliverable.

Spawn children without `model` or `thinking` arguments, so each child inherits the parent's model and thinking level. Do not route subagent spawns to other models. Pass a model or thinking level only when the user explicitly requests one for that child.

Collect child results, inspect any produced files, reconcile conflicts, and run the checks needed for the integrated result. Report meaningful progress while delegated work is running.
