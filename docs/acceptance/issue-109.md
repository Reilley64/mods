# Issue #109: hidden managed execution

## Revision and scope

Validated implementation commit `552540f` on Windows using the pinned
`nightly-2026-08-21` toolchain and `cargo-nextest 0.9.145`. This adds
`mods exec --hidden` to the existing executable; shortcut creation remains #110.
A brief startup console flash and a console-only child's own window are accepted.

The debug `mods.exe` SHA-256 was
`c489785fb28e99935ad5d26c37b647df713eb703b42b934ee4197e5fe86cf8f2`.
The native runtime came from the repository's verified pinned bundle.

Windows CI initially rejected `WindowsError::from_win32()` with E0599. Commit
`552540f` uses the pinned crate's `from_thread()` constructor. The subsequent
[Windows CI run](https://github.com/Reilley64/mods/actions/runs/36415127166) passed.

## Automated Windows checks

All passed under the authorized build account:

- x64 workspace check, including all targets;
- x64 CLI build;
- x64 workspace Clippy, all targets/features, warnings denied;
- x86 execution-adapter check;
- x64 workspace nextest: **418 passed**, including the hidden runner test;
- x86 execution-adapter nextest: **44 passed**;
- CLI help exposes `--hidden` and its Windows-only behavior.

Build logs remain in the build account's
`mods-109-validation-552540f/{build,lint,x86-check,tests}.log`.

## Interactive desktop evidence

The owner authorized the existing interactive desktop account. Temporary,
limited-privilege tasks ran in desktop session 1. Tests used a fresh synthetic
Game Installation and Mod Environment, never the real game executable. A
uniquely titled, test-owned Windows Forms child supplied the GUI observations.

| Check | Observed result |
| --- | --- |
| Hidden nonzero exit | Child status 37 preserved; no modal error dialog; no launcher stdout/stderr. |
| Ordinary execution | Expected stdout/stderr markers and child status 23 preserved. |
| Shared caller console | Caller stayed visible and attached while the live hidden supervisor was absent from its console membership. |
| GUI and supervision | Test child window was visible in session 1; supervisor stayed alive until its owned close control was activated, then exited 0. |
| Missing program | Visible error dialog contained `program_not_found` and the actual existing diagnostic JSONL path; dismissal yielded status 127. |
| Ordinary Ctrl-C | A signal confined to the test-owned console ended managed execution with `0xC000013A`; caller console stayed visible. |
| Standalone hidden launch | Shell-launched supervisor remained alive with a visible GUI child. A detached observer could not attach to the supervisor's console (`ERROR_INVALID_HANDLE`, 6), confirming no console was attached. Closing the GUI yielded supervisor status 0. |

The first controller stopped because it assumed the error dialog's OK control
had ID 1; this Windows dialog used ID 2. The dialog and its text had already been
verified. A separate, ownership-checked close request dismissed that exact live
dialog and captured status 127. This was a test-control failure, not a product
failure. The original failure receipt is retained alongside the recovery result.

Evidence remains under the desktop account's
`mods109-desktop-552540f-01/evidence/`, including `04-observation.json`,
`05-dialog.json`, `05-dismissal.json`, `final/normal-ctrl-c.json`,
`final/standalone-observation.json`, `final/standalone-exit.json`, and `cleanup.json`.
The one-off scripts remain in `mods109-inputs-552540f/`; no platform harness was
added to the repository.

## Limits and cleanup

- Visibility was checked through native window ownership/visibility APIs, not screenshots.
- Hidden nonzero-window observation sampled at 100 ms; it cannot exclude a transient window between samples. The process exited without dialog intervention.
- Small real profile configuration files and the Steam manifest were unchanged before/after. This is not a full game-tree or save-content hash audit.
- No test-owned processes remained. All temporary test tasks were removed; evidence was retained.
- The six advisory coding-style findings remain explicitly accepted as false positives with file/rule/reason accounting in the [review handoff](https://github.com/Reilley64/mods/issues/109#issuecomment-5868722382). This is not a clean gate claim.
