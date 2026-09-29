# Troubleshooting — v0.1.0

## Status and output

Capture `$LASTEXITCODE` immediately after `mods` in PowerShell. Preserve stdout and stderr separately when diagnosing a command.

| Result | Exit status |
| --- | --- |
| Successful query/mutation, help/version, incomplete FOMOD Choices, or Install Plan preview | 0 |
| Ordinary application failure | 1 |
| Argument parsing failure, invalid FOMOD selection, or Environment Root selection failure | 2 |
| Managed execution: program not found | 127 |
| Managed execution: unsupported/failed launch or invalid working directory | 126 |
| Other managed execution failure | 125 |
| Cancellation | Windows `0xC000013A` (shells may display a signed value) |
| Child finished under managed execution | Child's actual 32-bit exit status |

Failures before execution dispatch can still use general statuses. Output write/flush failure, including a broken pipe, produces status 1. A child can itself return 125–127; distinguish the source using stderr, not the number alone. A nonzero child status does not by itself mean the manager failed to launch it.

Successful mutations are quiet except warnings. [Installation outcomes](installation.md) must be distinguished before claiming completion. Conflict queries may return invalid resolution with status zero; inspect their report.

## Common failures and safe next steps

Errors normally begin `error [code]: message`. Optional details include phase, field, choice group/option/sequence, or expected/actual build IDs. Preserve those details; a raw internal cause chain is not part of public CLI output.

| Error or symptom | Next step |
| --- | --- |
| `environment_already_initialized`, `environment_root_not_empty`, `environment_root_unsafe` | Confirm Environment Root and inspect existing contents; do not wipe it |
| `environment_invalid`, settings/override errors | Inspect manifest and `config list`; only `MODS_GAME_DIR` is a supported `MODS_*` setting variable; malformed/unknown overrides can fail validation |
| Game Installation not found/invalid | Check Steam installation and effective/stored Game Binding; ask before updating it |
| `invalid_mod_name`, `mod_already_exists`, `mod_not_found` | Check name and replacement intent; do not silently rename or replace |
| Unsafe/unsupported archive or installer, unmet dependency | Inspect package provenance/layout and error details; do not bypass validation |
| `invalid_selection` | Use current returned FOMOD IDs, ordered complete choices, and cardinality |
| Invalid Data path or invalid conflict resolution | Use a relative path beneath Data and read scoped problems |
| Invalid Output Target | Select Overwrite by omission or an existing enabled Data Mod |
| `program_not_found`, `program_unsupported`, `program_launch_failed`, `invalid_working_directory` | Check actual executable path, PATH lookup, arguments, supported target, and cwd |
| `vfs_failed`, `execution_supervision_failed` | Check matching native runtime/prerequisites and collect diagnostics; do not retry execution without authorization |
| Execution failure with `retained_execution_inis` | After all managed processes have stopped, inspect retained INI edits before deciding any manual cleanup; do not discard them blindly |
| Export failure with `retained_partial_output` | Inspect the named destination folder before manual cleanup or retry; it can hold partial files |
| `manual_cleanup_required` | The Environment Root `temp` folder is not empty, for example because of retained execution INIs. Stop and inspect it with the user; there is no documented automatic cleanup command |

Cancellation can leave partial filesystem state. Preserve it for inspection rather than automatically cleaning or rolling back. For execution, first Ctrl-C requests cancellation; a second requests force. Managed supervision permits a grace interval (about five seconds) before forced termination. Do not claim immediate rollback or cleanup.

## Diagnostic Sessions

Each environment-bound CLI command normally creates a **Diagnostic Session** at `<Environment Root>\logs\<UUID>.jsonl`. The session includes nested work and cleanup, not a saved FOMOD workflow. Failure output can include `diagnostic session: UUID` for correlation.

```powershell
mods --environment 'D:\Mod Environments\Mojave' --log-level debug config list
mods --environment 'D:\Mod Environments\Mojave' --log-level off conflicts list
```

Use the first example to gather diagnostics through a query; repeating a mutation or program still requires its authorization. `--log-level off` disables session logging. A logging setup failure warns `warning: diagnostic session logging is unavailable`; the command continues, so it is not by itself the operation's failure. JSONL diagnostic files are distinct from CLI output; they do not imply a `--json` flag.

Report version, exact command with sensitive values redacted, selected Environment Root, status, stderr, and session ID. Inspect logs locally and redact personal paths or other sensitive content before sharing. Do not upload entire archives, saves, or logs automatically.


## Report a defect

Use the [defect report form](https://github.com/Reilley64/mods/issues/new?template=defect.yml). It asks for the CLI version, system details, reproduction steps, expected and actual behavior, exit status, and diagnostic evidence. Reports and attachments are public. Agents must obtain user approval before submitting a report or uploading evidence unless the user has already authorized that action.

1. Record the failing command, stdout, stderr, and exit status. In PowerShell, capture `$LASTEXITCODE` immediately after `mods`. Keep any `diagnostic session: UUID` line.
2. Find that command's `<Environment Root>\logs\<UUID>.jsonl`. Without `--environment`, look under `%LOCALAPPDATA%\mods\environments\default\logs`. If stderr has no session ID, correlate the filename and session records with the command and its time. Do not assume the newest file belongs to the failure.
3. Copy the relevant records to a separate file for review. Include session boundary events and records around the failure when available. Redact personal paths, usernames, credentials, and private command arguments consistently. Preserve timestamps, event names, error codes, and the session UUID where present. Keep the original log locally.
4. Paste the reviewed excerpt into the form's diagnostic field as a fenced code block, or attach the reviewed copy. If GitHub rejects `.jsonl`, use a `.txt` copy. State whether the evidence is an excerpt. Share only the affected session, not the entire logs directory, saves, or mod archives.

A defect report does not require logs. If `--log-level off`, a logging setup failure, or an early startup failure prevented them, explain that in the diagnostic field. If more detail is needed, an authorized reproduction can use `--log-level debug`. A separate `config list` run creates a different Diagnostic Session and does not recover the failed command's logs. Do not repeat a mutation or program execution just to gather evidence without authorization.
