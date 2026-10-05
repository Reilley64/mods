# File Conflicts and managed execution — v0.1.0

## Read-only conflict analysis

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' conflicts list
mods --json --environment 'D:\Mod Environments\Mojave' conflicts inspect 'Mojave Textures' --compare-content
mods --json --environment 'D:\Mod Environments\Mojave' conflicts explain 'textures\weapons\rifle.dds' --compare-content
```

- `list` reports effective File Conflicts among active providers, not every override of Steam Data.
- `inspect MOD_NAME` reports a Data Mod's participation, provider summary, and conflict rows. Disabled Data Mods are not active providers; inspect the reported participation rather than assume they are enabled.
- `explain PATH` takes a **Data-relative** file path, not an absolute path or a `Data\`-prefixed path. It reports the effective result, provider stack, Tombstone effects, reasons, comparisons, and problems.
- All three support optional `--compare-content` for SHA-256 comparisons. Without it content is not compared. States include `not_compared`, `same_sha256`, `different_sha256`, `unavailable`, and `unstable`. Matching bytes are not a game-compatibility guarantee.

A **File Conflict** is two or more unsuppressed active non-base providers with ordinary files at the same case-insensitive Data-relative path. **Mod Priority** follows Mod Organizer 2 `modlist.txt` order: the first listed mod has the highest priority and the last has the lowest. It is not plugin load order. There is no migration: a list written low-to-high by earlier mods versions now reads reversed, so reverse its mod entries by hand. Only enabled Data Mods participate; **Overwrite** has implicit highest priority. Physical BSA files are opaque ordinary files; their internal members are not analyzed.

A **Tombstone** is provider-owned metadata suppressing a lower file or inclusive subtree in analysis without supplying a file. If no higher file overrides it, analysis can report an absent effective path. This does not delete Steam Data and does not guarantee runtime suppression.

Read `resolution_status` and scoped `problems`, not just status zero or row count. An invalid analytical resolution can be returned as a successful query. An empty or clean conflict report is not proof that plugins, BSA members, or runtime behavior are correct. These commands do not resolve conflicts or reorder mods.

## Execute an authorized program

A **Virtual Game View** merges a Game Installation, enabled Data Mods, and Mod Environment-owned files for a game or tool. `exec` starts a program with managed mappings; it is not a security sandbox. Approve the program, arguments, working directory, and write intent before running it.

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' exec -- 'D:\SteamLibrary\steamapps\common\Fallout New Vegas\FalloutNV.exe'
# Named target must already be installed AND enabled:
mods --json --environment 'D:\Mod Environments\Mojave' exec --output-target 'Tool Output' --cwd 'D:\Tools' -- 'D:\Tools\Tool.exe' '--example-argument'
```

The tool and `--example-argument` above are placeholders: use the actual tool's supported arguments. `--` before PROGRAM is required; everything after it belongs to the child, even tokens resembling mods options.

- Omit `--output-target` to use **Overwrite**. `--output-target NAME` selects an installed, enabled Data Mod; it does not create or enable one. Do not pass `Overwrite` as a special target name: omission selects it.
- An **Output Target** receives new Data files and new copy/file-move destinations for this execution. It does not change Mod Priority or relocate an existing destination from its provider. Do not promise that every write goes to the Output Target.
- Without `--cwd`, the child starts in the bound Game Installation directory, because the game and its script-extender loaders resolve `Data\` from their working directory. `--cwd PATH` selects another directory; a relative PATH resolves against the caller's startup directory. The working directory never changes executable lookup. It does change how the child reads relative paths: without `--cwd`, relative program arguments and files the child opens by relative path resolve against the game directory, not the caller's directory. Pass absolute argument paths, or pass `--cwd`.
- Path-like PROGRAM values resolve from the caller's startup directory. Bare names search inherited PATH, not an implicit current directory. If no extension is supplied, `.exe` fallback is supported. Prefer an absolute `.exe` path. Scripts and shell syntax are not implicitly interpreted; unsupported targets fail. Command-line size and NUL validation also apply.
- Without `--hidden`, child stdout and stderr are forwarded live and byte-for-byte in both modes; stdin remains inherited. `--json` applies to pre-launch CLI failures, but a launched child has no JSON success wrapper. The CLI returns the child's exit status after managed supervision; read [statuses](troubleshooting.md) to distinguish launch/management errors from child failures.
- `exec --hidden` (Windows only) detaches the launcher console, captures child output privately instead of forwarding it, and shows launch or management failures in a dialog. The exit status is the only machine signal. `--hidden` cannot be combined with `--json`: the CLI rejects it as `invalid_arguments` with status 2 before it detaches or launches anything. On other platforms, `--hidden` fails with `program_unsupported` and status 126.

## Create a Launch Shortcut

On Windows, `shortcut` writes a desktop `.lnk` that later runs `mods exec --hidden` with the same Output Target, working directory, child arguments, and log level. It does not start the program. It writes a file and can replace a same-named `.lnk`, so get the user's approval first.

```powershell
# After approval:
mods --json --environment 'D:\Mod Environments\Mojave' shortcut --name 'Mojave Game' -- 'D:\SteamLibrary\steamapps\common\Fallout New Vegas\FalloutNV.exe'
```

- The default destination is the user's Desktop. `--destination PATH` must be an existing directory. `--name` is a filename stem, not a path.
- A same-named valid `.lnk` is replaced. Directories, symbolic links, and other files are not replaced.
- Success prints nothing in text mode, and `{"warnings": [...]}` with `--json`.
- The shortcut stores `exec --hidden` without `--json`, so a launch from it never emits JSON. Launch failures appear in a Windows dialog.
- The shortcut uses the environment's current mods and settings. Recreate it after moving the executable, its directory, or the Environment Root.

## Runtime limits

The released usvfs setup maps provider roots, not every per-file analytical winner. Tombstone suppression in conflict/installation analysis is not guaranteed during execution. Plugin diagnostics describe the analytical Data projection, not an observed runtime view. Canonical Profile State supplies mappings; analytical lists do not rewrite those files.

## Load order through file times

The game orders plugins and BSAs by modification time, and usvfs shows each file with its own time. So before it launches the program, `exec` sets the modification times of the winning plugins (`.esm`, `.esp`) and BSAs at the Data root. It changes the **real files**: files in the Game Installation's `Data` folder, in Data Mods, and in Overwrite. Read-only files are supported, and they stay read-only. Files that already have their time are not changed. Other files keep their times.

- Times start at 2000-01-01 00:00 UTC and add one minute per position.
- BSAs named in the derived `sArchiveList` come first, in list order. The list starts with `Fallout - Invalidation.bsa`, followed by the list from `FalloutCustom.ini` or `Fallout.ini`.
- Other BSAs that no plugin loads come next, in their current modification-time order.
- `FalloutNV.esm` always comes first among the plugins, because the game always loads it first, even when `plugins.txt` lists it later or not at all.
- The other plugins follow in `plugins.txt` line order. A UTF-8 byte order mark at the start of `plugins.txt` is removed before the file is read, so it never becomes part of the first plugin name. Present plugins that `plugins.txt` does not list are inactive; they come after the listed ones, in their current modification-time order, and the game does not load them.
- A BSA whose name starts with a plugin's name (case-insensitive) gets that plugin's time. If more than one plugin name matches, the longest one wins.

`plugins.txt` lists the active plugins. Its line order is the load order. To change the load order, reorder its lines. Profile State has no `loadorder.txt`; an existing one is ignored. `exec` sets the times again from `plugins.txt` on every run. A tool that sorts plugins only by changing their times, for example LOOT or xEdit run under `exec`, is reverted on the next run unless it also reorders `plugins.txt`. Mod Environments that share a Game Installation set the times of its plugins and BSAs on each run, so do not run two of them at the same time.

If reading or setting a time fails, `exec` stops before it stages profile INIs or launches anything, and nothing is retained. The error reports `phase = load_order` and names the file as `load_order_file`; see [troubleshooting](troubleshooting.md).

Report stale and duplicate `plugins.txt` entries and invalid retained Profile State warnings accurately. Do not promise safe execution from a preview or conflict report, or claim that root mapping and all write/delete cases follow the analytical model.
