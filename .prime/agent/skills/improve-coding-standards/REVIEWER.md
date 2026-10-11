# Reviewer brief

Use this brief for each rubric-sweep subagent. Fill in the placeholders. Spawn all reviewers in parallel, then end your turn and wait for their messages.

```text
You are a read-only reviewer. Do not edit, format, stage, or commit files.

Repository: <absolute repo path>
Base commit: <oid>
Rubric: docs/coding-standards/<area>.md, section "<## section title>", items:
- <rule-id>: <### item title>
- ...
Scope: <list of files, or a git ls-files pattern>
Output file: <tmpdir>/coding-standards-review-<timestamp>/<section-slug>.md

Read CODING_STANDARDS.md first for the Authority order, then the area file above.
Your job is only the listed items. An item with an Applies to list covers only
files that match one of its globs; skip the files it does not cover.

Read each file in scope against each listed item. A candidate must meet the item's
Violation boundary, and no Compliant clause may cover it. Look at the Bad and Good
examples to calibrate, but judge by the boundaries, not by surface similarity.

For each candidate, write one entry:

- file:line-range
- rule ID
- the Violation clause it meets, quoted
- each Compliant clause you checked, and why it does not apply
- a fix sketch: a before/after snippet of at most 15 lines each
- risk: mechanical (layout, imports, names, comments) or structural (signatures,
  module placement, control flow, error handling)
- whether the fix would change behavior, a public API, error or cancellation
  semantics, a file format, a limit, FFI ownership, or a user-visible message

End the file with a coverage list: every file you read. The review is complete when
every file in scope is on that list.

Then send the output file path and the candidate count to the parent with
agent_message.send(..., receiver_role='parent').
```

Assign each file in scope to at least one reviewer for each area file whose items cover it. For a large scope, split by area as well as by section. Keep each reviewer to about 40 files.
