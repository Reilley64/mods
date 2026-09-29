# File Conflicts and managed execution — v0.1.0

## Read-only conflict analysis

```powershell
mods --environment 'D:\Mod Environments\Mojave' conflicts list
mods --environment 'D:\Mod Environments\Mojave' conflicts inspect 'Mojave Textures' --compare-content
mods --environment 'D:\Mod Environments\Mojave' conflicts explain 'textures\weapons\rifle.dds' --compare-content
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
mods --environment 'D:\Mod Environments\Mojave' exec -- 'D:\SteamLibrary\steamapps\common\Fallout New Vegas\FalloutNV.exe'
# Named target must already be installed AND enabled:
mods --environment 'D:\Mod Environments\Mojave' exec --output-target 'Tool Output' --cwd 'D:\Tools' -- 'D:\Tools\Tool.exe' '--example-argument'
```

The tool and `--example-argument` above are placeholders: use the actual tool's supported arguments. `--` before PROGRAM is required; everything after it belongs to the child, even tokens resembling mods options.

- Omit `--output-target` to use **Overwrite**. `--output-target NAME` selects an installed, enabled Data Mod; it does not create or enable one. Do not pass `Overwrite` as a special target name: omission selects it.
- An **Output Target** receives new Data files and new copy/file-move destinations for this execution. It does not change Mod Priority or relocate an existing destination from its provider. Do not promise that every write goes to the Output Target.
- Without `--cwd`, the child starts in the bound Game Installation directory, because the game and its script-extender loaders resolve `Data\` from their working directory. `--cwd PATH` selects another directory; a relative PATH resolves against the caller's startup directory. The working directory never changes executable lookup. It does change how the child reads relative paths: without `--cwd`, relative program arguments and files the child opens by relative path resolve against the game directory, not the caller's directory. Pass absolute argument paths, or pass `--cwd`.
- Path-like PROGRAM values resolve from the caller's startup directory. Bare names search inherited PATH, not an implicit current directory. If no extension is supplied, `.exe` fallback is supported. Prefer an absolute `.exe` path. Scripts and shell syntax are not implicitly interpreted; unsupported targets fail. Command-line size and NUL validation also apply.
- Child standard streams are inherited. The CLI returns the child's exit status after managed supervision; read [statuses](troubleshooting.md) to distinguish launch/management errors from child failures.

## Runtime limits

The released usvfs setup maps provider roots, not every per-file analytical winner. Tombstone suppression in conflict/installation analysis is not guaranteed during execution. Plugin diagnostics describe the analytical Data projection, not an observed runtime view. Projected plugin order is advisory and is **not enforced through virtual timestamps**. Canonical Profile State supplies mappings; analytical lists do not rewrite those files.

Report `load_order_not_enforced`, stale/duplicate plugin entries, unlisted plugins, and invalid retained Profile State warnings accurately. Do not promise safe execution from a preview or conflict report, or claim that root mapping and all write/delete cases follow the analytical model.
