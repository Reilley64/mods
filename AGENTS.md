## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Triage uses the five default canonical labels. See `docs/agents/triage-labels.md`.

### Domain docs

Domain documentation uses the single-context layout. See `docs/agents/domain.md`.

### Coding style

Rust implementation and review must follow `CODING_STYLE.md`.

### Ticket scope

Ticket implementation and review remediation use a frozen scope contract and bounded repair cycle. See `docs/agents/ticket-scope.md`.

### Implementation worktrees

Before starting `/skill:implement`, fetch `origin/main`, then create a new branch and worktree from `origin/main`. Create worktrees under `<project-root>/.worktrees/` and run the implementation only in its worktree.
