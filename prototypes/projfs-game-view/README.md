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

## Bounded real-install launch smoke

`run-real-smoke.ps1` is separate from the synthetic scenario. It requires a known SHA-256 for a binary named exactly `projfs-game-view.exe`; Steam and game executable names are rejected for `-Binary`. It launches one allowed root executable through a disposable view of a user-selected real installation or its verified disposable copy. `-Program` accepts only `nvse_loader.exe`, `FalloutNV.exe`, or `FalloutNVLauncher.exe`; its default is `nvse_loader.exe`.

Run this script from a normal interactive PowerShell 7.4 or newer. **Never run it elevated.** Enable ProjFS separately with the elevated setup script, close that shell, then run the smoke with a normal token. The smoke intentionally does not query `Get-WindowsOptionalFeature -Online`, because that query encourages elevation. A successful provider start is its runtime ProjFS check.

`-ScratchRoot` is mandatory, must not exist, must be on NTFS, and must not overlap the Game Installation. The script rejects reparse points in the provider binary, Game Installation, selected program, and scratch-parent ancestor chains before it creates scratch. Ancestor traversal starts at `FileInfo.Directory` for file inputs and `DirectoryInfo.Parent` for directory inputs.

```powershell
$bin = (Resolve-Path .\target\release\projfs-game-view.exe).Path
.\run-real-smoke.ps1 `
  -Binary $bin `
  -ExpectedBinarySha256 "<64-hex-hash-from-the-authorized-build>" `
  -GameInstall "D:\ProjFS-Issue20\game-copy" `
  -Program "nvse_loader.exe" `
  -ScratchRoot "D:\ProjFS-Issue20\smoke-run-001" `
  -ObservationSeconds 20
```

For the authorized issue #20 run, the recommended plan is to make a full disposable copy of the 9.29 GiB installation under an authorized outer scratch directory, then pass that copy as `-GameInstall`. Use a separate sibling path as the script's new `-ScratchRoot`, because the two parameters must not contain one another. For example, use `D:\ProjFS-Issue20\game-copy` and `D:\ProjFS-Issue20\smoke-run-001`. Inventory the canonical installation separately before and after the run. The script's own full inventory then checks the disposable copy for observable changes. This approach tests a real installation image without claiming direct canonical-install integration or gameplay compatibility. Install/build validation was recorded out of band: Steam AppID `22380`, installdir `Fallout New Vegas`, buildid `1510068`, and manifest SHA-256 `6B47AC3C8E796219FCE7EBF3D12BA0D59B81D8ECBAC1A242DBDCEE9FB49E0DD2`. This prototype does not parse VDF manifests.

For unattended use, create a temporary Scheduled Task with `LogonType InteractiveToken` and `RunLevel Limited` in the same interactive session as Steam. Delete the task after it finishes. The script writes `$ScratchRoot\control\real-smoke-complete.json` with `PASS` or `FAILED` and prints `REAL SMOKE COMPLETE: <path>`, so an SSH orchestrator can wait for the file without running the smoke elevated.

The script requires a nonzero interactive session with a same-session `explorer.exe`, and records every Explorer PID/session/path. It records its process/session ID and every Steam PID/session/path. If Steam is running, at least one Steam process must share the smoke process's interactive session. It refuses to launch if `FalloutNV`, `nvse_loader`, or `FalloutNVLauncher` is present in either of two system-wide preflight checks.

The launched root process is accepted only after the script retains its OS process handle, confirms the same session, and reads an exact projected `MainModule.FileName`. Matching descendants are owned only when the script retains their process handles while an exact retained parent is still alive. Before termination it verifies PID, name, start time, image path, and session through that same retained object, then calls `Kill` and `WaitForExit` on it. Steam-brokered or otherwise unjoinable processes are inconclusive: the script records them, does not terminate them, and refuses to print `REAL SMOKE PASS`. Steam is never a termination target.

After initial cleanup, the provider stays alive while the script repeats fresh system-wide capture and proven-owned cleanup until it observes a bounded three-second quiet interval. It performs another fresh target query near the final PASS decision. Provider output drains asynchronously from startup, and the provider must still be alive immediately before the script writes its stop marker. Clean shutdown also requires provider stdout to contain the exact provider-written `provider stopped` line, which is emitted only after `PrjStopVirtualizing` returns.

The provider itself never writes the Game Installation. The launched app or Steam could bypass the projected path, so the script does not claim absolute source immutability. It records a complete before/after inventory of every Game Installation file's relative path, length, and `LastWriteTimeUtc`, saves exact differences, and blocks PASS on any observable change. It also hashes the selected root executables. This check can miss a content rewrite that preserves both file length and write time outside the hashed executable set.

Before launch, the script records visible projected root and `Data` entries. Afterward it records physical Overwrite and tombstone outputs. All evidence remains under `-ScratchRoot`.

`REAL SMOKE PASS` means the exact projected executable was confirmed through its retained process handle, process cleanup reached the quiet interval with no inconclusive or final matching targets, the provider stopped cleanly, selected executable hashes stayed unchanged, and the full inventory had no observable differences. A spawned `FalloutNV` process is useful evidence but is not required. For example, `nvse_loader.exe` may start correctly and exit with a diagnostic failure. This smoke test does not prove Steam integration or gameplay compatibility. Delayed-target detection uses bounded polling, not a permanent system monitor. PASS means no matching target appeared during the observation, cleanup, quiet-interval, and final-query window. It cannot rule out a Steam-brokered launch after monitoring ends.

The three evidence levels remain separate:

- `run-scenarios.ps1` is the synthetic provider and resolver scenario.
- Projected `NativeWhere.exe` in that scenario is extra native Windows PE/tool evidence.
- `run-real-smoke.ps1` is a bounded launch from a real installation. It still is not a gameplay or Steam pass.

## Recorded Windows evidence

GitHub Actions [run 34867710415](https://github.com/Reilley64/mods/actions/runs/34867710415) at commit `de024cde262d0eabcad083d42a7ec9f7917af9e8` passed the complete synthetic scenario on Windows 11 Enterprise ARM64 build 26200 and Windows Server 2025 x64 build 26100. The same scenario passed on the target host: Windows 11 Home x64 build 26200, NTFS with Client-ProjFS enabled, under `C:\Users\prime\mods-projfs-test\home-scenario`. These runs covered all documented scenario assertions, including the 482-winner enumeration, enumeration across 90 callbacks, restart/cold reconstruction, projected probes, close-time mirroring, tombstones, and complete source path/SHA-256 equality.

The bounded real smoke used a 544-file, 9,973,514,960-byte disposable copy. Its relative paths, timestamps, lengths, and SHA-256 values matched the canonical Steam installation. The provider hash was `86E95DAB9BFEEA7331222DB87DDF55A47D3F9E18E2B6CB81BE672EADA5B32B95`. The task ran as `OFFICEPC\reill`, `InteractiveToken` / `Limited`, non-elevated, in session 1 with same-session Explorer and Steam.

At `2026-09-14T18:11:38Z`, retained-handle evidence confirmed projected `nvse_loader.exe` PID 13968 at the exact view path. It exited 0 and directly spawned `FalloutNV.exe` PID 12652 after 107 ms with ParentPID 13968 and an exact path inside the view. Provider logs show projected ESM, BSA, MP3, and shader asset opens during the 20-second window. The disposable-copy inventory had zero normalized differences; selected hashes were unchanged; physical Overwrite and tombstones were empty. The provider remained alive through its stop marker, printed exact `provider stopped`, and stopped cleanly. Steam and Explorer remained running.

The script status was **FAILED**, not PASS. The fast-exiting loader ended before the conservative live-parent retained-handle rule attached the game child. The script therefore left `FalloutNV.exe` running. Manual cleanup re-queried the exact recorded PID/name/session/start/path identity, confirmed its recorded parent and unique-view path, terminated it through the retained `Process` object, and confirmed no target or provider remained. See `manual-exact-cleanup.json`.

The canonical installation's pre-copy and post-smoke inventories are byte-for-byte identical JSON for all 544 SHA-256-bearing records. An initial 1,088-record `Compare-Object` result was a `DateTime`-versus-string type artifact; normalized comparison found zero differences. Temporary tasks were removed, the event log was restored to disabled, the temporary ACL grant was revoked, and evidence remains under approved scratch.

This demonstrates an observed real loader/game launch and projected asset reads. It does not establish `REAL SMOKE PASS`, Steam integration, gameplay, or transparent write routing. Detection is bounded to the observation/cleanup/final-query window.

## Known limitations

- This is close-time mirroring, not transparent write redirection. A crash before the relevant close notification can lose an Overwrite update.
- The provider deliberately leaves a duplicate local full file in the disposable view cache after mirroring. It does not call `PrjDeleteFile`, because safe eviction of dirty/full files was not established in this prototype. Delete the whole cache only after stopping the provider.
- Rename, rename-replace, hard links, alternate streams, extended attributes, security descriptors, backing-store reparse points, sparse files, and concurrent writers to the same path are not covered by the scenario.
- Tombstone persistence uses a temporary file followed by remove-and-rename. It is readable and sufficient for an orderly prototype restart, but it is not crash-atomic on Windows.
- Mutation path checks are lexical and require a plain relative path strictly beneath `Data`. The fixture assumes trusted local directories and does not defend against hostile junctions inside Overwrite.
- Root-level writes and patches are outside the strategy being tested. The provider neither mirrors nor manages them.
- The provider takes a snapshot when directory enumeration begins. Changes made during one enumeration session appear on a later enumeration.
- The ProjFS view is path/global, not process-scoped like a per-process interception layer. In the recorded real smoke, Session 0 `SearchIndexer` PID 12036 and `SearchProtocolHost` PID 7220 produced `Data` read/close notifications alongside FalloutNV PID 12652. Background indexers can therefore hydrate and read the view. No source or physical Overwrite changes resulted in that run. This distinction matters when comparing the prototype with usvfs.
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
