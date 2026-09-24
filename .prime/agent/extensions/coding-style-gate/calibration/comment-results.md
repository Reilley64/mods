# Comment calibration

## Scope and labels

Audited added Rust comments in `7d7d939` (#31), `4734903` (#86),
`94c69c9` (#52), and `6daca92` (#46). The #52 commit adds no Rust comments.
`comment-audit.json` preserves 14 excerpts and label rationales: eight clearly
useful comments and six ambiguous terse Rustdoc summaries. Ambiguous summaries
are observations, not ground-truth negatives or confirmed violations.

`comment-cases.ts` contains ten useful excerpts and five explicitly identified
controlled mutations that replace useful documentation with redundant narration.
Two extra useful excerpts cover second-interrupt semantics and Windows child
status semantics. The mutations are **not actual historical review misses**.
The source audit did not establish clear violations in the sampled real additions.
It does not establish that every comment in those PRs is necessary.

## Live measurements

Used `jev-1.13.0`, the production reviewer, and all 35 rubric questions. Credentials
came from the environment. `comment-results.json` stores scores, thresholds,
usage, and rubric hashes. Calls were live, not mocked.

| Run | Redundant comments caught | Useful examples flagged |
| --- | ---: | ---: |
| Original rubric, 0.86 | 4/5 | 0/10 |
| Clarified rubric, 0.86 | 4/5 | 0/10 |
| Clarified rubric, comment-only 0.70 | 5/5 | 0/10 |

The redundant `ProcessStatus` field summary scored 0.66 with the original rubric,
then 0.78 with clarified wording; the final repeat scored 0.79. The first smaller
probe scored it 0.61. Scores vary between calls. The clarified useful-comment
maximum was 0.50 (0.46 in the final repeat), while the redundant minimum was 0.77
in the full-corpus run before changing the threshold. A comment-only threshold
of 0.70 separates these observed groups without lowering other rule thresholds.

The final expanded 47-case corpus recorded 20 true positives, one false negative,
26 true negatives, and zero false positives under configured thresholds.
The remaining false negative is `use-case-local-placeholder-logic-bad`, also
missed in the original baseline. It is outside this comment calibration's scope.

The six ambiguous real summaries scored 0.36–0.65 in a separate observational
run. None crossed 0.70. These scores are not evidence that the comments are
necessary; stricter labels need an explicit policy decision or user examples.

## Changes and limits

- The reason-comments rule explicitly covers Rustdoc that only repeats names,
  fields, signatures, or implementation steps.
- API contracts, error conditions, sentinel semantics, safety proofs, and meaningful
  introductory summaries attached to contracts remain allowed.
- Only the reason-comments threshold changes, from 0.86 to 0.70.
- No production Rust comments were deleted. Advisory mode remains advisory.

This is a small calibration set, not an independent estimate of production recall.
The same examples informed the rubric and threshold. New labeled user examples
should become held-out checks before further tuning. Historical per-edit reviews
cannot be reconstructed from these scores; receipts only help after the updated
extension has loaded.

Reproduce with `bun ./.prime/agent/extensions/coding-style-gate/calibration/run.ts`.
This incurs live API usage. Normal tests validate corpus structure with mocked
reviewer tests and make no live calls. Use `/reload` to load the changed policy
and configuration in the agent.
