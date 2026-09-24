# Repository-wide style cleanup

Base: `8e8ba7afe5a5b512dae498800e509e4186dde6ee`.

## Coverage

Reviewed all 131 handwritten Rust files across domain/application (41), environment
(12), archive (13), execution/dependencies (22), game platform/settings (29), and
presentation (14). The generated `execution/src/usvfs/ffi.rs` boundary is excluded.
No vendor sources, dependencies, public interfaces, file formats, resource limits,
error/cancellation semantics, or FFI ownership protocols were changed.

## Selected cleanup

- Flatten optional metadata and profile validation while preserving checks and order.
- Remove the temporary modlist representation and redundant final-iteration branch.
- Remove ignored fixture identity arguments; retain meaningful fixture seams.
- Name plugin collections, output selection, and archive cursor/owned resources clearly.
- Use required-pattern guards where only one outcome is accepted.
- Keep explicit matches where both success and error recovery carry meaningful work.
- Import unambiguous items once and separate real orchestration phases.
- Place useful comments with their operation or use Rustdoc for item contracts.
- Include the user's existing native README heading fix verbatim.

Reviewers found no basis for blanket comment deletion. Safety proofs, compatibility
rationale, caller contracts, and irreversible-publication explanations are retained.
Jev findings were reviewed as candidates, not used as a target count. Application
use-case rules do not apply to infrastructure methods or private fixture helpers.
The CLI cwd guard preserves its existing allowlisted error mapping; this cleanup
does not redesign its error contract. The qualified bindgen::CargoCallbacks::new()
call is retained because importing the name also imports a deprecated constant.

## Separate follow-ups (not implemented)

- Three conflicts/projection.rs tests can silently pass on an unexpected variant;
  changing assertions is test-correctness work rather than style-only cleanup.
- Existing cancellation-check ownership and parser/result architecture need semantic
  review before any broad consolidation or migration.
- Presentation's existing root-selection/OS error conversion contract merits its own
  review; visible messages and report propagation are frozen here.
- Broad parser unification, new dependencies, public API narrowing, and FFI redesign
  are excluded rather than folded into a style patch.

## Validation

Baseline and final `bun run check` passed on macOS: formatting, Clippy with warnings
denied, dependency graph, 366 Rust tests, and 71 tooling tests. Independent final
standards and behavior reviews found no blockers. Windows native and x86/x64 checks
require CI.

## Final aggregate review: 22 findings across 38 edited Rust files

All 22 candidates were inspected. No additional source repair is required:

- **Two narrow-condition findings (Steam discovery and version selection):** both
  Result arms carry useful work: successful value extraction versus first-error
  retention and continuation. An explicit match is the narrow clear form here.
- **One Rustdoc finding (archive limits):** the three item counting contracts use
  `///`. The remaining `//` explains why staged output needs an independent cap;
  it is implementation rationale, not an API contract needing Rustdoc sections.
- **One test-behavior finding (RAR):** retained-source and handle tests verify the
  project's snapshot ownership, byte limits, and path-swap policy. They do not
  duplicate the dependency's parser suite; this diff only renames owned fields.
- **One phase-spacing finding (FOMOD evaluator):** guard, option construction,
  warning collection, and condition traversal phases are separated. Option
  enumeration, its synthetic-none alternative, nonempty validation, and storing
  the resulting group remain a cohesive construction operation. Independent
  standards review found no missing semantic separator in these edited hunks.
- **One declaration-order finding (install_archive):** the actual order is
  InstallArchiveDependencies, InstallArchiveOutput, InstallArchiveError, then
  the instrumented async install_archive function. The Display impl does not
  change the relative order of those four primary items.
- **Sixteen use-case-parameter findings:** install_archive already passes its
  dependency bundle first, explicit typed business values next, and cancellation
  last. The remaining flagged files are infrastructure implementations (rar,
  seven_zip, extract, native execution adapter, snapshot, version, transactions,
  conflict_scan, profile_activation, managed, archive adapter, environment
  profile), presentation runner, projection helpers, and the private FOMOD
  evaluator. These are not application use-case entry points. Unchanged helper
  signatures do not acquire an application dependency bundle merely because
  their files are included in an aggregate diff.

Independent final Standards and behavior reviews both found no blockers. Local
`bun run check` passed: formatting, Clippy with warnings denied, dependency graph,
366 Rust tests, and 71 tooling tests. Windows validation remains pending CI.
