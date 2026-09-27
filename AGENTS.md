## Agent skills

### Issue tracker

Before starting issue work or writing a PR description, read `docs/agents/issue-tracker.md` for assignment and closing-reference requirements.

### Triage labels

Triage uses the five default canonical labels. See `docs/agents/triage-labels.md`.

### Domain docs

Domain documentation uses the single-context layout. See `docs/agents/domain.md`.

### Coding style

Rust implementation and review must follow `CODING_STYLE.md`.

### Ticket scope

Ticket implementation and review remediation use a frozen scope contract and bounded repair cycle. See `docs/agents/ticket-scope.md`.

## Orchestrating subagents

Keep the parent responsible for the final result. Delegate bounded, substantive tasks when parallel work saves time or improves quality; handle a short lookup inline. Launch independent tasks in parallel, and keep dependent steps in order. Give each child a unique name, the context it needs, and a clear deliverable.

Before **each** `rlm.spawn()` where the user has pinned neither model nor thinking level, follow [the Jev subagent router skill](.prime/agent/skills/subagent-router/SKILL.md). Supply the planned child task; pass the returned model and thinking level to `rlm.spawn()`. If routing falls back, omit both arguments and use normal inheritance. If spawn admission rejects a routed model as unavailable, retry once with inherited settings; discovery alone is not an availability check. Honor explicit user model or thinking choices instead of routing over them. The skill is an explicit pre-spawn step, not an automatic runtime hook.

Collect child results, inspect any produced files, reconcile conflicts, and run the checks needed for the integrated result. Report meaningful progress while delegated work is running.
