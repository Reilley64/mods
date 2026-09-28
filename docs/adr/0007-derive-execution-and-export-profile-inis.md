---
status: accepted
---

# Derive execution and export INIs from portable Profile State

For post-MVP resolved-file export, keep stored Profile State free of injected private save routing and archive-invalidation settings; derive temporary execution INIs with both, and exported INIs with invalidation plus normal save routing. The owner chose the private execution path over mapping onto ordinary shared saves to avoid increasing overlap with unrelated saves, accepting temporary-file lifetime and settings-persistence work rather than stronger isolation claims.

Preserve valid child-written settings without copying injected overrides back, retaining original canonical managed-key values and reporting concurrency/preservation failures. Keep temporary files through the full managed process lifetime; retain uncertain or failed work without rollback. This deliberately changes #31's initialization/projection timing, not [ADR-0005](0005-use-upstream-usvfs.md)'s upstream limitations. Migration is excluded. Research and decisions are in [#96](../research/issue-96-resolved-file-export.md) and the [#97 contract](../research/issue-97-export-contract.md); the owner approved the consolidated handoff, and #25 closure is still required before implementation.

Owner-approved exception: #98 may now implement a bounded test-only prototype before #25 closes. Production work in #99 onward remains gated on #25. The prototype does not alter production initialization/execution or approve migration.
