# Bounded spec recheck

> Superseded note: the missing-archive-list refusal described here was later replaced by the approved embedded default fallback. See [streamline exec preparation](streamline-exec-preparation.md).

## Result

The sole implementation blocker in `/tmp/streamline-spec-review.md` is resolved. No scope expansion was found in this repair. This was a read-only source and saved-log review; I did not rerun tests or modify the repository.

## Repair inspected

`src/infrastructure/environment/src/safe_fs.rs:156-163` now opens through `self.open_regular(name)` and reads the resulting handle with standard `read_to_end`. This restores the existing no-follow, regular-file, reparse-point, and hard-link checks at open time. The read remains uncapped and adds no custom chunk loop. It does not restore asset-inventory validation opens or prelaunch change detection.

The new public-path regressions in `src/infrastructure/environment/src/execution_preparation.rs:461-511` cover:

- Hard-linking either the canonical or child Fallout.ini after prepare/derive. Preservation must fail, keep child edits and the temporary directory, leave canonical bytes unchanged, and report both an I/O cause and the retained-INI path.
- A relative symlink replacing required Fallout.ini. Public launch preparation must reject it.

These directly cover the previous finding without creating a new filesystem policy.

## Evidence inspected

- `/tmp/streamline-repair-red.log`: both new tests failed against the prior implementation.
- `/tmp/streamline-repair-focused.log`: 158 tests passed, including both regressions, relaxed asset-link traversal, and uncapped exec configuration reads.
- `/tmp/streamline-repair-check.log`: 438 Rust tests and 119 tool tests passed, including both regressions.

## Remaining limits

The policy for both profile archive lists being absent is still unresolved. The temporary `profile_archive_list_missing` refusal is not an approved final policy. Native Windows compilation/runtime behavior and JIP archive/save timing remain unverified. Closing this implementation finding does not remove those completion limits.
