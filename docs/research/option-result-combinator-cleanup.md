# Option and Result combinator cleanup

Base: `c99c9023122e5a14de9e1b7bb7928c1667e47494`.

This pass checks Rust code that existed before two coding standards were added on 2026-09-30.
The items are "Prefer Option and Result combinators" and "Reusable capability ports".
The previous full cleanup used base `8e8ba7afe5a5b512dae498800e509e4186dde6ee`.

## Scope contract

- Behavior, user-visible messages, exit statuses, JSON documents, file formats, limits,
  cancellation semantics, and FFI ownership stay unchanged.
- One public application API changes, with approval. `application::ports::ResolveGameInstallation`
  becomes `DiscoverGameInstallation`, and `InitializeEnvironmentDependencies` gains
  `validate_game_directory` and `discover_game_installation`.
- One internal error-tree change, with approval. CLI input validation now keeps the domain
  cause under the same outermost `ErrorMarker`. Output is unchanged because presentation
  formats only the outermost marker.
- One reason comment is added in `main.rs`, with approval, to state why the `--json` flag is
  read from raw arguments.

## Coverage

- The two new rules and the changed "Choose the narrow conditional form" rule were checked
  on all 167 handwritten Rust files. Five reviewers split the files by area.
- The full rubric was checked on the 11 files with five or more commits since the previous
  cleanup.
- Jev reviewed the range `8e8ba7a..c99c902` (138 files). `runner.rs` exceeded the
  100,000-character patch limit, so Jev reviewed it in three sub-ranges.
- `format:check`, `lint`, and `dependency:check` passed before any edit.

There were 361 candidates. 78 site groups became findings in 14 batches, and 282 were dropped.
Jev produced 272 candidates. Of these, 128 applied use-case rules outside application use cases.
Three more literal violations are required by issue #128: two secret-redacting parse errors
and the removal of partial downloads on cancel.

## Cleanup done

1. `let ... else` guards that only turn `None` into an error now use `ok_or_else` (14 sites),
   including the rubric's own Bad example in `usvfs/mod.rs` (8 Windows sites).
2. Option and Result defaults use `map_or_else`, `unwrap_or_else`, `map`, and `transpose`
   (11 sites). The UTF-8 then Windows-1252 decode keeps its named binding.
3. Test guards use `ok_or` and `?`. Archive tests now propagate the report, so a failure
   shows its cause instead of a fixed string.
4. CLI input validation for `init`, `config set game-dir`, `install`, `shortcut`, and `exec`
   keeps the domain report as the cause.
5. Test modules import `execute`, `run_current_process`, `Path`, and `INVENTORY_IO` instead
   of repeating qualified paths.
6. Blank lines separate validation, staging, and publication in `snapshot.rs`, `usvfs/mod.rs`,
   `settings/lib.rs`, `nexus/http.rs`, `nexus/cache.rs`, `environment/export.rs`, and `main.rs`.
   Two back-to-back progress calls in `install_archive` now share one block.
7. Three one-use forwarding helpers are inlined: `snapshot::load_inner`,
   `EnvironmentAdapter::assess_installation`, and `NativeExecution::check_retained_state`.
8. Win32 launch error codes and `STATUS_CONTROL_C_EXIT` have names.
9. `validate_layout` is now `validate_initialized_layout`, and its step-list comment is gone.
   `foreign_handle` uses Rustdoc.
10. `initialize_environment` chooses the explicit path, then `MODS_GAME_DIR`, then discovery.
    It validates the first two with the shared `ValidateGameDirectory` port, which
    `set_game_directory` also uses. The game-platform adapter keeps only discovery.
11. CLI error advice and init routing lose one nesting level.
12. The malformed-choice and root-selection tests run through `run`. The six Overwrite
    provider tests run through `load` on a published environment, and the two DataMod tests
    use `validate_staged_provider`.

## Kept on purpose

- `direct_choices_preserve_occurrence_order_and_whitespace` still calls `parse_choices`.
  Observing whitespace through `run` needs a FOMOD archive fixture with space-padded group ids.
- In the CLI dispatch functions, a `match` binds the error. `let ... else` cannot bind it, and
  these functions return `RunOutcome`, so `?` is unavailable.
- `install_archive` picks the mod name with a three-way `match`. The combinator version was
  flagged as harder to read, and readability ranks first in the rubric.
- Forwarding `Some(error)` to `Err` in `steam/discovery.rs`, `version.rs`, and `child_output.rs`
  stays a guard. `map_or(Ok(None), Err)` is denser than the guard.

## Follow-ups not done

- `ValidateGameInstallation` is declared and re-exported, but no code uses it.
- `archive/src/path.rs` and `domain/src/paths.rs` contain the same reserved-device-name check.
- Three files repeat the UTF-8 then Windows-1252 decode fallback.
- `usvfs/mod.rs` and `launch_inputs.rs` repeat the bare `CreateProcessW` limit `32767`.

## Repository changes in the same pull request

- The Jev-based coding-style gate is removed: the Prime Agent extension, its calibration
  and tests, `.prime/agent/coding-style-gate.json`, the dispositions file convention, and
  the `@typesafe-ai/sdk`, `diff`, and `msw` packages that only it used. This cleanup
  accepted some gate findings locally, and those records are not kept.
- `CODING_STYLE.md` is now `CODING_STANDARDS.md`, the area files moved to
  `docs/coding-standards/`, and the skill is now `improve-coding-standards`.
- `AGENTS.md` points at `CODING_STANDARDS.md` and no longer has the Herdr session-naming
  section.

## Validation

- `bun run check` passed on macOS: formatting, Clippy with warnings denied, the dependency
  graph, release-version tests, 520 Rust tests (1 skipped), and 40 tooling tests
  after the gate tests were removed.
- `cargo check --target x86_64-pc-windows-msvc` is blocked on macOS. `ring` cannot compile
  its C code for MSVC, and `usvfs-sys` needs the Windows SDK header `Windows.h`. CI must
  check the Windows-only edits in `usvfs/mod.rs`, `windows_inputs.rs`, and
  `execution_adapter/native.rs`.
