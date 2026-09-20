# Ticket scope control

Use this process for issue and ticket implementation, including review remediation.

## Freeze the contract

Before the first implementation edit, present a short scope contract containing:

- required behavior and acceptance criteria;
- explicit non-goals;
- frozen interfaces, constants, and compatibility requirements;
- expected files or modules;
- validation that can run in the current environment and known platform gaps.

Treat the accepted contract as the boundary for the ticket. A scope change requires explicit user approval before implementation continues.

## Keep implementation bounded

Use one writer for a worktree. Reviewers remain read-only. Hand ownership to a new writer only after the previous writer has stopped.

Pause and report a scope delta before adding any of the following unless the contract already requires it:

- a new threat model or security boundary;
- a public API, wire-format, or frozen-constant change;
- an unexpected dependency, module, service, or build target;
- a new platform test harness;
- work that belongs to a stated non-goal or later ticket.

Record validation that requires an unavailable platform as blocked evidence. Do not replace it with speculative infrastructure or claim it passed.

## Triage review findings

Classify each finding before editing:

- **Blocker:** directly violates the accepted contract, a documented repository standard, or demonstrates a concrete defect in the requested behavior.
- **Follow-up:** hardening, broader threat coverage, refactoring, or additional evidence outside the accepted contract.

Fix blockers in the ticket. Report follow-ups without implementing them unless the user expands the contract.

## Bound repair cycles

Run one repair and re-review cycle for the collected blockers. If the re-review still finds blockers, stop and ask the user to choose the next step. Do not start a chain of repair agents or silently broaden the design.

At each progress update, state whether scope changed. Completion requires the in-scope checks to pass, unavailable checks to be listed explicitly, and follow-up work to remain separate.
