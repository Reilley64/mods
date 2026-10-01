# Windows setup

## Acquire the released CLI

Use Windows 11 x64 with WinGet available. A Steam-managed Fallout: New Vegas Game Installation supplies the clean shared base. Both x86 and x64 Microsoft Visual C++ 2015–2022 Redistributables are runtime prerequisites. Rust, a compiler, and the source archive are not required to run the release.

Install through WinGet after user approval:

```powershell
winget install Reilley64.Mods
mods --version
mods --help
```

Confirm that WinGet selects `Reilley64.Mods`; stop if it reports an ambiguous or different package. If `mods` is not immediately available, open a new terminal and check `Get-Command mods` before retrying installation.

### Release ZIP fallback

If WinGet cannot find the package, offer the [v0.1.0 release ZIP](https://github.com/Reilley64/mods/releases/tag/v0.1.0) as an alternative and obtain approval before installation. Download `mods-v0.1.0-runtime-x86_64-pc-windows-msvc.zip` and `mods-v0.1.0-runtime-SHA256SUMS`. Verify the ZIP's SHA-256 against the matching checksum file, then extract into a fresh installation directory. Stop on a mismatch.

Install both Microsoft Visual C++ redistributables if missing. Keep the complete extracted layout, including licenses and `BUILD-AND-SOURCE.md`:

```text
mods.exe
usvfs/usvfs_x86.dll
usvfs/usvfs_x64.dll
usvfs/usvfs_proxy_x86.exe
usvfs/usvfs_proxy_x64.exe
```

Do not move only `mods.exe` or substitute native files from another release. The original all-in-one `mods-v0.1.0-x86_64-pc-windows-msvc.zip` remains supported and has its own `SHA256SUMS`; it includes corresponding source. The smaller runtime ZIP has a separate source download linked in `BUILD-AND-SOURCE.md`.

Use the full executable path with PowerShell's `&` operator, or add its installation directory to PATH with user approval. Check `mods --version` and `mods --help` after setup. Do not assume Chocolatey or Scoop availability.

## Select a Mod Environment

A **Mod Environment** is one isolated setup, not a separately selectable profile. Its **Environment Root** contains `mods.toml` (the **Environment Manifest**) and managed state. **Profile State** is its one set of mod order, plugin state, INIs, and saves beneath `profile`. `plugins.txt` lists the active plugins. Its line order is the load order. To change the load order, reorder its lines. Profile State has no `loadorder.txt`; `init` does not import one, and an existing one is ignored.

- `--environment PATH` selects the Environment Root.
- Without it, the root is `%LOCALAPPDATA%\mods\environments\default`; this is not current-directory or ancestor discovery.
- Relative `--environment`, archive, `--game-install`, and `config set game-dir` paths resolve against the CLI's startup directory, not the Environment Root. Prefer absolute paths in automation.
- For executable lookup and `--cwd`, see [execution](execution.md).

## Initialize and configure

After approval, select a new Environment Root and a real Steam Game Installation:

```powershell
mods --environment 'D:\Mod Environments\Mojave' init --game-install 'D:\SteamLibrary\steamapps\common\Fallout New Vegas'
mods --environment 'D:\Mod Environments\Mojave' config list
mods --environment 'D:\Mod Environments\Mojave' config get game-dir
```

A **Game Binding** records the Game Installation's Steam identity, path, and observed build. Selection precedence is `--game-install`, then `MODS_GAME_DIR`, then Steam library discovery, then Bethesda registry fallback. A selected invalid path fails rather than silently falling back. The fallback emits a warning.

Initialization creates the manifest and Profile State. Use a nonexistent or empty root; existing `logs` and an empty `temp` are permitted when safe. An existing `mods.toml`, other contents, unsafe paths, or unfinished temporary state can block initialization. It is not a repair/reset command. Initialization writes the layout directly into the root and writes `mods.toml` last. A failed initialization can leave a partial layout, which blocks a retry until it is removed.

`MODS_GAME_DIR` is the only supported `MODS_*` setting override. Use an absolute Game Installation path. Unknown, duplicate, empty, or non-Unicode overrides fail validation; even an explicit `--game-install` does not bypass malformed override validation. Use [configuration](commands.md) to inspect effective versus stored values before changing a binding.

## Setup failures

- `mods` not found: use the full path, inspect `Get-Command mods`, and check which version PATH selects.
- Missing DLL/proxy or VFS startup failure: restore the entire matching ZIP layout and check both VC++ redistributables. Inspect security-tool quarantine; do not disable security controls automatically.
- Hash mismatch or extraction failure: stop; check that the asset and checksum belong together and use a fresh destination. Do not overlay a partly extracted installation.
- Missing `LOCALAPPDATA`: specify an absolute `--environment`.
- Game discovery or build mismatch: inspect `config get game-dir`, inherited `MODS_GAME_DIR`, and the real Steam installation before an approved binding change. Do not copy mod files into Steam Data as a workaround.
- Invalid or already initialized root: inspect the selected path and existing state. Do not delete it or rerun initialization as an automatic fix.

For error codes, retained partial state, and logs, read [troubleshooting](troubleshooting.md).
