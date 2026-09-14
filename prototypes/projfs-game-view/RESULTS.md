# Prototype results

This file separates checks run on the development host from Windows 11 evidence. Do not mark runtime items complete until `run-scenarios.ps1` has run on Windows 11 with ProjFS enabled.

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

## Windows runtime evidence

Status: **not run**. The development host is macOS and cannot start a ProjFS virtualization instance.

- [ ] Windows 11 or Windows Server 2025 build, X64/ARM64 architecture, NTFS volume, and enabled Client-ProjFS are logged and asserted
- [ ] `run-scenarios.ps1 -KeepFixture` prints `SCENARIO PASS`
- [ ] base root appears at the virtual root
- [ ] mod-only and Overwrite-only names do not leak to the virtual root
- [ ] a root collision keeps the base-root file
- [ ] Overwrite wins over high mod, low mod, and base `Data`
- [ ] high mod wins over low mod and base when Overwrite is absent
- [ ] low mod wins over base when high mod and Overwrite are absent
- [ ] mixed-case first open retains backing-store casing on the placeholder
- [ ] nested merged directory enumeration is complete
- [ ] 482 long-name merged winners exactly match the expected union
- [ ] provider logs prove the large enumeration continued across at least two callbacks
- [ ] case-duplicate and file/directory collision entries have the expected winner
- [ ] highest-priority file hides a lower-priority directory and descendants
- [ ] new and modified virtual `Data` files appear in physical Overwrite after handle close
- [ ] projected file deletion removes its Overwrite item and persists an `F` tombstone
- [ ] projected directory deletion removes its Overwrite item and persists a `D` tombstone
- [ ] a directory tombstone hides descendants
- [ ] recreating the same path clears its tombstone and persists the new Overwrite file
- [ ] `GameProbe.exe` hydrates, launches from the virtual root, and reads merged files
- [ ] projected `NativeWhere.exe` hydrates, launches, and returns observable output
- [ ] provider run 1 stops cleanly
- [ ] provider run 2 restarts against the retained marked root and local full files
- [ ] provider run 2 stops cleanly
- [ ] disposable view cache is deleted and recreated
- [ ] provider run 3 reconstructs Overwrite changes and deletions from cold state
- [ ] provider run 3 stops cleanly
- [ ] asynchronous stdout/stderr logs exist for all three runs
- [ ] every base/mod file path and SHA-256 hash is unchanged after all runs

`NativeWhere.exe` is extra projected native-PE evidence. It does not close the real Steam/Fallout: New Vegas test gap.

### Evidence template

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
