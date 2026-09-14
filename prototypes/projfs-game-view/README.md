# ProjFS game view prototype

This is throwaway code for [issue #20](https://github.com/Reilley64/mods/issues/20). It tests one question: can ProjFS expose an independent game root while applying an ordered union only under `Data`, then reconstruct that view from Overwrite and tombstone state?

Do not turn this directory into production code. The prototype favors a small, inspectable provider and an end-to-end Windows scenario over a reusable VFS design.

## What it does

`serve` projects the physical game installation at the view root. Beneath virtual `Data`, it resolves these sources from highest to lowest priority:

1. the physical Overwrite directory;
2. each `--mod` directory in reverse command-line order;
3. the game installation's physical `Data` directory.

A mod argument points directly at the files contributed beneath `Data`. It does not contain another `Data` directory. Lookup is case-insensitive. Enumeration merges every visible child and uses the highest-priority item for file/directory collisions.

ProjFS has no callback that redirects a write before it happens. A write makes a full local file in the disposable view. This provider uses ProjFS notifications to mirror new and modified files into Overwrite when the writing handle closes. It never writes to the game installation or mod sources.

Deletion under `Data` removes the matching Overwrite item and adds an `F` or `D` line to a plain-text tombstone file. Directory tombstones also hide descendants. Creating or writing the exact tombstoned path removes that tombstone. The state file must sit outside Overwrite.

`probe` prints its current directory and checks expected file contents. The scenario places the built executable in the fake game installation as `GameProbe.exe`, launches its projected copy from the view root, and makes it read merged `Data` files. It also copies the host's `where.exe` as `NativeWhere.exe` and launches that projected PE file. This is extra native-tool hydration evidence. It is not evidence that Fallout: New Vegas or Steam works.

## Prerequisites

Use Windows 11 or Windows Server 2025 on an NTFS volume. The scenario checks the OS build, architecture, volume filesystem, and ProjFS feature state before it starts. Install:

- the Windows Projected File System optional component;
- stable Rust with the MSVC host toolchain;
- Visual Studio Build Tools with the C++ build tools and Windows SDK;
- PowerShell 7.4 or newer. The scenario uses `ProcessStartInfo.ArgumentList` so paths with spaces remain single arguments.

Enable ProjFS from an elevated PowerShell, then restart if Windows requests it:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\enable-projfs.ps1
```

The setup script exits nonzero if Windows requires a restart or if the final feature state is not `Enabled`. After a restart, run it again to confirm the state.

The equivalent elevated setup command is:

```powershell
Enable-WindowsOptionalFeature -Online -FeatureName Client-ProjFS -All -NoRestart
```

## Run the self-contained scenario

From this directory in an elevated Developer PowerShell 7.4 or newer. Elevation lets the script query the optional-feature state.

```powershell
rustup default stable
cargo fmt --check
cargo test
Set-ExecutionPolicy -Scope Process Bypass
.\run-scenarios.ps1 -KeepFixture
```

By default the scenario builds the current Rust Windows host, so native x64 and ARM64 hosts use their installed host toolchain. Use `-Target` only for an explicit target:

```powershell
.\run-scenarios.ps1 -Target x86_64-pc-windows-msvc -KeepFixture
```

The script builds a release binary unless `-Binary` names an existing one:

```powershell
.\run-scenarios.ps1 -Binary .\target\x86_64-pc-windows-msvc\release\projfs-game-view.exe -KeepFixture
```

It fails on the first unmet assertion. A successful run ends with `SCENARIO PASS`. It runs the provider three times. Run 2 reuses the marked view and retained local full files. Run 3 starts after the script deletes and recreates the cache. The checks cover root confinement, independent priority cases, winner casing, long multi-page union enumeration, collisions, close-time Overwrite mirroring, every base/mod file hash, tombstones, and probe launches.

The script drains provider stdout and stderr from process start and writes `provider-1`, `provider-2`, and `provider-3` logs. Enumeration callbacks log their path, ID, page, returned count, continuation index, total, and buffer-full state. The scenario requires at least two logged callbacks for its 482-entry long-name union. `-KeepFixture` retains these logs and the readable tombstone file under the printed fixture path.

Without `-KeepFixture`, the script deletes its fixture after a successful run. It retains the fixture and logs after a failure.

## CLI

```text
projfs-game-view serve --base PATH --view PATH --overwrite PATH --state PATH [--mod PATH ...] --ready-file PATH --stop-file PATH
projfs-game-view probe --expect RELATIVE_PATH=VALUE [--expect ...]
```

`serve` creates `--view` and `--overwrite` if needed, then retains absolute canonical paths for every source, the view, Overwrite, and state. It marks a new view once. A retained ProjFS root is reused; an arbitrary reparse point gets a clear `PrjStartVirtualizing` failure. An RAII guard always calls `PrjStopVirtualizing` before callback context can drop. Create the stop file to stop the provider. It removes stale control files before startup and writes the ready file only after `PrjStartVirtualizing` succeeds.

## Optional real-install smoke test

This is manual and intentionally outside `run-scenarios.ps1`. Choose a disposable view, Overwrite directory, and state path. Do not point `--view` at the real installation.

```powershell
$bin = (Resolve-Path .\target\release\projfs-game-view.exe).Path
& $bin serve `
  --base "D:\SteamLibrary\steamapps\common\Fallout New Vegas" `
  --view "D:\ProjFS-Smoke\Fallout New Vegas" `
  --overwrite "D:\ProjFS-Smoke\Overwrite" `
  --state "D:\ProjFS-Smoke\state\tombstones.txt" `
  --mod "D:\Mods\Example Mod\Data" `
  --ready-file "D:\ProjFS-Smoke\ready" `
  --stop-file "D:\ProjFS-Smoke\stop"
```

While that process runs, use a second PowerShell:

```powershell
Push-Location "D:\ProjFS-Smoke\Fallout New Vegas"
& ".\FalloutNV.exe"
Pop-Location
```

Replace `FalloutNV.exe` with a user-selected game or tool already present at the installation root. The provider only projects root-level files from the shared installation. It does not copy, patch, or manage them. Create `D:\ProjFS-Smoke\stop` when finished.

## Known limitations

- This is close-time mirroring, not transparent write redirection. A crash before the relevant close notification can lose an Overwrite update.
- The provider deliberately leaves a duplicate local full file in the disposable view cache after mirroring. It does not call `PrjDeleteFile`, because safe eviction of dirty/full files was not established in this prototype. Delete the whole cache only after stopping the provider.
- Rename, rename-replace, hard links, alternate streams, extended attributes, security descriptors, backing-store reparse points, sparse files, and concurrent writers to the same path are not covered by the scenario.
- Tombstone persistence uses a temporary file followed by remove-and-rename. It is readable and sufficient for an orderly prototype restart, but it is not crash-atomic on Windows.
- Mutation path checks are lexical and require a plain relative path strictly beneath `Data`. The fixture assumes trusted local directories and does not defend against hostile junctions inside Overwrite.
- Root-level writes and patches are outside the strategy being tested. The provider neither mirrors nor manages them.
- The provider takes a snapshot when directory enumeration begins. Changes made during one enumeration session appear on a later enumeration.
- The implementation does not use the negative path cache. It reads physical directories during lookup and enumeration.

## Primary references

- [Enable Windows Projected File System](https://learn.microsoft.com/windows/win32/projfs/enabling-windows-projected-file-system)
- [Virtualization instance lifecycle](https://learn.microsoft.com/windows/win32/projfs/virtualization-instance-lifecycle)
- [Enumerating files and directories](https://learn.microsoft.com/windows/win32/projfs/enumerating-files-and-directories)
- [Providing file data](https://learn.microsoft.com/windows/win32/projfs/providing-file-data)
- [File system operation notifications](https://learn.microsoft.com/windows/win32/projfs/file-system-operation-notifications)
- [`PRJ_CALLBACKS`](https://learn.microsoft.com/windows/win32/api/projectedfslib/ns-projectedfslib-prj_callbacks)
- [`PrjStartVirtualizing`](https://learn.microsoft.com/windows/win32/api/projectedfslib/nf-projectedfslib-prjstartvirtualizing)
- [`PrjWritePlaceholderInfo`](https://learn.microsoft.com/windows/win32/api/projectedfslib/nf-projectedfslib-prjwriteplaceholderinfo)
- [`PrjWriteFileData`](https://learn.microsoft.com/windows/win32/api/projectedfslib/nf-projectedfslib-prjwritefiledata)
- [`windows` 0.62.2 ProjectedFileSystem bindings](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Storage/ProjectedFileSystem/index.html)
