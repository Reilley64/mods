# Data Mod installation — v0.1.0

A **Data Mod** contributes only beneath the game's `Data` directory. Use archives intended for Data installation, not executable/root patch installation. The release recognizes ZIP, 7z, and RAR signatures; an extension alone does not make an archive supported or safe. Unsafe paths, ambiguous layouts, unsupported installers, and unmet dependencies can be rejected.

## Preview, then install

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures.zip' --name 'Mojave Textures' --dry-run
# After installation approval:
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures.zip' --name 'Mojave Textures'
```

`ARCHIVE` is required. `--name NAME` overrides the archive filename with its final extension removed. The **Mod Name** is case-insensitively unique. Use a valid directory name; reserved/invalid names fail instead of being silently repaired.

An **Install Plan** describes validated source-to-Data destinations, winner decisions, and overlaps on planned paths. It is neither an installed mod nor a complete File Conflict report. Inspect `plan.archive_identity`, `plan.mod_name`, `plan.replacement`, choices, warnings, candidates, and `plan.projected_state` before proceeding. A later invocation recomputes the plan; a preview does not freeze the archive or environment.

New installs append to the mod list at the next Mod Priority and are **initially disabled**. Installation success does not mean participation in the Virtual Game View. There is no released enable command; do not invent one or silently edit `profile/modlist.txt`.

## Replacement

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures-update.7z' --name 'Mojave Textures' --replace --dry-run
# Only after replacement is authorized:
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures-update.7z' --name 'Mojave Textures' --replace
```

`--replace` requires an existing Data Mod. It retains the existing canonical Mod Name, Mod Priority, list position, and enabled state. Without `--replace`, a matching name fails with `mod_already_exists`; replacement of a missing name fails with `mod_not_found`. Never add `--replace` automatically to bypass a collision.

## FOMOD Choices: resubmit complete state

1. Start with `mods --json install ARCHIVE --dry-run` and inspect stdout. If `outcome` is `"additional_selections_required"`, read every unresolved group's `id`, label, description, cardinality, and options. Use actual returned IDs, not display labels.
2. Ask the user for unresolved preferences. Select only options marked selectable and respect cardinality. Optional groups can expose a synthetic `none`; use that returned option ID when appropriate, rather than inventing an empty selection.
3. Repeat `--choice 'GROUP=OPTION'` in order. Each invocation carries **all previously accepted choices plus the new choices**. There is no persisted installation session. Values are not trimmed; copy IDs exactly. Changing an earlier choice can change which later groups exist.
4. Repeat the preview until `outcome` is `"preview"`. Review the completed Install Plan. After installation approval, rerun with the full choice list and the same archive/name/replacement intent, omitting `--dry-run`. Inspect the outcome again; changed inputs may require choices again.

Illustrative syntax only, assuming the output actually returned `visuals`, `high`, `extras`, and `none`:

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Fomod.zip' --choice 'visuals=high' --dry-run
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Fomod.zip' --choice 'visuals=high' --choice 'extras=none' --dry-run
# After approval, preserve the COMPLETE accepted list:
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Fomod.zip' --choice 'visuals=high' --choice 'extras=none'
```

An `invalid_selection` error can include `field`, `group_id`, `option_id`, and zero-based `sequence`. Correct invalid, repeated, out-of-order, hidden, or unselectable choices from current output; do not keep sending only the newest choice. Unsupported installer behavior is not a reason to bypass validation.

## Distinguish success outcomes

| Status and stdout | Meaning | Next action |
| --- | --- | --- |
| 0, JSON `outcome` is `"additional_selections_required"`, or text `outcome = "additional_selections_required"` | Choices incomplete; no completed installation | Resubmit complete choices |
| 0, JSON `outcome` is `"preview"`, or text `outcome = "preview"` | Completed Install Plan only; no publication | Review and obtain installation approval |
| 0, JSON `outcome` is `"installed"`, or empty text stdout from a non-dry-run install | Completed installation | Report completion and `warnings` (stderr warning lines in text mode); new install remains disabled |
| Nonzero | Failed/cancelled operation | Read the stderr Problem Details (or the text `error [code]` lines) and [troubleshooting](troubleshooting.md); preserve partial state |

Incomplete choices can occur with or without `--dry-run`. Warnings can appear in every outcome. Never equate exit zero alone with installed files.
