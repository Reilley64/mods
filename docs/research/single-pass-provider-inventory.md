# Single-pass provider inventory

## Scope

The performance branch starts at optional-metadata commit
`438fb2eed4c5d2a4d9ba58711fcf7e16a1b19403`.

Snapshot loading collects each validated provider once. Winner construction uses
those inventories in base-game, enabled-mod priority, then Overwrite order.
Disabled mods still undergo validation but do not contribute winners.
Execution preparation reuses provider metadata bytes and file size and modification
time from validated file handles. It no longer reopens each winning path to get
those details.

The discovery loop records exact mod-folder names in a `HashSet`. It compares that
set with the names from the parsed modlist. The parser rejects duplicate and
case-colliding names before set construction. The directory map still rejects
case-colliding folders. The ordered modlist retains enabled state and priority.

No public API, dependency, build target, platform test setup, traversal limit,
settings policy, hash policy, or export policy changed. Conflict scanning and
prospective installation assessment retain their filesystem collection paths.
There is no persistent cache, environment mutation, or game launch.

## Before and after

The colocated `execution_collects_each_provider_once` test calls
`EnvironmentAdapter::prepare_execution`. Its fixture has four flat providers:
base Data, one enabled mod, one disabled mod, and empty Overwrite. Each mod has
one data file and `meta.toml`. Base Data has one file. Two files win.

Test-only thread-local counters record calls to both provider directory walkers
and provider metadata content reads. This counts directory enumerations, not all
filesystem system calls. The initial measurement also counted the winner reopen
loop. The final implementation removes that loop, so the retained regression
counter covers directory enumeration and metadata content reads.

| Operation | Baseline | Single pass |
| --- | ---: | ---: |
| Provider directory enumerations | 7 | 4 |
| Provider metadata content reads | 5 | 2 |
| Winning-file path reopens after snapshot | 2 | 0 |

The baseline walks base, enabled mod, and Overwrite twice, and the disabled mod
once. It reads enabled metadata three times and disabled metadata twice.
The new path walks each provider once and reads each present metadata file once.
The test also revalidates the prepared execution and checks that counts double.
Revalidation therefore performs a fresh scan rather than reusing a prior inventory.

The baseline preparation took 3.968334 ms in one local macOS debug fixture run.
The first passing single-pass run took 2.959708 ms. A later focused-suite run took
3.160833 ms. These are individual diagnostic timings, not a controlled benchmark
or evidence of a production speedup. Fixture setup is outside the timed section.
The 90,660-file Windows environment was not measured.

The first baseline test printed the measured counts before failing an incorrect
provisional metadata expectation of eight reads. The observed baseline was five
reads. The final regression asserts the measured single-pass counts above.

## Preserved behavior and limits

The existing safe filesystem operations still check ancestry, no-follow opens,
regular files, hard links, entry budgets, and traversal depth. Provider validation
still checks case-folded names, reserved roots, metadata schema, tombstone overlap,
and directory-tombstone ancestry. Winner construction retains the existing
`TombstoneIndex` and `resolve_effective_file` behavior. Providers without tombstones
skip the otherwise empty scan of prior winners.

New preparation regressions cover directory tombstone suppression, higher-priority
subtree reinstatement, exact tombstones, Overwrite priority, disabled providers,
metadata exclusion, changed winning-file length, changed disabled-provider
metadata, cross-provider file/directory collisions, and exact modlist matching.
Existing provider tests cover reserved names, Unicode tombstone ancestry, bounds,
and cancellation.

File details come from handles already opened by provider validation, not from
unchecked path metadata. Execution preparation collects details for losing and
disabled files too. This adds a handle metadata query for those files but removes
winner-path traversal and reopen work. The retained safe filesystem interface does
not expose the metadata it reads internally during `open_regular`.

Inventories require memory proportional to provider entries. This change does not
claim a coherent concurrent filesystem snapshot. Read-only preparation can still
observe concurrent edits at different points. Pre-launch revalidation retains its
fresh preparation and equality check. Real Windows reparse behavior and real-game
performance remain unmeasured here.

## Validation

- `cargo test -p infrastructure-environment -- --nocapture` passed all 91 tests.
- Focused environment Clippy passed.
- Full `bun run check` passed formatting, workspace Clippy, dependency checks,
  2 release-version tests, all 423 Rust tests, and all 119 tool tests.
- `git diff --check` passed.
- Independent spec and standards reviews passed with no blockers. The spec reviewer
  reran all nine execution-preparation tests and confirmed the 4-enumeration,
  2-metadata-read count and fresh revalidation.

## Coding-style gate dispositions

The gate reported likely findings during edits. These are dispositions, not a claim
of a clean gate result.

| File | Rule | Disposition |
| --- | --- | --- |
| `snapshot.rs` | Use-case parameters | Accepted as not applicable. These are infrastructure helpers, not application use-case entry points. Existing recursive walker parameter order stays intact. |
| `execution_preparation.rs` | Use-case parameters | Accepted as not applicable. Existing infrastructure adapter methods retain their public signatures and cancellation-last order. |
| `execution_preparation.rs` | Use-case declaration order | Accepted as not applicable. This file implements an infrastructure adapter, not an application use case. |
| `snapshot.rs` | Focused use-case orchestration | Accepted as not applicable. This existing infrastructure snapshot module owns provider validation and winner construction. No application use-case file or generic utility module was added. |
| `snapshot.rs` | Test public behavior | Accepted. Existing private walker tests protect project-owned traversal bounds. The new counters support the approved scan-count regression through public preparation. They do not test dependency internals. |
| `execution_preparation.rs` | Test public behavior | Accepted. New tests call public preparation and revalidation. Private counters and length observations prove the requested traversal and metadata-reuse behavior. No production helper was extracted for tests. |
| `snapshot.rs` | Narrow custom implementations | Accepted. The inventory is a private data structure for existing provider rules. It uses the existing safe filesystem and domain tombstone implementations. It does not introduce general-purpose infrastructure. |
| `execution_preparation.rs` | Narrow custom implementations | Accepted. The change consumes snapshot details through existing adapter methods. Test counters use the standard library. No replacement library or runtime was added. |
| `snapshot.rs` | Phase spacing | Addressed. Validation, inventory ordering, and output construction have separate blocks. Remaining adjacent statements collect or consume one inventory. |
| `execution_preparation.rs` | Phase spacing | Addressed. Winner projection, profile reads, consumed metadata, and output construction remain separate blocks. Test fixture setup, preparation assertions, and mutation checks are separate blocks. |

Evidence prose was reviewed with the local Unslop skill. No outgoing repository
message, commit, push, or pull request was made by this implementation task.
