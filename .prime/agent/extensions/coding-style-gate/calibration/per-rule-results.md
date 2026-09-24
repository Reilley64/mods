# Per-rule threshold calibration

Model: `jev-1.13.0`. Live run: `2026-09-24T01:35:06.202Z`.

## Method

All 35 authoritative rules have an explicit threshold. There is no scalar fallback.
Each rule has four independently authored training cases and two separate validation
cases. Labels apply only to the target rule; other rules may legitimately flag a
snippet. The earlier 47 cases are a separate regression set, not fitting inputs.

Fit thresholds using training scores only. Candidate thresholds are 0, 1, and
unrounded midpoints between adjacent observed scores. Minimize
`2 * false positives + false negatives`; break ties by fewer false positives,
larger distance to observed scores, then the higher threshold. A training overlap
is reported, not hidden. Thresholds were saved before validation started. They
were not retuned against validation or regression outcomes.

Training and legacy regression requests use the production reviewer with their
target question. Held-out validation requests use all 35 production questions.
No rubric text was changed during this calibration. Snippets are review inputs,
not compilation units. `per-rule-fixture-notes.md` explains coverage and limits.

## Measured outcomes

| Set | TP | FN | TN | FP |
| --- | ---: | ---: | ---: | ---: |
| training | 69 | 1 | 70 | 0 |
| validation | 32 | 3 | 34 | 1 |
| regression | 20 | 1 | 25 | 1 |

Held-out accuracy is 66/70 on this small balanced set. It is not a production
accuracy estimate. Each rule has only one positive and one negative held-out
example. Thresholds are provisional; advisory mode remains unchanged.

### Known weaknesses

- Context propagation has overlapping training scores and misses one training violation.
- Held-out primitive selection flags one compliant example.
- Held-out custom-primitive justification, cancellation-state preservation, and helper
  interface value each miss one violation.
- The older regression set retains the use-case-local placeholder miss and flags
  the inline-cancellation good example. These are known errors, not passes.

Do not switch to enforce mode on these measurements alone. Add independent
boundary examples before tuning weak rules again. Compiler, layout, dependency,
and other deterministic checks remain authoritative; Rust patches cannot supply
complete workspace or dependency-policy evidence.

## Explicit thresholds

| Rule | Threshold | Training separable | Held-out TP/FN/TN/FP |
| --- | ---: | --- | --- |
| `comments-and-documentation-readability-before-secondary-cleanup` | 0.475 | yes | 1/0/1/0 |
| `comments-and-documentation-self-explanatory-code` | 0.395 | yes | 1/0/1/0 |
| `comments-and-documentation-reason-comments` | 0.52 | yes | 1/0/1/0 |
| `comments-and-documentation-rustdoc-format` | 0.35 | yes | 1/0/1/0 |
| `formatting-and-imports-import-placement-and-use` | 0.315 | yes | 1/0/1/0 |
| `formatting-and-imports-phase-spacing` | 0.205 | yes | 1/0/1/0 |
| `architecture-and-modules-dependency-direction-and-composition-roots` | 0.16 | yes | 1/0/1/0 |
| `architecture-and-modules-capability-modules-and-public-apis` | 0.47 | yes | 1/0/1/0 |
| `dependencies-prefer-established-crates` | 0.4 | yes | 1/0/1/0 |
| `dependencies-narrow-custom-implementations` | 0.165 | yes | 1/0/1/0 |
| `application-use-cases-and-ports-use-case-declaration-order` | 0.075 | yes | 1/0/1/0 |
| `application-use-cases-and-ports-use-case-parameters` | 0.085 | yes | 1/0/1/0 |
| `application-use-cases-and-ports-callable-port-invocation` | 0.435 | yes | 1/0/1/0 |
| `application-use-cases-and-ports-focused-use-case-orchestration` | 0.315 | yes | 1/0/1/0 |
| `application-use-cases-and-ports-use-case-local-implementation-modules` | 0.575 | yes | 1/0/1/0 |
| `errors-rootcause-lower-layer-results` | 0.45 | yes | 1/0/1/0 |
| `errors-preserve-causes-at-owned-boundaries` | 0.445 | yes | 1/0/1/0 |
| `errors-context-propagation` | 0.55 | no | 1/0/1/0 |
| `errors-presentation-error-allowlists` | 0.425 | yes | 1/0/1/0 |
| `runtime-primitives-primitive-selection-order` | 0.33 | yes | 1/0/0/1 |
| `runtime-primitives-custom-primitive-justification` | 0.555 | yes | 0/1/1/0 |
| `runtime-primitives-cancellation-propagation-and-checkpoints` | 0.405 | yes | 1/0/1/0 |
| `runtime-primitives-cancellation-state-preservation` | 0.485 | yes | 0/1/1/0 |
| `runtime-primitives-separate-progress-reporting` | 0.51 | yes | 1/0/1/0 |
| `control-flow-choose-the-narrow-conditional-form` | 0.365 | yes | 1/0/1/0 |
| `control-flow-guard-clauses` | 0.46 | yes | 1/0/1/0 |
| `control-flow-match-only-for-multi-way-logic` | 0.525 | yes | 1/0/1/0 |
| `functions-and-tests-cohesive-orchestration` | 0.44 | yes | 1/0/1/0 |
| `functions-and-tests-helpers-earn-an-interface` | 0.7 | yes | 0/1/1/0 |
| `functions-and-tests-pre-mvp-test-placement` | 0.27 | yes | 1/0/1/0 |
| `functions-and-tests-test-public-behavior` | 0.255 | yes | 1/0/1/0 |
| `unsafe-rust-unsafe-layer-confinement` | 0.42 | yes | 1/0/1/0 |
| `unsafe-rust-safety-proofs` | 0.585 | yes | 1/0/1/0 |
| `linux-review-principles-retained-language-neutral-review-priorities` | 0.41 | yes | 1/0/1/0 |
| `linux-review-principles-retained-rust-conventions-over-linux-c-idioms` | 0.395 | yes | 1/0/1/0 |

## Evidence and reproduction

`per-rule-results.json` contains every score, label, split, threshold, aggregate,
token usage, rubric/fixture hashes, and reviewer/input-adapter/fitter hashes.
The threshold table rounds display values only; configuration preserves exact
fitted midpoints. The request run contains 257 unique examples (140 training,
70 held out, 47 legacy regression), plus one corrective re-evaluation of a deleted
file with no current module references. Both observations and the reason for the
correction are retained; no threshold was retuned. API credentials and raw
provider responses are not stored.

```text
bun ./.prime/agent/extensions/coding-style-gate/calibration/per-rule-run.ts .scratch/per-rule-new.json
bun run check:tools
```

The first command incurs live TypeSafe usage and requires `TYPESAFE_API_KEY`.
It refuses to overwrite its progress journal and never edits configuration.
Copy thresholds only after reviewing results. The second command is offline.

Missing or invalid per-rule entries fail before creating a client or sending a
review request. Legacy scalar `threshold` configuration is rejected with migration
instructions. Extra map entries are accepted for subset reviews; repository tests
require the checked-in map to match the current rubric exactly.

Use `/reload` to activate the new extension/configuration.
