# Data Mod installation — v0.1.0

A **Data Mod** contributes only beneath the game's `Data` directory. Use archives intended for Data installation, not executable/root patch installation. The release recognizes ZIP, 7z, and RAR signatures; an extension alone does not make an archive supported or safe. Unsafe paths, ambiguous layouts, unsupported installers, and unmet dependencies can be rejected.

## Preview, then install

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures.zip' --name 'Mojave Textures' --dry-run
# After installation approval:
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures.zip' --name 'Mojave Textures'
```

`ARCHIVE` or a supported Nexus URL is required. `--name NAME` overrides the archive filename with its final extension removed. The **Mod Name** is case-insensitively unique. Use a valid directory name; reserved/invalid names fail instead of being silently repaired.

An **Install Plan** describes validated source-to-Data destinations, winner decisions, and overlaps on planned paths. It is neither an installed mod nor a complete File Conflict report. Inspect `plan.archive_identity`, `plan.mod_name`, `plan.replacement`, choices, warnings, candidates, and `plan.projected_state` before proceeding. A later invocation recomputes the plan; a preview does not freeze the archive or environment.

New installs go at the top of `modlist.txt`, after any leading `#` comment lines, with the highest Mod Priority, and are **initially disabled**. There is no migration: a list written low-to-high by earlier mods versions now reads reversed, so reverse its mod entries by hand. Installation success does not mean participation in the Virtual Game View. There is no released enable command; do not invent one or silently edit `profile/modlist.txt`.

## Nexus URL input

`install` also accepts a New Vegas Nexus mod-page URL or a file-specific URL. Local archive inputs still work without Nexus credentials.

```powershell
mods --environment 'D:\Mod Environments\Mojave' install 'https://www.nexusmods.com/newvegas/mods/12345' --dry-run
mods --environment 'D:\Mod Environments\Mojave' install 'https://www.nexusmods.com/newvegas/mods/12345' --file 67890 --dry-run
mods --environment 'D:\Mod Environments\Mojave' install 'https://www.nexusmods.com/newvegas/mods/12345?tab=files&file_id=67890' --dry-run
```

These IDs are examples. An explicit URL file ID or `--file ID` selects that file. Conflicting IDs fail. Without a file ID, the command selects only when exactly one available Main file exists. Otherwise it lists available file IDs, names, versions, and categories and requires `--file`. With `--json`, this is a `nexus_file_selection_required` Problem on stderr with status 2 and `details.files` in the published order. It does not guess by date or version. Other games, NXM links, and malformed URLs are unsupported.

New downloads require a Premium account API key. Add optional `nexus_api_key` to the selected environment's `mods.toml`, or supply `MODS_NEXUS_API_KEY` in the process environment. The environment variable overrides the stored key. Storage in `mods.toml` is plaintext. Do not share that file with a key in it. `config get` and `config list` never return the key, and there is no key argument or setter command. Changing `game-dir` preserves the stored key.

If a new download lacks credentials or the account is not Premium, install a local archive instead with `mods install <archive>`. Invalid credentials, rate limits, unavailable files, and network failures produce separate errors. The command does not open a browser.

Completed archives and source metadata stay in `cache/downloads/newvegas-MOD_ID-FILE_ID/` within the environment. Only the exact selected file can reuse that entry. An explicit file input with a complete entry works without network access or authentication. A mod-page input resolves current metadata on every invocation, including repeated FOMOD Choice commands. Failed resolution never selects from stale metadata. A newly selected file goes through the ordinary choice flow; the cache does not remember choices or pin a previous selection. If a completed entry has malformed metadata, a different Nexus identity, or an archive of the wrong size, the command fails with `environment_invalid` and `phase = download_cache`. It does not delete or replace the entry. Delete that entry to download the file again.

A remote `--dry-run` may download and retain an archive. It does not install a Data Mod or change Profile State. Transfer failure or cancellation removes partial bytes. Completed archives remain after installation success or failure. There is no automatic retry, resume, or eviction.

The default Nexus Mod Name is the selected file's display name, or the mod-page title if the file name is empty. `--name` overrides it. Invalid names require correction. Name collisions require a different name or explicit `--replace`; matching Nexus IDs do not authorize replacement. All existing archive safety, FOMOD, dependency, and Game Installation protection rules still apply.

Successful URL installations write a `[nexus]` table in the Data Mod's `meta.toml`. It contains `game_domain`, `mod_id`, `file_id`, `file_version`, `mod_version`, `mod_name`, and `file_name`. File and mod-page version strings remain distinct and unchanged. Credentials and temporary download URLs are never provenance. Installing a local path writes no Nexus provenance, even when the path points into the managed cache. Installation does not check for updates or enable the mod.

## Replacement

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures-update.7z' --name 'Mojave Textures' --replace --dry-run
# Only after replacement is authorized:
mods --json --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures-update.7z' --name 'Mojave Textures' --replace
```

`--replace` on a Data Mod listed in `modlist.txt` retains the existing canonical Mod Name, Mod Priority, list position (counted in mod entries from the top of `modlist.txt`; comment lines are not counted), and enabled state. Without `--replace`, a matching name fails with `mod_already_exists`. `--replace` on a name with no listed mod and no unlisted entry fails with `mod_not_found`. Never add `--replace` automatically to bypass a collision.

## Unlisted entries in `mods`

A folder or stray file in `mods` that `modlist.txt` does not list is ignored like a disabled mod. It contributes nothing, and `list` and conflict reports do not show it. mods never adds it to `modlist.txt`.

- Installing over its name (compared case-insensitively) fails with `mod_already_exists`, which names the entry.
- With `--replace`, the install removes the entry and lists the new mod like a new install, under the entry's spelling: at the top, disabled. The plan shows `plan.projected_state.mode = "unlisted_replacement"` (the same value in JSON).
- `--replace` is still refused with `mod_already_exists` when several entries that differ only in case match the name.
- Any replacement is refused with `unsafe_archive` when the archive lies inside the folder it would remove. Move the archive first.

Installation writes directly into `mods/<Mod Name>` and the Profile State files. There is no staging copy and no rollback. Replacement deletes the old mod folder before it writes the new files. A failure or cancellation during installation can leave a partial mod folder, a mod folder that `modlist.txt` does not list yet, or a partly updated `plugins.txt`. Inspect that state with the user before any retry. An unlisted folder left this way is ignored; with the user's approval, the same install with `--replace` takes it over.

Installing a mod does not activate its plugins. `plugins.txt` lists the active plugins. Its line order is the load order. To change the load order, reorder its lines. Replacing an enabled mod removes plugins that are no longer present from `plugins.txt` and leaves the other lines in place.

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
| 2, `nexus_file_selection_required` (JSON Problem with `details.files`, or text `error [nexus_file_selection_required]` lines) | No file selected; nothing downloaded or installed | Ask the user to pick a `file_id`, then rerun with `--file ID` |
| Nonzero | Failed/cancelled operation | Read the stderr Problem Details (or the text `error [code]` lines) and [troubleshooting](troubleshooting.md); preserve partial state |

Incomplete choices can occur with or without `--dry-run`. Warnings can appear in every outcome. Never equate exit zero alone with installed files.
