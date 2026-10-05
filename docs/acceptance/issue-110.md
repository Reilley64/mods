# Issue #110: Launch Shortcuts

## Scope and implementation

Stacked on PR #111 at `c64f641ea7a82df4e4a167d5d735193d8193b247`.
The existing CLI creates native Windows `.lnk` files targeting `exec --hidden`.
Creation validates but does not execute; launch reloads current environment state.
No new executable, workspace crate, external dependency, or permanent platform
harness was added. See [usage](../usage.md).

## Automated evidence

- Initial complete macOS `bun run check`: 399 Rust tests, 102 tooling tests,
  2 release-version tests, formatting, strict Clippy, and dependency checks passed.
- Initial complete Windows snapshot: x64 workspace check/build/strict Clippy,
  425 x64 workspace tests and 46 x86 execution-adapter tests passed.
- Review found that diagnostic setup failure could print a warning after successful
  creation. A failing runner regression reproduced it. The repair suppresses this
  warning only for successful shortcut creation, retaining actual failure diagnostics.
  Four focused CLI shortcut tests and host Clippy passed afterward; Windows focused
  shortcut tests, Clippy, and rebuild also passed.
- Desktop testing exposed `IShellLinkW::SetPath` rejecting canonicalized verbatim
  paths with `0x80070057`. A new native regression reproduced the failure. The
  approved repair reuses the existing lossless DOS/UNC conversion, rejecting
  components whose meaning would change. Only Shell Link path fields are adapted;
  saved CLI arguments remain unchanged. All 47 execution-adapter tests passed on
  both x64 and x86 afterward, with Windows strict workspace Clippy and CLI rebuild.
- Final format, staged whitespace, and dependency checks passed. Complete-suite
  counts above describe the pre-repair snapshots; repairs received focused checks.
- A user-approved refactor applies the "Reusable capability ports" rule. The
  shortcut-named `ValidateShortcutLaunch` workflow port is split into single-step
  ports. `create_shortcut` and `execute_program` now compose the same
  `ResolveLaunchInputs` and `PrepareExecutionEnvironment` ports. Shortcut also
  reuses `LoadSettings`. Only `LocateLauncher`, `LocateEnvironmentRoot` and
  `PersistShortcut` stay shortcut-specific. `RunManagedProgram` now receives the
  resolved launch and prepared environment, so exec no longer resolves or
  prepares inside the run step. After the refactor, macOS `bun run check` passed:
  409 Rust tests, 137 tooling tests and 2 release-version tests. The Windows-only
  paths of this refactor were not compiled on the macOS host; Windows CI covers them.
- The integration with the shared export and exec preparation replaced those ports.
  `create_shortcut` now composes `ResolveLaunchTarget`, `PrepareEnvironmentPlan` and
  `ProjectProfile`, the same ports as `exec`, and receives the command's loaded
  settings instead of `LoadSettings`. Without `--cwd`, it saves the bound Game
  Installation directory, the same default as `exec`.

## Native desktop acceptance

Passed on the authorized limited-privilege interactive Windows desktop, using a
fresh synthetic Steam installation and Mod Environment. The inert `FalloutNV.exe`
was never launched. A test-owned C# recorder observed exact argument values,
working directory, and the virtual game Data marker.

Candidate `mods.exe` SHA-256:
`5d940dad93af3de59af5f56743a9edd313762777536aaf2de653898034b75ecb`.
The four runtime files were matched to the previously verified pinned native
bundle and copied with SHA-256 checks.

Passed checks:

- Quiet creation without executing the recorder.
- Absolute launcher, environment, program and working-directory capture; custom
  relative destination, default root-folder name and explicit display/name choices.
- Executable icon and explicit log-level/output-target preservation.
- Actual shell activation of `.lnk` files, equivalent to desktop double-click.
- Exact empty, Unicode (`雪 café`), quote, space, trailing-backslash and literal
  `--`/`--environment` child arguments, checked as UTF-16 Base64 records.
- The default cwd of that build (the creation-time working directory) and an explicit
  cwd, despite launching from another cwd. The approved default is now the bound
  Game Installation directory, as with `exec`; that default was not run on this desktop.
- The same unchanged link reads `BASE`, then `CURRENT` after enabling a synthetic
  Data Mod: current-state behavior, not a snapshot.
- Existing-link replacement; failed validation preserves its previous bytes;
  directories, invalid names, missing programs and invalid environments rejected.
- Actual current-user Desktop placement and launch of one uniquely named owned link.

The first script attempt stopped before CLI invocation because PowerShell's `cli`
alias shadowed its helper; renamed the helper. The next attempt found the real
verbatim-path product defect described above. After that repair, the first launch
passed but a later launch correctly rejected a synthetic mod missing `meta.toml`.
The test-owned error dialog was recorded and closed, yielding status 125. Fixed
that fixture only. A fresh final run passed through `15-desktop-launch`.
Failed attempts are retained, not represented as uninterrupted success.

Evidence remains in the desktop account's `mods110-desktop-04/evidence`, including
`success.json`, `*.decoded.json`, `*.link.json`, `protected.result.json` and
`cleanup.json`. Earlier attempts use suffixes `01` through `03`. Throwaway scripts
remain under matching `mods110-inputs-*` directories.

Cleanup confirmed no owned processes, no temporary tasks, and no owned Desktop
shortcut remained. Checked real profile configuration files and Steam manifest,
and all physical synthetic game files, were unchanged. This is not a full real
save/game-tree hash audit. No screenshots or transient-window exclusion claimed.
Native preparation failure coverage includes oversize argument rejection; no
injected OS rename failure or concurrent-user race claim is made.

## Final reviews

Separate standards and spec reviews found no remaining blockers after the
approved canonical-path repair. The spec review also inspected the final native
receipts. Hidden console/dialog behavior remains the existing #109 policy; these
shortcut receipts do not independently prove visual window behavior.

## Advisory coding-style findings: explicitly accepted

This is **not a clean gate result**. The standards review independently checked
these findings. No enforce-mode override was used or authorized. Paths below are
relative to `src/`.

| File | Rule | Reason for acceptance |
| --- | --- | --- |
| `presentation/cli/src/runner.rs` | Callable port invocation | Calls an application use-case function; actual callable ports use `.call`. |
| `infrastructure/dependencies/src/create_shortcut.rs` | Use-case parameters | Infrastructure factory, not an application use case. |
| `application/src/shortcut/types.rs` | Presentation error allowlists | Internal error contexts are mapped to fixed CLI strings, not printed raw. |
| `infrastructure/execution/src/shortcut.rs` | Test public behavior | Tests exercise persistence through its public adapter seam and inspect saved fields; canonicalized-path test reproduces an observed project defect. |
| `application/src/shortcut/create_shortcut/name.rs` | Phase spacing | Sanitization, reserved-name handling, rejection and output have distinct phases. |
| `infrastructure/execution/src/launch_inputs.rs` | Focused use-case orchestration | Existing infrastructure encoder only gains crate-private visibility; no application algorithm moved here. |
| `presentation/cli/src/runner.rs` | Phase spacing | Conversion, application invocation and result/diagnostic reporting are separated. |
| `presentation/cli/src/runner.rs` | Use-case parameters | Presentation dispatch, not an application use case; forwards business values separately. |
| `presentation/cli/src/runner.rs` | Test public behavior | Tests exercise the agreed CLI runner seam, including quiet success versus failed-command diagnostics. |
| `infrastructure/execution/src/shortcut.rs` | Phase spacing | COM, destination, encoding, staging, verification and publication are distinct blocks. |
| `presentation/cli/src/main.rs` | Phase spacing | Existing composition phases remain; shortcut only joins the mutating-command classification. |
| `application/src/shortcut/mod.rs` | Use-case declaration order | Re-export module; owning use-case file has the required declaration order. |
| `presentation/cli/src/main.rs` | Use-case parameters | Presentation composition, not an application use case. |
| `application/src/shortcut/create_shortcut.rs` | Phase spacing | Name validation, launch and environment validation, saved arguments and publication are separated. |
| `infrastructure/execution/src/shortcut.rs` | Narrow custom implementations | Narrow missing Shell Link adapter reuses Windows APIs, tempfile and existing encoding/path conversion. |
| `infrastructure/dependencies/src/create_shortcut.rs` | Dependency direction/composition roots | Existing composition crate implements application-owned ports; deterministic dependency check passes. |
| `infrastructure/dependencies/src/create_shortcut.rs` | Use-case declaration order | Infrastructure factory/adapter, not the owning application use-case file. |
| `application/src/shortcut/types.rs` | Use-case parameters | Port aliases/data types, not use-case entry points. |
| `infrastructure/execution/src/shortcut.rs` | Use-case parameters | Native adapter boundary, not application orchestration. |
| `presentation/cli/src/runner.rs` | Use-case declaration order | No application use case declared in presentation dispatch. |

The user-approved port split removed two earlier dispositions for
`infrastructure/dependencies/src/create_shortcut.rs`: "Phase spacing" and "Narrow
custom implementations". That file now only composes ports, and the validation
workflow it held no longer exists.

Earlier notices are also accounted: application cause preservation uses `.context`;
application use-case dependencies are first, with separate business inputs; initial
TDD stubs were replaced by readable orchestration. Native guard clauses distinguish
present/missing/error cases and reject bad metadata early. `error.rs` separates
marker fallback from shortcut mapping. Existing runner cancellation is forwarded
directly; no infrastructure checkpoint was moved into application code.
All findings in `worktree-quiet-river-8945` were excluded as another worktree's work,
not judged compliant or cleared. No files in that worktree were edited.
