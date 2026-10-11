## Agent skills

### Issue tracker

Before starting issue work or writing a PR description, read `docs/agents/issue-tracker.md` for assignment and closing-reference requirements.

### Triage labels

Triage uses the five default canonical labels. See `docs/agents/triage-labels.md`.

### Domain docs

Domain documentation uses the single-context layout. See `docs/agents/domain.md`.

### Coding standards

Before you write or review Rust, read `CODING_STANDARDS.md` and the area files it names for the diff.

### Ticket scope

Ticket implementation and review remediation use a frozen scope contract and bounded repair cycle. See `docs/agents/ticket-scope.md`.

## Orchestrating subagents

Keep the parent responsible for the final result. Delegate bounded, substantive tasks when parallel work saves time or improves quality; handle a short lookup inline. Launch independent tasks in parallel, and keep dependent steps in order. Give each child a unique name, the context it needs, and a clear deliverable.

Spawn children without `model` or `thinking` arguments, so each child inherits the parent's model and thinking level. Do not route subagent spawns to other models. Pass a model or thinking level only when the user explicitly requests one for that child.

Collect child results, inspect any produced files, reconcile conflicts, and run the checks needed for the integrated result. Report meaningful progress while delegated work is running.
