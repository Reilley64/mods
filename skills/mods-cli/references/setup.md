# Windows setup

## Acquire the released CLI

Use Windows 11 x64 and install **both x86 and x64 Microsoft Visual C++ 2015–2022 Redistributables** from Microsoft. A Steam-managed Fallout: New Vegas Game Installation supplies the clean shared base. Rust, a compiler, and the source archive are not required to run the release.

After user approval, download the Windows x64 runtime ZIP and matching runtime checksum file from [v0.1.0](https://github.com/Reilley64/mods/releases/tag/v0.1.0). Use a fresh download directory. PowerShell 7 example:

```powershell
$ErrorActionPreference = 'Stop'
$base = 'https://github.com/Reilley64/mods/releases/download/v0.1.0'
$zip = 'mods-v0.1.0-runtime-x86_64-pc-windows-msvc.zip'
if ((Test-Path -LiteralPath $zip) -or (Test-Path -LiteralPath 'mods-v0.1.0-runtime-SHA256SUMS')) {
    throw 'Use a fresh download directory'
}
Invoke-WebRequest "$base/$zip" -OutFile $zip
Invoke-WebRequest "$base/mods-v0.1.0-runtime-SHA256SUMS" -OutFile 'mods-v0.1.0-runtime-SHA256SUMS'
$expectedHash = '3a71d12b77cbfd8e2371258075ae0e5148db219865006cb3e0f952b9f31f44f9'
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash -ne $expectedHash) {
    throw 'Runtime checksum mismatch'
}
$destination = Join-Path $env:LOCALAPPDATA 'Programs\mods-v0.1.0'
if (Test-Path -LiteralPath $destination) { throw 'Choose a fresh extraction directory' }
Expand-Archive -LiteralPath $zip -DestinationPath $destination
& "$destination\mods.exe" --version
& "$destination\mods.exe" --help
```

The pinned hash also appears in `mods-v0.1.0-runtime-SHA256SUMS`. Stop on a mismatch. Keep the complete extracted layout, including licenses and `BUILD-AND-SOURCE.md`:

```text
mods.exe
usvfs/usvfs_x86.dll
usvfs/usvfs_x64.dll
usvfs/usvfs_proxy_x86.exe
usvfs/usvfs_proxy_x64.exe
```

Do not move only `mods.exe` or substitute native files from another release. The original all-in-one `mods-v0.1.0-x86_64-pc-windows-msvc.zip` remains supported and has its own `SHA256SUMS`; it includes corresponding source. The smaller runtime ZIP has a separate source download linked in `BUILD-AND-SOURCE.md`.

To make `mods` available in this PowerShell session, with approval:

```powershell
$env:PATH = "$destination;$env:PATH"
Get-Command mods
mods --version
```

Alternatively use the full executable path with PowerShell's `&` operator. Persistent PATH edits require separate authorization. There is no verified public package-manager installation route in this reference; use the published ZIP rather than assume a Winget, Chocolatey, or Scoop package exists.

## Select a Mod Environment

A **Mod Environment** is one isolated setup, not a separately selectable profile. Its **Environment Root** contains `mods.toml` (the **Environment Manifest**) and managed state. **Profile State** is its one set of mod order, plugin state, INIs, and saves beneath `profile`.

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

Initialization creates the manifest and Profile State. Use a nonexistent or empty root; existing `logs` and an empty `temp` are permitted when safe. An existing `mods.toml`, other contents, unsafe paths, or unfinished temporary state can block initialization. It is not a repair/reset command.

`MODS_GAME_DIR` is the only supported `MODS_*` setting override. Use an absolute Game Installation path. Unknown, duplicate, empty, or non-Unicode overrides fail validation; even an explicit `--game-install` does not bypass malformed override validation. Use [configuration](commands.md) to inspect effective versus stored values before changing a binding.

## Setup failures

- `mods` not found: use the full path, inspect `Get-Command mods`, and check which version PATH selects.
- Missing DLL/proxy or VFS startup failure: restore the entire matching ZIP layout and check both VC++ redistributables. Inspect security-tool quarantine; do not disable security controls automatically.
- Hash mismatch or extraction failure: stop; check that the asset and checksum belong together and use a fresh destination. Do not overlay a partly extracted installation.
- Missing `LOCALAPPDATA`: specify an absolute `--environment`.
- Game discovery or build mismatch: inspect `config get game-dir`, inherited `MODS_GAME_DIR`, and the real Steam installation before an approved binding change. Do not copy mod files into Steam Data as a workaround.
- Invalid or already initialized root: inspect the selected path and existing state. Do not delete it or rerun initialization as an automatic fix.

For error codes, retained partial state, and logs, read [troubleshooting](troubleshooting.md).
