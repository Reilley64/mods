# Calibration

`cases.ts` contains paired synthetic Rust patches for fifteen boundary pairs. Each boundary has one deliberate violation and one compliant counterpart.

`run.ts` sends every patch through the production reviewer against the complete `CODING_STYLE.md` rubric. It reports:

- the target-rule score for each labeled patch;
- the highest score from any other rubric item;
- threshold confusion matrices;
- the midpoint between the observed bad-score floor and good-patch ceiling.

Run it only when live TypeSafe usage is intended:

```text
bun ./.prime/agent/extensions/coding-style-gate/calibration/run.ts
```

## Latest calibration

Model: `jev-1.13.0`

Rubric items: 35, sent together in one request per patch.

Corpus: 15 violation patches and 15 compliant counterparts.

Observed results:

- default threshold: `0.86`;
- use-case-local implementation module threshold: `0.45`;
- use-case-local synthetic bad path: `0.87`;
- use-case-local synthetic good path: `0.20`;
- configured result: 15 target true positives, 0 target false negatives, 15 whole-patch true negatives, and 0 whole-patch false positives;
- owning-entry-point distinction: helper bad `0.78`, owning use-case good `0.05`;
- capability-interface distinction: helper bad `0.83`, `types.rs` good `0.05`;
- Rustdoc-format distinction: plain item comment bad `0.95`, Rustdoc good `0.08`;
- application-layer scope: application helper bad `0.79`, equivalent infrastructure module good `0.05`;
- semantic phase spacing: bad `0.90`, good `0.07`.

PR #46 is a violation holdout, not a compliant calibration target. Against the exact PR range, the targeted rule reports:

- `application/src/installation/fomod.rs`: `0.84` (violation);
- `application/src/installation/planning.rs`: `0.83` (violation).

The expected compliant locations are private children under `application/src/installation/install_archive/`. The authoritative corrected full-range review from `b08bed9aeec17c796205ed66fce2ffbbf388372c` through `63c6b7404e7ed24e872c319f2589b105e15a3703` reviewed 95 Rust files against all 35 rules with zero findings.

The corpus is deliberately small. Keep the gate in advisory mode while collecting representative repository patches. Add every confirmed miss or false positive as a paired regression fixture before changing the threshold.

## Retroactive range review

Review the Rust merge diff between two full commit OIDs without checking out either revision:

```text
bun ./.prime/agent/extensions/coding-style-gate/calibration/review-range.ts <base-oid> <head-oid>
```

Review one rule against selected changed files:

```text
bun ./.prime/agent/extensions/coding-style-gate/calibration/review-range.ts <base-oid> <head-oid> <rule-id> <exact-path>...
```

The command resolves the merge base, loads each changed Rust file from that base and the head, loads the complete head Rust tree for module-reference evidence, builds task-style patches with 20 context lines, and runs the configured production model and threshold. Redirect stdout when a large JSON report is expected.

