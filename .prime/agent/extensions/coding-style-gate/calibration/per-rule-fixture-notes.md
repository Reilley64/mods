# Independent per-rule calibration fixture inventory

## Contract

- Source: current `CODING_STYLE.md` and `reviewer.ts` question/state contract.
- New fixture: `.prime/agent/extensions/coding-style-gate/calibration/per-rule-cases.ts`.
- 35 rule IDs; 210 individually named review snippets.
- Each rule: two positive and two negative training cases; one positive and one negative validation case.
- Total training: 140 (70 positive, 70 negative). Total validation: 70 (35 positive, 35 negative).
- Labels apply **only to `ruleId`**. Other rules can legitimately flag the same snippet.
- Fixtures are independent authored scenarios, not verbatim rubric examples or imports of the old calibration corpus.
- The interface is locally declared. There are no imports or live API calls.
- All cases supply Rust paths. Placement/reuse cases supply `referencingFiles` when required.
- These snippets supply focused review evidence, not full standalone crates. External domain types, ports, and supporting imports can be omitted. They are not compilation tests or an API-signature oracle.
- Empty `before` means addition; empty `after` means deletion. An adapter must map those to `undefined` for `RustChange` when computing `change_kind`, not pass both as defined strings. Nonempty pairs are modifications. Do not calibrate on an empty patch.

## Evidence limits and unfilled sub-boundaries

All 35 rules have six supported target-label examples. This does **not** mean every normative clause is covered:

- Dependency direction: positive examples show forbidden external types directly in domain/application Rust paths. Rust patches alone cannot establish the complete dependency graph, virtual-root workspace status, package inventory, or actual binary composition-root ownership. No evidence-free positives were assigned to those clauses. Run dependency/workspace checks separately.
- Established crates: positive cases use TOML table parsing, UTF-16 decoding, and asynchronous delay. `toml`, `encoding_rs`, and `tokio` are present in the current workspace dependency declarations. The snippets make those abstractions relevant. Popularity, maintenance status, approval changes, and the full requirement fit still need dependency review; a model cannot infer those facts from an arbitrary Rust patch.
- Lower-layer error policy: examples establish result-shape violations only. They cannot prove Rootcause version 0.13 or the absence of another error framework elsewhere in a dependency graph.
- Declaration order, callable-port syntax, unsafe confinement, and test placement: fixtures exercise visible Rust syntax/path evidence. They do not replace syntax-aware, compiler, lint, or complete repository-layout checks. A custom Cargo test target declared only in Cargo.toml is not represented.
- Single-use helper placement: `referencingFiles` is explicit fixture evidence, not the result of scanning a synthetic repository. The capability helper has one owner plus its declaration; the allowed shared helper has two actual owning use-case paths plus its declaration. Module visibility and reuse across untouched files cannot be recovered from the isolated snippet.
- Focused orchestration: reusable-algorithm cases include exported helper bodies and references from the second use-case file. Cross-file callers are represented by reviewer metadata, not by concatenating unrelated files into a fake Rust patch.
- Custom primitive and narrow-implementation justification: comments provide concrete hypothetical provider contracts. They are evidence within the scenario, not verified facts about production providers. They do not assert that an undocumented project-specific gap actually exists.
- Cancellation completion: the final-mutation cases explicitly identify the irreversible publication point in patch context. Do not generalize their labels to code where completion cannot be established.
- Validation uses distinct scenarios but the same frozen rubric. No model scoring, threshold tuning, or claim of statistical generalization was performed.

## Preservation and deletion coverage

- Cause preservation: replacing `.context(...)` with cause-erasing mapping.
- Context propagation: replacing tree-preserving context with `context_transform`.
- Cancellation preservation: deleting the completed-publication success guard.
- Custom primitive justification: deleting the gap/invariant explanation.
- Safety proofs: deleting the immediate proof while leaving the unsafe block.
- Declaration requirements: deleting use-case instrumentation.
- Whole-file deletion negatives: old capability helper and separate integration-test target removal. These remove forbidden placement rather than introduce it.

## Complete inventory

Every row has `train +2/-2`, `validation +1/-1`. The boundary column identifies the useful exception or near-boundary training negative.

| Rule ID | Exception / boundary negative |
| --- | --- |
| `comments-and-documentation-readability-before-secondary-cleanup` | Explicit enum mapping remains readable; brevity or a match alone is not the violation. |
| `comments-and-documentation-self-explanatory-code` | External registry retry limits justify a reason comment when names already explain the code. |
| `comments-and-documentation-reason-comments` | Rustdoc may describe pending-publication semantics and caller obligations. |
| `comments-and-documentation-rustdoc-format` | Local implementation rationale stays in ordinary comments rather than Rustdoc. |
| `formatting-and-imports-import-placement-and-use` | Qualified calls disambiguate same-named decoders from two modules. |
| `formatting-and-imports-phase-spacing` | Consecutive writes constructing one header stay together. |
| `architecture-and-modules-dependency-direction-and-composition-roots` | Filesystem handles belong in infrastructure, not application or domain. |
| `architecture-and-modules-capability-modules-and-public-apis` | Public capability modules are not public implementation leaves. |
| `dependencies-prefer-established-crates` | A documented legacy escape grammar permits a narrow custom codec. |
| `dependencies-narrow-custom-implementations` | A bespoke wire grammar is justified when its exact gap and one-pass invariant are stated. |
| `application-use-cases-and-ports-use-case-declaration-order` | Imports and secondary constants may surround correctly ordered primary items. |
| `application-use-cases-and-ports-use-case-parameters` | A use case that does not support cancellation need not accept a token. |
| `application-use-cases-and-ports-callable-port-invocation` | Ordinary local functions still use ordinary function-call syntax. |
| `application-use-cases-and-ports-focused-use-case-orchestration` | One-use transformation can remain inline; not every loop needs a module. |
| `application-use-cases-and-ports-use-case-local-implementation-modules` | Repeated callers justify a precise capability-level helper. |
| `errors-rootcause-lower-layer-results` | An infallible predicate need not return a Rootcause result. |
| `errors-preserve-causes-at-owned-boundaries` | A newly detected local validation error has no external cause to preserve. |
| `errors-context-propagation` | Forwarding an existing report can use into_report without a fresh ownership context. |
| `errors-presentation-error-allowlists` | Known marker inspection with fixed public messages is permitted; the report itself stays internal. |
| `runtime-primitives-primitive-selection-order` | A synchronous queue may use an ordinary standard-library collection. |
| `runtime-primitives-custom-primitive-justification` | A domain collection may wrap Vec without inventing a replacement primitive. |
| `runtime-primitives-cancellation-propagation-and-checkpoints` | Infrastructure owns inline checkpoints; direct token checks there are required, not forbidden. |
| `runtime-primitives-cancellation-state-preservation` | Success after the final irreversible mutation takes precedence over late cancellation. |
| `runtime-primitives-separate-progress-reporting` | A progress-only port may expose multiple progress measurements without becoming cancellation control. |
| `control-flow-choose-the-narrow-conditional-form` | Multi-way action mapping legitimately uses match. |
| `control-flow-guard-clauses` | Both continuing branches may use else. |
| `control-flow-match-only-for-multi-way-logic` | A two-variant exhaustive value mapping may be clearer as match. |
| `functions-and-tests-cohesive-orchestration` | A shared codec is a meaningful algorithmic seam, not one-use step fragmentation. |
| `functions-and-tests-helpers-earn-an-interface` | Repeated callers can justify a small named predicate. |
| `functions-and-tests-pre-mvp-test-placement` | Fixture helper functions belong inside the colocated test module. |
| `functions-and-tests-test-public-behavior` | Project reproducibility policy may require an ordering assertion even when a collection performs the sorting. |
| `unsafe-rust-unsafe-layer-confinement` | A safe wrapper in a dedicated infrastructure FFI module may contain justified unsafe. |
| `unsafe-rust-safety-proofs` | A public unsafe function is allowed by this proof rule when caller obligations and block proofs are explicit (confinement is a separate target). |
| `linux-review-principles-retained-language-neutral-review-priorities` | Explicitly best-effort telemetry may fail without undoing a successful durable operation. |
| `linux-review-principles-retained-rust-conventions-over-linux-c-idioms` | Rust cfg attributes are legitimate platform selection, unlike copied C preprocessor conventions. |

## Validation performed

- Parsed all 35 rule IDs from the authoritative Markdown.
- Checked exact per-rule label/split counts, 210 unique names, and nonempty before/after differences.
- Imported the TypeScript fixture through the repository Bun runtime and compared IDs to `extractStyleRules` (see parent report for command result).
- No live TypeSafe calls and no Rust builds. Synthetic snippets are not runnable units.
