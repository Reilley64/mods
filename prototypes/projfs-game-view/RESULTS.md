# Prototype results

This file separates checks run on the development host, the synthetic Windows scenario, and the bounded real-install launch. Do not mark runtime items complete until the named script has produced its evidence on Windows with ProjFS enabled.

## Compile and portable resolver checks

Development host: macOS 15.7.3, Apple Silicon.

- [x] `cargo +stable fmt --check`
- [x] `cargo +stable test`
- [x] `cargo +stable check --target x86_64-pc-windows-msvc`

Record the final command output or CI link here:

```text
Validated 2026-09-14 with rustc 1.98.1 (48a229cea 2026-09-01).
cargo +stable fmt --check                                      exit 0
cargo +stable test                                             exit 0, 3 passed
cargo +stable check --target x86_64-pc-windows-msvc            exit 0
```

The portable tests cover ordered and case-insensitive winner lookup, nested union enumeration, file/directory collision masking, lexical path rejection, and file/directory tombstone filtering.

PowerShell 7 is not installed on the macOS development host, so local validation was structural. Subsequent Windows executions supplied parser/runtime evidence. Real-smoke preflight also exposed the `FileInfo.Directory` versus `DirectoryInfo.Parent` ancestor-walk bug; it was corrected before any provider or game process started.

## Windows runtime evidence

Status: **passed on three Windows targets**.

GitHub Actions [run 34867710415](https://github.com/Reilley64/mods/actions/runs/34867710415) at commit `de024cde262d0eabcad083d42a7ec9f7917af9e8` passed Windows 11 Enterprise ARM64 build 26200 and Windows Server 2025 x64 build 26100. The same full scenario passed on the target host: Windows 11 Home x64 build 26200, NTFS, Client-ProjFS enabled, using `C:\Users\prime\mods-projfs-test\home-scenario`.

All three runs passed every scenario assertion, including ordered/case-insensitive priority, `Data` confinement, file/directory collisions, 482-winner enumeration with enumeration across 90 callbacks, close-time mirroring of projected writes, tombstones, retained restart, cold reconstruction, projected `GameProbe.exe` and `NativeWhere.exe`, and complete source path/SHA-256 equality.

- [x] Windows 11 or Windows Server 2025 build, X64/ARM64 architecture, NTFS volume, and enabled Client-ProjFS are logged and asserted
- [x] `run-scenarios.ps1 -KeepFixture` prints `SCENARIO PASS`
- [x] base root appears at the virtual root
- [x] mod-only and Overwrite-only names do not leak to the virtual root
- [x] a root collision keeps the base-root file
- [x] Overwrite wins over high mod, low mod, and base `Data`
- [x] high mod wins over low mod and base when Overwrite is absent
- [x] low mod wins over base when high mod and Overwrite are absent
- [x] mixed-case first open retains backing-store casing on the placeholder
- [x] nested merged directory enumeration is complete
- [x] 482 long-name merged winners exactly match the expected union
- [x] provider logs prove the large enumeration continued across at least two callbacks
- [x] case-duplicate and file/directory collision entries have the expected winner
- [x] highest-priority file hides a lower-priority directory and descendants
- [x] new and modified virtual `Data` files appear in physical Overwrite after handle close
- [x] projected file deletion removes its Overwrite item and persists an `F` tombstone
- [x] projected directory deletion removes its Overwrite item and persists a `D` tombstone
- [x] a directory tombstone hides descendants
- [x] recreating the same path clears its tombstone and persists the new Overwrite file
- [x] `GameProbe.exe` hydrates, launches from the virtual root, and reads merged files
- [x] projected `NativeWhere.exe` hydrates, launches, and returns observable output
- [x] provider run 1 stops cleanly
- [x] provider run 2 restarts against the retained marked root and local full files
- [x] provider run 2 stops cleanly
- [x] disposable view cache is deleted and recreated
- [x] provider run 3 reconstructs Overwrite changes and deletions from cold state
- [x] provider run 3 stops cleanly
- [x] asynchronous stdout/stderr logs exist for all three runs
- [x] every base/mod file path and SHA-256 hash is unchanged after all runs

`NativeWhere.exe` is extra projected native-PE evidence. It does not close the real Steam/Fallout: New Vegas test gap.


### Synthetic-scenario evidence template

```text
Windows version/build:
Architecture:
NTFS volume/path:
Rust host and optional explicit target:
ProjFS feature state:
Command:
Exit code:
SCENARIO PASS line:
Fixture path:
provider-1 stdout/stderr:
provider-2 stdout/stderr:
provider-3 stdout/stderr:
large-enumeration callback page lines:
tombstones.txt:
base/mod snapshot comparison:
Unexpected behavior:
Real Steam/FNV smoke run separately, if any:
Verdict for issue #20:
```

## Bounded real-install launch evidence

Status: **runtime evidence captured; script status FAILED under its conservative safety gate, not PASS**.

Install/build evidence: Steam AppID `22380`, buildid `1510068`, installdir `Fallout New Vegas`, manifest SHA-256 `6B47AC3C8E796219FCE7EBF3D12BA0D59B81D8ECBAC1A242DBDCEE9FB49E0DD2`.

The authorized scratch contained a disposable installation copy with 544 files totaling 9,973,514,960 bytes. Every relative path, timestamp, length, and SHA-256 matched the canonical installation. The canonical installation did not change during the copy.

- [x] Windows 11 x64 and NTFS ScratchRoot logged and asserted
- [x] provider leaf/name policy and mandatory SHA-256 verified (`86E95DAB9BFEEA7331222DB87DDF55A47D3F9E18E2B6CB81BE672EADA5B32B95`)
- [x] ran as `OFFICEPC\reill`, `InteractiveToken` / `Limited`, non-elevated
- [x] current SessionId was 1 and recorded `explorer.exe` shared it
- [x] full 9.29 GiB disposable installation copy used as `-GameInstall`
- [x] canonical installation captured separately before and after the smoke
- [x] AppID, installdir, buildid, and full manifest SHA-256 recorded out of band
- [x] provider start succeeded as the runtime ProjFS check
- [x] reparse-point and path-overlap preflight checks passed; file inputs walk ancestors starting at `FileInfo.Directory`
- [x] current, Explorer, and Steam PID/session/path evidence recorded; Explorer and Steam shared session 1
- [x] both target-process preflight queries were empty
- [x] selected executable metadata/hashes and complete disposable-copy inventory recorded before launch
- [x] projected root and `Data` listings recorded before launch
- [x] retained root handle confirmed projected `nvse_loader.exe` at the exact view path, PID 13968, session 1
- [ ] script did not attach a retained handle to child `FalloutNV.exe` before its live parent exited
- [x] unproven child was recorded and deliberately not terminated by the script
- [x] Steam, Explorer, and unrelated processes were not terminated
- [x] provider stdout/stderr drained and saved
- [x] provider remained alive through the stop marker, printed exact `provider stopped`, and stopped cleanly
- [x] physical Overwrite and tombstone evidence recorded; both were empty
- [x] disposable-copy after-inventory had zero normalized differences; selected hashes were unchanged
- [ ] script cleanup did not reach its quiet interval while the unproven child remained
- [ ] script final target query was not empty
- [ ] completion marker did not report `PASS`
- [ ] `REAL SMOKE PASS` was not printed

At `2026-09-14T18:11:38Z`, projected `nvse_loader.exe` was confirmed through its retained handle at the exact view path. PID 13968, session 1 exited with code 0. It directly spawned `FalloutNV.exe` PID 12652 after 107 ms. The recorded ParentPID was 13968, and its exact image path was inside the unique projected view. Provider logs show that `FalloutNV.exe` opened projected ESM, BSA, MP3, and shader assets during the 20-second observation window. The same notification logs contain `Data` reads and close notifications from Windows `SearchIndexer` PID 12036 and `SearchProtocolHost` PID 7220 in Session 0, in addition to `FalloutNV.exe` PID 12652. This demonstrates that the ProjFS view is path/global rather than process-scoped: background indexers can hydrate and read it. No source or physical Overwrite changes resulted.

The script's final status was **FAILED** solely because its conservative live-parent retained-handle rule did not attach `FalloutNV.exe` before the fast-exiting loader exited. It left that process running by design. Manual cleanup then freshly queried the one exact PID/name/session/start/path identity, confirmed its recorded ParentPID matched the exact loader root and its image was in the unique view, terminated it through the retained `Process` object, and confirmed final target/provider sets were empty. This is recorded in `manual-exact-cleanup.json`.

The canonical Steam installation's pre-copy and post-smoke inventories are byte-for-byte identical JSON: 544 records including SHA-256. An initial `Compare-Object` result of 1,088 records was a known false difference caused only by comparing `DateTime` values with strings; normalized comparison produced zero differences.

Temporary Scheduled Tasks were removed. The event log was restored to disabled. The temporary ACL grant was revoked. Evidence remains under the approved scratch location.

This is evidence of an observed real loader/game launch and projected asset reads. It is not `REAL SMOKE PASS`, Steam integration proof, or gameplay proof. Delayed-target detection remains bounded polling: the result covers only the observation, cleanup, and final-query window and cannot rule out a later Steam-brokered launch. Empty Overwrite and tombstone outputs do not demonstrate transparent write routing.

### Real-smoke evidence template

```text
Windows version/build and architecture:
NTFS ScratchRoot:
Provider runtime ProjFS start result:
Smoke PID/session and elevated-token result:
Explorer PID/session/path evidence:
Steam PID/session/path evidence:
Expected/actual provider binary SHA-256:
Disposable Game Installation copy:
Canonical installation external before/after inventory:
Game Installation:
Selected Program:
Command and exit code:
REAL SMOKE PASS line:
Requested projected path:
Observed root PID/path/timing/exit:
New matching processes and paths:
Cleanup actions:
Provider stdout/stderr and exact `provider stopped` line:
Out-of-band AppID/installdir/buildid/manifest hash evidence:
Overwrite file list:
Tombstones:
Source executable before/after comparison:
Full Game Installation inventory differences:
Quiet-interval passes and final target query:
Completion marker:
Observed loader/game diagnostic:
Steam/FNV compatibility conclusion, if any:
```
