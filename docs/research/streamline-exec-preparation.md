# Streamlined exec preparation

## Final bounded review checkpoint

The [spec recheck](streamline-exec-spec-recheck.md) confirms the INI safe-open regression is repaired. The [standards recheck](streamline-exec-standards-recheck.md) confirms all three required spacing repairs and finds no new blocker within the repair diff. Full local validation passes with 438 Rust tests and 119 tooling tests; 158 focused tests pass. Reviewers inspected code and saved red/green logs; they did not rerun the full suite.

The parent accepts the remaining findings individually for the file/rule/reasons recorded below and in the [95-pair standards audit](streamline-exec-standards-review.md). Its pre-repair spacing findings are superseded by the linked recheck. The latest gate still reports 94 likely findings; this is not a clean gate and no enforce override has been supplied. The user later approved the archive-list fallback described below; it replaces the temporary refusal. Windows compilation/native and JIP timing remain unverified. No commit, push, PR, installation or game launch was performed for this change.

## User acceptance of documented findings

The user stated that every finding with a documented file, rule, and reason should clear the gate. The parent treats this as the user's acceptance of the recorded dispositions for this change, and proceeds with commit, Windows packaging, and installation. The gate tool itself still reports the findings, because it does not read this report. This is not a clean review.

## Status and scope

This work starts at `5eb0f192d2a7c5346d66280c4a46fe6c6a894b7b`. The approved scope is in [the exec discussion](../exec-performance-discussion.md). No game was launched. No installation or real environment was changed. Test writes stayed inside temporary fixtures.

Exec selects the archive list in this order:

1. The canonical FalloutCustom.ini `sArchiveList`, if present. An explicit empty value counts as present.
2. Otherwise the canonical Fallout.ini `sArchiveList`, if present. An explicit empty value counts as present.
3. Otherwise the embedded `FALLOUT_NEW_VEGAS_DEFAULT_ARCHIVE_LIST` constant in `derived_profile.rs`.

The derivation then appends `Fallout - Invalidation.bsa` once, with the existing transform. The constant copies `SArchiveList` from line 706 of the user's installed Steam `Fallout_default.ini` (SHA-256 `A701C3A96AF26F83BA6399B4A579AF59FA075868949519F4DEC45BF47BF7F95D`). It lists six archives in the source order and keeps the source's double space before `Fallout - Misc.bsa`. The transform trims each entry. Exec does not read `Fallout_default.ini` or enumerate BSAs. Export keeps its existing `Fallout_default.ini` fallback. The `profile_archive_list_missing` refusal and its marker are removed; no other code used the marker.

Localization caveat: only the English Steam copy was checked. Localized editions may use different archive names, such as localized voice archives. On those editions, a profile without an `sArchiveList` gets the English list.

The user later approved ignoring unrelated root, cache, and profile entries and removing generated-BSA validation from launch preparation. The new regression accepts extra directories and a corrupt fixture BSA. This does not establish that a game can use a corrupt BSA. Pending-operation and owned-temp-directory rules remain.

## Implementation

- `execution_preparation.rs` retains strict `prepare_execution` and `PreparedExecution` for export. Native exec calls `prepare_launch`, consumes `PreparedLaunch`, and performs its postrun check through `check_launch_with_spool`. The launch DTO has no asset size, modification time, or freshness state.
- `execution_preparation/inventory.rs` parses the full modlist once before scanning. It builds the enabled-name set before entering the top-level mods loop. That loop records every folder, including empty and disabled folders, and compares exact names against the full modlist. Disabled contents are never traversed or read.
- The inventory updates case-insensitive winners during recursive traversal. Each winner stores a domain provider reference with identity and priority. Base Data is lowest and Overwrite is highest. Priority comparisons, not directory enumeration order, select winners. Namespace types remain separate from winners, so suppression does not hide file/directory collisions.
- Tombstones suppress already-seen lower-priority winners. The retained tombstone index suppresses lower-priority files encountered later. Exact and subtree scopes retain independent maximum ranks. Higher-priority files can reinstate a subtree path. Own-provider tombstone overlaps remain invalid.
- Ordinary enumeration and file types replace ancestry walks and validation-only asset opens. Linked files and directory links are allowed. Directory recursion still consumes the existing entry and depth budgets. Native usvfs traversal is separate and was not changed.
- Enabled-mod metadata requests create `schema_version = 1` only when the file is absent. The safe create-new writer never overwrites an existing or concurrently created file. The implementation rereads and validates the resulting metadata. Disabled mods, base Data, and Overwrite never receive default metadata.
- Exec uses whole-file reads for configuration. It has no configuration byte caps or custom chunk loops. Export retains its bounded reads and strict snapshot checks. Cancellation checkpoints surround synchronous work, but cannot interrupt a synchronous whole-file read.
- CLI composition resolves the existing binding once and supplies it to native exec. It does not validate Steam layout, required executable/default INI, appmanifest, build freshness, or game/environment separation. The adapter no longer compares a second binding or performs prelaunch snapshot/INI revalidation.
- Unlisted plugins retain the input collection order. Explicit listed order remains. No timestamp order or discovery-order tracking was added.
- Windows launch lookup retains caller PATH, cwd, argument quoting, and NUL checks. It no longer retains validation handles or rejects extensions. Lookup skips missing candidates. Other candidate lookup errors defer to Windows process creation instead of selecting a later PATH entry.

An intermediate version mistakenly routed export through the lightweight inventory. Export fault-injection tests exposed changed source ordering. The final design restores the original strict preparation and DTO for export. Export tests were not changed to accept that regression.

## Application composes exec through ports

The user chose option A in the [exec discussion](../exec-performance-discussion.md#proposed-redesign-application-composes-exec-through-ports). The user then asked to keep the existing port shape. This is a behavior-preserving refactor. CLI syntax, output, warnings, progress events, error markers, phases, exit codes, cancellation, INI retention, the post-run warning, export, and settings loading do not change.

The application `execute_program` use case now composes these ports. All ports have the usual `Arc<dyn Fn(...) -> PortFuture<T> + Send + Sync>` shape and are invoked with `.call(...)`:

1. `ResolveLaunchTarget`: caller PATH/cwd lookup and argument encoding. It also duplicates the inherited standard streams when output is not captured.
2. `PrepareLaunchPlan`: modlist, providers, and streaming winner resolution. Conflict resolution stays inside this port.
3. `ProjectExecutionProfile`: platform profile directories, `build_profile_configuration`, and advisory plugin logging.
4. `StageExecutionProfile`: temporary INIs.
5. `CreateVirtualFileSystem`: view configuration and usvfs setup. It keeps the existing cancellation checkpoint between validation and native setup.
6. `LaunchProgram`: private stream setup, hooked launch, and native error mapping.
7. `SuperviseProgram`: supervision, the Job drain query, and release of the process.
8. `PreserveExecutionProfile`: INI preservation.
9. `FinishProgramOutput`: joins private output drains.
10. `CheckProfileState`: the post-run check.

A later change removed the `CloseVirtualFileSystem` port. When cancellation arrives after the file system is created, the use case drops the handle and returns `operation_cancelled` as before. `VirtualGameView` closes its session in `Drop`. The one accepted behavior change: a native teardown failure on this path is no longer reported as `vfs_failed` with phase `cleanup`. As before, a failed teardown keeps the session gate set and the native code loaded.

`RunManagedProgram` is removed. The use case owns the step order, the output-target rule (the Data mod must exist and be enabled), the mapping from `ProfileWarning` to `ExecutionWarning`, progress events, cancellation checks between steps, retention of the staged profile on failure, and forced-cancellation mapping. `ProfileWarning` moved from `infrastructure-execution` into application ports, so infrastructure builds the application type and the use case maps it. Native error-to-marker mapping stays in the adapters.

Handles that the application passes between steps (`LaunchTarget`, `LaunchPlan`, `ExecutionProfile`, `StagedExecutionProfile`, `VirtualFileSystem`, `RunningProgram`, `ProgramOutput`) are opaque. Each wraps `AdapterState`, a `Box<dyn Any + Send>`. Handles are `Send` but not `Sync`. Each port consumes or borrows a handle for one step, so native state is never used concurrently. A handle from another adapter fails with `execution_supervision_failed`.

### Working directory default

A later approved change sets the child's default working directory. When `mods exec` has no `--cwd`, the use case passes the bound game directory to `ResolveLaunchTarget`, which now takes a required `WorkingDirectory`. New Vegas and its script-extender loaders resolve `Data\` from the working directory. Program lookup keeps the caller's startup directory and PATH. An explicit `--cwd` is unchanged. The use case now receives the `GameBinding` from composition. The test `child_defaults_to_the_game_directory_unless_a_directory_is_given` covers both cases. The skill reference `execution.md` describes the new default.

Gate dispositions for this change:

| File | Rule | Disposition |
| --- | --- | --- |
| `src/application/src/execution/execute_program.rs` | Import placement and use | Accepted as a false positive. The new test imports (`temp_dir`, `GameBinding`, `GameInstallationPath`, `WorkingDirectory`) are at test-module scope. |
| `src/application/src/execution/execute_program.rs` | Focused use-case orchestration | Accepted. The default is a five-line, single-use business rule, kept inline in the use case. |
| `src/application/src/execution/execute_program.rs` | Dependency direction and composition roots | Accepted as a false positive. `GameBinding` is a domain type. Composition supplies it, and no infrastructure type enters the application. |
| `src/application/src/execution/execute_program.rs`, `ports/execution.rs`, `native.rs`, `execution_adapter.rs` | Use-case parameters; Narrow custom implementations; Use-case declaration order | Accepted. Dependencies stay first, the supplied binding and requested values are typed arguments, and cancellation stays last. No new facility is added. |

### Execution thread and usvfs `Send`

Composition runs the whole use case on one `spawn_blocking` thread with a current-thread Tokio runtime. `ExecutionAdapter::into_execute_program` builds the ports and calls the use case on that thread. It keeps the tracing dispatcher and span handoff that the old adapter wrapper used. A cancelled request returns `operation_cancelled` before the thread starts. Non-Windows builds still return `program_unsupported` right after that cancellation check. That now happens before the use case runs, so before any `PreparingExecution` event. The CLI passes no progress reporter, so its output does not change. The CLI holds this entry point as `application::execution::ExecuteProgram`, a `Send` boxed `FnOnce`.

`VirtualGameView` is now `Send` but not `Sync` (`unsafe impl Send` in `usvfs/mod.rs`). As a result, `HookedProcess` is also `Send`. Evidence from the pinned `usvfs-rs@c23705c` source:

- `usvfsConnectVFS` and `usvfsDisconnectVFS` store and delete the process-global static `context` and `manager` (`src/usvfs_dll/usvfs.cpp`).
- The controller exports that the shim calls use that global `context` without a lock. These include `usvfsVirtualLinkFile`, `usvfsVirtualLinkDirectoryStatic`, `usvfsCreateProcessHooked`, and the clear functions. `READ_CONTEXT`/`WRITE_CONTEXT` locking appears only in `src/usvfs_dll/hooks/kernel32.cpp` and `hooks/ntdll.cpp`, which run inside injected children.
- No controller state is thread-local. `src/` has no `thread_local`, `DllMain` ignores thread attach and detach (`usvfs.cpp` ~948-965), and the shim (`rust/usvfs-sys/native/barrier.cpp`) keeps no thread or lock state. The hook manager exists only inside injected children.

So the caller's thread does not matter, but upstream calls are unsynchronized and must never overlap. Overlap is prevented by `SESSION_ACTIVE` (one session per process), by the view staying `!Sync`, by every native call taking `&mut self` or `self`, and by the single execution thread. Moving the value between threads transfers ownership, which also orders every earlier call before any later one. An earlier version of this proof claimed that controller calls lock `HookContext`; the standards review found that claim wrong, and the repair corrected it.

The Windows-only test `session_moves_between_threads_for_configure_launch_and_close` checks this at run time. It configures a real view on one thread, launches a hooked `cmd.exe` on a second thread that reads a mapped file, and finishes the process and closes the session on a third thread. It runs three cycles, then closes an unlaunched view on another thread. The test cannot run on macOS. The parent ran `cargo test --package infrastructure-execution` on Windows for i686 and x86_64, and it passed after commit `c5e040f` changed the fixture to a recursive directory link. Residual risk: the source reading and three cycles in one test do not rule out a race under different timing. The single execution thread still prevents overlap in production. The test and the existing session test share a lock, because `SESSION_ACTIVE` is process-global.

### Windows-only code touched

None of this code compiles on macOS. It was checked by reading only.

- `src/infrastructure/dependencies/src/execution_adapter/native.rs`: rewritten. `ExecutionAdapter::dependencies(&self) -> ExecuteProgramDependencies` replaces `ExecutionAdapter::execute`. New private step functions, plus `NativeLaunchTarget` and `NativeProgram`.
- `src/infrastructure/dependencies/src/execution_adapter.rs`: the `#[cfg(windows)]` branch of `into_execute_program` (replaces `run_port`).
- `src/infrastructure/execution/src/usvfs/mod.rs`: `unsafe impl Send for VirtualGameView`. The `_thread: PhantomData<Rc<()>>` field is removed. New cross-thread test and a shared session test lock.
- `src/infrastructure/execution/src/launch_inputs.rs` and `lib.rs`: `ResolvedLaunch` is re-exported under `cfg(windows)`.

### Validation

- The application use-case tests use fake ports. They cover step order and progress events, warning mapping, Overwrite and Data mod output targets, missing and disabled output targets, cancellation before start and after VFS creation (handle dropped, no launch, retained INIs), an undrained Job, an unknown drain state, and a failed undrained supervision (all retained). They also cover a drained supervision failure (preserved first, not retained), failed preservation (retained), forced cancellation (after preserve and finish, with the caller's token cancelled while the post-run check receives an uncancelled token), and cause preservation. 9 tests.
- CLI runner tests use the new entry point.
- Focused run (use case and CLI exec tests): 16 passed, `/tmp/exec-ports-focused.log`. Full `bun run check` passed with 451 Rust tests, 2 release-version tests, and 119 tool tests, `/tmp/exec-ports-check.log`. `git diff --check` passed. Windows compilation and the new Windows test remain unverified here.

### Standards review repair

The standards review of `a449b66` (`/tmp/exec-ports-standards-review.md`) found these issues. One repair change fixes them:

- H1: the `unsafe impl Send` SAFETY proof and this section now state the actual upstream facts: unlocked controller exports, no thread-local state, and exclusive ownership that prevents overlap.
- H2: `CheckProfileState` keeps its unlinked token as a documented exception, with a code comment. The forced-cancellation test now cancels the caller's token inside the supervise fake and asserts it afterwards, so the check fake's uncancelled-token assertion proves the exception.
- H3: `native.rs` no longer keeps inherited stream duplicates until output finishes. Process creation gives the child its own copies, so the duplicates now close when `launch_program` returns. A reason comment replaces the history comment. `ProgramOutput` now carries only the private streams. This lifetime change is not observable.
- The dispositions for Cancellation propagation, Capability modules, the usvfs test, and the carried-over environment rows are corrected. The non-Windows `program_unsupported` wording and the residual-risk statement now match what actually happens.
- J1: the `ExecuteProgram` entry-point alias is defined once, in `application::execution`, and used by composition and the CLI.

Gate findings on the repair diff: `execution/types.rs` Use-case parameters and Use-case declaration order, and `execution/mod.rs` Use-case declaration order. Accepted as inapplicable: `ExecuteProgram` is an entry-point type alias whose cancellation argument comes last. It is not a use-case function, and `execute_program.rs` keeps the Dependencies, Output, Error, function order. The Cancellation propagation finding on `execute_program.rs` is the documented `CheckProfileState` exception in the table below.

### Coding-style gate dispositions

The gate remains non-clean. The CLI crate now enables `fn_traits`, so `runner.rs` calls the exec entry point with `.call_once(...)`.

| File | Rule | Disposition |
| --- | --- | --- |
| `src/application/src/execution/execute_program.rs` | Cancellation state preservation | Accepted. An unlaunched view is dropped on cancellation, which releases the native session the same way the old explicit close did; the user approved not reporting its teardown error. Forced cancellation reports `cleanup` after preservation and the post-run check. The staged profile is retained, not removed. |
| `src/application/src/execution/execute_program.rs` | Cancellation propagation and checkpoints | Accepted, with one documented exception. The parent contract assigns checks between steps to the use case. Each is an inline `is_cancelled()` guard. The caller's token goes to `PrepareLaunchPlan`, `StageExecutionProfile`, `CreateVirtualFileSystem`, and `SuperviseProgram`. The other ports take no token, as before. Exception: `CheckProfileState` gets an unlinked `CancellationToken::new()` so that the post-run check still runs after the caller cancels. That is frozen behavior, moved from the old adapter, and a code comment records it. |
| `src/application/src/execution/execute_program.rs` | Test public behavior | Accepted. Tests call the public use case with fake ports. They assert project-owned order, retention, and warning policy, not dependency internals. |
| `src/application/src/execution/execute_program.rs` | Phase spacing; Narrow custom implementations; Use-case declaration order | Phase spacing fixed between resolve, prepare, projection, and warning mapping. The rest is accepted: the file declares Dependencies, Output, Error, then the instrumented function, and it adds no general-purpose facility. |
| `src/application/src/ports/execution.rs` | Capability modules and public APIs | Accepted. The ports module is the existing capability interface. The handle fields and `AdapterState::downcast`/`downcast_ref` are `pub`, so adapters can build and read handles. The state is opaque only because its type is erased. Production application code never downcasts it; only the use-case test fakes do. |
| `src/application/src/ports/execution.rs` | Narrow custom implementations | Accepted. `AdapterState` only wraps `Box<dyn Any + Send>` so application signatures carry no infrastructure types. It is not a runtime primitive. |
| `src/application/src/ports/execution.rs`, `ports/mod.rs` | Use-case parameters | Accepted as inapplicable. These are port type declarations. Every port that takes cancellation takes it last. |
| `src/infrastructure/dependencies/src/execution_adapter/native.rs` | Use-case parameters; Use-case declaration order | Accepted as inapplicable. These are infrastructure adapter functions. Cancellation is last where present. |
| `src/infrastructure/dependencies/src/execution_adapter/native.rs` | Cancellation propagation and checkpoints | Accepted. The checkpoint between view validation and native setup is the existing infrastructure checkpoint, moved unchanged. |
| `src/infrastructure/dependencies/src/execution_adapter/native.rs` | Phase spacing; Narrow custom implementations | Phase spacing fixed around projection input, view configuration, and the drain query. The rest is accepted: the code moved from the old adapter and adds no new facility. |
| `src/infrastructure/dependencies/src/execution_adapter.rs`, `execute_program.rs` | Use-case parameters; Use-case declaration order; Phase spacing | Accepted as inapplicable. These are composition methods, not application use cases. The thread entry keeps the old wrapper's order. |
| `src/infrastructure/execution/src/usvfs/mod.rs` | Rustdoc format | Accepted as a false positive. The `// SAFETY:` comment before `unsafe impl Send` is the proof format that CODING_STYLE requires, not item documentation. |
| `src/infrastructure/execution/src/usvfs/mod.rs` | Test public behavior; Phase spacing | Accepted. The Windows test exercises the native session's thread contract, which is a project boundary. Like the existing session test, it uses the private `VirtualGameView::load`, `ConfigureView::link_directory`, and the `SESSION_ACTIVE` static, because the public `configure` path needs a packaged executable layout. Setup, each thread step, and the assertions are separate blocks. |
| `src/presentation/cli/src/main.rs` | Callable port invocation | Accepted as a false positive. `resources.execute_program(...)` is an inherent composition factory, not an application port call. |
| `src/presentation/cli/src/runner.rs`, `main.rs` | Use-case parameters; Use-case declaration order; Narrow custom implementations | Accepted as inapplicable. The runner holds a presentation-owned entry-point type and passes cancellation last. |
| `src/infrastructure/environment/src/execution_preparation.rs`, `export.rs` | Phase spacing; Use-case parameters | Unchanged here. The INI-fix table (commit `1e37225`) records `execution_preparation.rs` Phase spacing and both files' Use-case parameters. `export.rs` Phase spacing is recorded as unchanged baseline in the parent gate reconciliation table above. |
| `src/application/src/execution/execute_program.rs` | Cancellation propagation and checkpoints; Test public behavior; Use-case parameters | Close-port removal: accepted. The checkpoint after file-system creation is still an inline guard owned by the use case, as the parent contract requires. The test fixture's drop recorder shows that the use case drops the handle; it does not inspect adapter internals. Dependencies stay first and cancellation stays last. |
| `src/application/src/ports/execution.rs`, `native.rs` | Use-case parameters | Close-port removal: accepted as inapplicable. Only a port type and its adapter were removed. |

## MO2 modlist order

The user approved reading `profile/modlist.txt` in Mod Organizer 2 order. The first listed mod has the highest Mod Priority and the last has priority 0. Overwrite stays implicitly highest and game Data lowest. There is no migration: an existing list written in the old low-to-high order now reads with the order reversed. The skill references and `CONTEXT.md` say so, and tell users to reverse the mod entries by hand.

- `snapshot::parse_modlist` (strict preparation, installation state, export) and `conflict_scan::parse_modlist` (conflict list, inspect, explain, and installation previews) collect entries in file order. They then return them lowest priority first, with priority = rank from the end of the file. Exec's inventory uses the strict parser. Every caller that iterates installed mods therefore keeps its low-to-high order.
- `insert_disabled_mod` (renamed from `append_disabled_mod`) puts a new disabled mod before the first non-comment line, after any leading `#` lines. It keeps the BOM and reuses the file's last separator. New installs still get priority = number of installed mods, which is now the highest.
- `ProjectedModState.list_position` now counts mod entries from the top of `modlist.txt`; comment lines are not counted. A new install is at 0, and a replacement keeps its current entry position. Transaction intent validation computes the replacement position with checked arithmetic and rejects an inconsistent plan as `transaction_failure`. The installation use case and transaction intent validation both use this meaning. The install preview prints this field.
- `CONTEXT.md` (Mod Priority) and the skill references `execution.md` and `installation.md` describe the new order.
- New tests:
  - `first_listed_mod_wins_in_an_mo2_ordered_modlist`: a comment header, `+High` first, `+Base` last, and a conflicting `shared.txt`. `High` wins in both launch and strict preparation.
  - `modlist_lists_the_highest_priority_first`: conflict scan priorities.
  - `new_mods_are_inserted_at_the_top_after_leading_comments`: BOM, LF and CRLF, comment-only, and empty lists.
  - `new_install_takes_the_top_of_an_mo2_ordered_modlist`: publication.
  - `list_position_counts_from_the_top_of_the_mo2_modlist`: new and replacement previews.
- Test fixtures with more than one mod now list the higher-priority mods first. They keep the same resulting priorities.
- Gate findings on this diff: Use-case parameters (≤0.10) in `conflict_scan.rs`, `transactions.rs`, and `execution_preparation.rs`. Accepted as inapplicable: the changes are infrastructure parsers, a writer, intent validation, and test functions, and no signature changed apart from the `insert_disabled_mod` rename. A transient Phase spacing finding on `snapshot.rs` cleared after the parser was split into collection and priority-assignment blocks.

## SafeDir removal and tokio::fs

The user decided to drop `SafeDir` (cap-std) and use `tokio::fs` throughout. The decision is in [the exec discussion](../exec-performance-discussion.md#decision-drop-safedir-use-tokiofs-everywhere). The planned git rollback ticket covers recovery from bad writes. The change lands in two commits: first settings and game platform, then environment and its callers.

### Part 1: settings and game platform

- `infrastructure-settings` no longer uses cap-std or `libc`. `fs_access.rs` is deleted. Every settings read, layout check, and manifest write is async `tokio::fs`. `load_command`, readiness, and store are async, so the CLI dependency factory is now an async closure (`AsyncFnOnce`).
- Settings manifest writes go directly to `mods.toml` after validation. The staged `temp/operation` copy, flushes, rename, and the compare-before-publish source check are gone, as is `SettingsAdapter::verify_source` (`settings_source_changed`). Pending work in `temp` still blocks a write. Export no longer compares settings source bytes or re-validates the binding before publication.
- The layout check keeps entry names, entry types, required files, and content validation. It no longer walks trees to reject links, reparse points, or special files. Links are followed like ordinary entries.
- `infrastructure-game-platform` no longer uses cap-std or `libc`. `fs_access/` and `separation.rs` are deleted. Steam validation checks directories and files by path, canonicalizes the game directory, and returns the canonical path instead of an open handle. It keeps the Steam structure, executable and default-INI checks, the reserved-archive check, and the appmanifest check. The same-directory identity check and the game/environment containment proof (an ancestry check) are removed. The validation ports no longer take the environment root.
- Version reads and profile-source reads use `tokio::fs::read`. `read_file_version` now parses bytes.
- A Steam root path that is a regular file is now treated like a missing root on Windows. Reading `<root>/steamapps/libraryfolders.vdf` through a file fails with `ERROR_PATH_NOT_FOUND`, which maps to `NotFound`, so the root has no libraries. On macOS the same read fails with `ENOTDIR` and the root is invalid. The old handle-based open rejected a non-directory root on both platforms. This behavior is accepted. `invalid_root_does_not_mask_later_library_discovery_cancellation` now makes the invalid root with a malformed `libraryfolders.vdf`, which fails on every platform. A check of the other tests converted in this branch found no other case where a file stands in for a directory and an error is expected.
- Tests removed because they asserted dropped protections:
  - settings: `failed_manifest_stage_blocks_a_later_store_and_preserves_all_artifacts`, `mutation_snapshot_refuses_changed_source_without_overwriting_it`, `nested_provider_symlink_is_rejected_as_unsafe`, `unsafe_disposable_entries_never_change_settings_behavior`, `manifest_symlink_is_rejected_without_reading_its_target`, `symlinked_root_ancestor_is_rejected`, and `root_directory_symlink_is_rejected`
  - manifest writer: `failed_staged_validation_preserves_operation_state`, `edit_after_staging_is_not_overwritten`, `final_cancellation_preserves_the_validated_stage`, and `cleanup_failure_after_commit_returns_success_and_leaves_refusal_state`
  - Steam validation: `rejects_symlinked_game_files_directories_and_manifest` and `rejects_symlinked_game_ancestor_and_directory`
  - profile sources: `symlinked_profile_source_is_rejected`
  - containment: the five separation tests
- The remaining tests run on a Tokio test runtime. The manifest-writer replacement test now asserts only the direct write.

Part 1 gate dispositions:

| File | Rule | Disposition |
| --- | --- | --- |
| `settings/src/manifest_writer.rs`, `settings/src/layout.rs`, `game_platform/src/steam/libraries.rs` | Language-neutral review priorities | Accepted. Removing staging, flushes, source comparison, and link/reparse rejection is the user's explicit decision. Git rollback covers recovery. It is not a terseness trade. |
| `settings/src/*`, `game_platform/src/*`, `dependencies/src/export_environment.rs`, `cli/src/runner.rs`, `cli/src/main.rs` | Use-case parameters; Use-case declaration order | Accepted as inapplicable. These are infrastructure adapters, ports, and composition. Cancellation stays last where present. |
| `dependencies/src/set_game_directory.rs` | Callable port invocation | Accepted as a false positive. It calls inherent adapter factory methods, not an application port. |
| `settings/src/lib.rs`, `settings/src/layout.rs`, `game_platform/src/steam/validation.rs`, `game_platform/src/profile_sources.rs` | Narrow custom implementations | Accepted. The checks are direct `tokio::fs` calls grouped per validation step. No general-purpose facility is added. Phase spacing in these files is fixed; see the `exec --cwd` help section. |

### Part 2: environment and callers

Scope: `infrastructure-environment` (all modules), `infrastructure-dependencies` (`execution_adapter/native.rs`, Windows only), `mods` CLI (`error.rs` export advice), workspace `Cargo.toml` and `Cargo.lock`, and the `mods-cli` skill export and troubleshooting references.

- `safe_fs.rs` (`SafeDir`, `SafeFile`, `EntryBudget`, `read_bounded`, `sync_tree`) and `export_publication.rs` (`MoveFileExW` no-replace publication) are deleted. `cap-std`, `cap-fs-ext`, and `same-file` leave the environment crate. `cap-std` and `libc` leave the workspace dependency table. Tokio gets the `fs`, `io-util`, and `rt` features. The environment crate no longer needs the Windows `Win32_Storage_FileSystem` feature.
- The new `files.rs` has two helpers: `read_optional` (a missing file is `None`) and `validate_exact_entries` (the directory holds only allowed names).
- Every environment read, walk, and write uses `tokio::fs`. The adapter methods and their ports are async: initialization assessment and publication, installation state, assessment and transactions, conflict scan and content reads, export, `prepare_execution`, `prepare_launch`, `derive_execution_inis`, `ExecutionInis::preserve`, `check_launch`, and `check_launch_with_spool`.
- Walks follow links like ordinary entries. The no-follow opens, reparse-point and hard-link rejection, containment (ancestry) checks, byte caps, entry budgets, and depth limits are gone. A link cycle now stops only when the operating system reports a path or link-depth error. The conflict scan no longer reports `ReparsePoint`, `HardLink`, or `ContainmentEscape` problems.
- Directory and file fsyncs are gone everywhere. Initialization and installation kept their stage-and-rename structure in this part; the next section removes it.
- `meta.toml` default creation is the only `create_new` open (`OpenOptions::create_new`, then write and flush). Installation files are staged with `File::create`; the transaction already refuses a path that repeats.
- Execution INIs: the compare-before-publish check (`profile_changed`) and the `preserved/` stage are gone. Preservation writes each changed INI directly to the canonical profile. A canonical INI edited while the game runs is overwritten.
- Export: staging, source fingerprints, the second capture, the per-file hash check, and the Windows no-replace rename are gone. Export creates the new output folder and writes files into it directly, then sets each source modification time. Export now also runs on non-Windows hosts. `validate_destination` keeps two rules: the output must not exist, and its canonical parent must not be inside the canonical Environment Root. The Game Installation rule is gone. On failure the output folder is reported as `retained_partial_output`.
- `native.rs` (Windows only, checked by reading): `prepare_launch_plan`, `stage_execution_profile`, `check_profile_state`, and `preserve_execution_profile` are async. The stage port borrows `&LaunchPlan`, so it clones the `PreparedLaunch` before the future starts. The other ports are unchanged.
- Tests removed because they asserted dropped protections:
  - `safe_fs.rs`: all tests (six cross-platform and four Windows-only)
  - `export_publication.rs`: `publication_never_replaces_a_racing_destination` (Windows only)
  - `derived_profile.rs`: `later_publication_failure_keeps_earlier_edit_and_retains_remaining_inis`, and the `concurrent` case of `uncertain_drain_deletion_and_concurrent_edits_retain_temporary_files`
  - `execution_preparation.rs`: `preservation_rejects_hard_linked_inis_and_retains_child_edits` and `launch_rejects_a_symlinked_required_fallout_ini`
  - `export.rs`: `mid_copy_write_failure_keeps_partial_bytes_and_typed_retained_path`, `timestamp_failure_keeps_copied_bytes_without_final_publication`, `changed_sources_and_existing_or_overlapping_destinations_fail`, `completed_stage_preserves_bytes_times_and_reports_unsupported_publication`, `cancellation_retains_owned_stage_without_creating_final_output`, and `selected_save_links_are_rejected`
  - `manifest.rs`: `manifest_cap_is_deliberate_and_preserves_limit_cause`
  - `profile.rs`: `profile_resource_caps_are_deliberate`, `save_validation_rejects_total_entry_cap`, and `save_validation_rejects_exhausted_depth_with_io_cause`
  - `snapshot.rs`: `snapshot_resource_caps_are_deliberate`, `canonical_tree_collection_rejects_total_cap_with_io_cause`, and `provider_collection_and_validation_reject_exhausted_depth_with_io_causes`
  - `conflict_scan.rs`: the Unix symlink-replacement block of `indexed_content_reads_hash_once_opened_and_report_namespace_replacements_as_failures`
- Tests added: `existing_outputs_and_outputs_inside_the_environment_are_rejected` and `export_writes_bytes_and_times_directly_to_the_output`. The ignored release measurement `measure_launch_inventory_walk_on_twenty_thousand_files` stays and runs on a multi-thread Tokio runtime, as the CLI does.

Code kept on synchronous `std::fs` (sync code outside an async context, or sync by design):

- `infrastructure-archive` readers (`source.rs`): ZIP, 7z, and RAR readers need `Read + Seek` files.
- `infrastructure-execution/build.rs`: a build script.
- `child_output.rs`: the capture spool directory, spool files, drain threads, and `read`. The drain threads are OS threads, and the Windows launch port that prepares them is sync.
- `launch_inputs/windows_inputs.rs`: one `fs::metadata` probe in sync program resolution.
- `usvfs/mod.rs`: the native artifact read in sync VFS setup.
- `ExecutionInis::drop`: `TempDir::keep` in a `Drop` impl.
- `ExecutionInis::create`: `tempfile::Builder::tempdir_in` makes one uniquely named directory synchronously.
- `export.rs` `set_modified`: `std::fs::File::set_modified` runs on `spawn_blocking`, because `tokio::fs::File` has no way to set times.

Walk timing (ignored release test above; 4 mods × 50 directories × 100 files, 5,001 winners, 5 `prepare_launch` runs per test; host load average 13–18 during the runs). Before and after builds ran in turn, three times each:

| Build | Per-run medians (ms) | Median (ms) |
| --- | --- | --- |
| Before (e55617a, sync `std::fs` walk) | 20.9, 23.6, 21.0 | 21.0 |
| After (`tokio::fs` walk) | 28.5, 27.6, 26.3 | 27.6 |

The first baseline (median 19.9 ms, `/tmp/tokio-fs-walk-before.log`) agrees. `tokio::fs` sends each directory read and metadata call through the blocking pool, so the walk is about 30% (6–7 ms) slower for 20,000 files. Logs: `/tmp/tokio-fs-walk-after*.log` and `/tmp/tokio-fs-walk-interleaved.log`.

Part 2 gate dispositions:

| File | Rule | Disposition |
| --- | --- | --- |
| `environment/src/publication.rs`, `derived_profile.rs`, `export.rs`, `manifest.rs`, `conflict_scan.rs` | Language-neutral review priorities | Accepted. Removing flushes, staged INI and export publication, source comparison, link/reparse rejection, and caps is the user's explicit decision. Git rollback covers recovery. It is not a terseness trade. |
| All changed environment modules, `dependencies/src/execution_adapter/native.rs` | Use-case parameters; Use-case declaration order | Accepted as inapplicable. These are adapter internals and port bodies, not application use cases. `CancellationToken` stays the last parameter where present. |
| `environment/src/files.rs`, `lib.rs`, `profile.rs`, `transactions.rs`, `conflict_scan.rs`, `export.rs`, `execution_preparation.rs`, `native.rs` | Narrow custom implementations | Accepted. `files.rs` has two small helpers over `tokio::fs::read` and `read_dir` that replace the larger `safe_fs.rs`. The rest are direct `tokio::fs` calls. |
| `environment/src/lib.rs`, `execution_preparation.rs`, `transactions.rs` | Choose the narrow conditional form | Accepted. The flagged `match` statements have three arms (found, `NotFound`, other error) with different results; one `if let` cannot express them. |
| `environment/src/conflict_scan.rs`, `snapshot.rs`, `transactions.rs` | Guard clauses | Accepted. The flagged code keeps the original structure: the `temp` check nests the two access modes, and loops `continue` past unrelated entries before one check that returns. |
| `game_platform/src/profile_sources.rs` | Match only for multi-way logic (Part 1, 0.55) | Fixed. The two-arm `Option` match is now an `if let`. |

The new "Prefer Option and Result combinators" rule was applied to the code written after it arrived: `derived_profile.rs` (deleted child INI) and `transactions.rs` (split of the staged path) use `ok_or_else(...)?`. Three-way `NotFound` matches and `let`-`else` branches that `continue` stay conditionals.

### No staging for initialization and installation

The user decided that initialization and installation also write directly.

- Initialization creates `mods`, `profile`, `overwrite`, `cache`, and `temp` in the Environment Root, writes the Profile State and the support BSA, and writes `mods.toml` last. A partial layout without `mods.toml` makes a retry fail with `environment_root_not_empty`, so it is never mistaken for an initialized environment. The layout check runs on the root. `publication.rs`, `publish_initialization`, `LayoutLocation`, and `validate_stage` are removed; the check is now `validate_layout`. `stage_profile` is now `write_initial_profile`.
- Installation loads the environment with mutation access, validates the intent, and, for an enabled replacement, records the visible plugins. Then it deletes the old `mods/<name>` for a replacement and creates `mods/<name>`. Extracted files go straight into that folder. `finish` writes `meta.toml`, validates the provider and the prospective namespace, and then writes `modlist.txt` (new install) or updates `plugins.txt` and `loadorder.txt` (enabled replacement, `update_plugin_lists`, formerly `stage_plugin_maintenance`). A last snapshot load checks the planned priority and enabled state. The backup folder, the renames, `publish_installation`, and the read-back of published profile files are removed. `visible_plugins` no longer takes a staged replacement folder.
- A failure or cancellation after the mod folder is created leaves partial state: a partial or unlisted mod folder, or partly updated plugin lists. There is no rollback; git rollback covers recovery. The namespace check still runs, but after the files are written.
- Pending-operation refusal: nothing creates `temp/operation` any more, so the operation-specific checks are removed. These are the `SnapshotLoad::Publication` mode, `load_during_publication`, the `OPERATION_DIRECTORY` constant, the exclusive `temp/operation` reservation in initialization and installation, and the `environment_publication_failed` error code (`ErrorCode::EnvironmentPublicationFailed` and `ErrorMarker::environment_publication_failed`). The general refusal of a non-empty `temp` (`manual_cleanup_required`) still has a purpose: failed execution keeps its derived INIs in `temp/execution-inis-*` for the user to inspect, and a live capture spool sits there during exec. The refusal is kept for initialization, installation, settings mutation, and exec. The reservation was also the only guard against two concurrent installs; none remains.
- Tests removed: `lib.rs` `staged_initialization_has_no_canonical_mutation`, `publication_failure_before_cache_keeps_manifest_staged_and_refuses_initialization`, and `cancellation_before_publication_preserves_the_complete_stage`; `transactions.rs` `pending_operation_refuses_a_later_mutation`, `staging_does_not_mutate_canonical_state`, `unexpected_backup_entry_is_a_publication_failure_with_its_original_cause`, `backup_validation_keeps_cancellation_as_the_top_marker`, `a_mid_publication_failure_keeps_stage_and_backups_then_refuses_later_mutation`, `cancellation_preserves_staging_without_canonical_mutation`, `postcommit_cancellation_is_not_observed`, and `cleanup_failure_after_validation_still_returns_success`. Two tests lost only their `temp/operation` assertion and were renamed: `prior_temp_debris_is_rejected` and `failed_intent_validation_writes_nothing` (it now checks that no mod folder exists).
- The `mods-cli` skill references (`setup.md`, `installation.md`, `troubleshooting.md`) now describe direct writes and what `manual_cleanup_required` means.

Gate dispositions for this change:

| File | Rule | Disposition |
| --- | --- | --- |
| `environment/src/transactions.rs`, `lib.rs`, `snapshot.rs`, `profile.rs` | Use-case parameters; Use-case declaration order | Accepted as inapplicable. These are adapter internals; `CancellationToken` stays last. |
| `environment/src/transactions.rs`, `snapshot.rs` | Language-neutral review priorities; Readability before secondary cleanup | Accepted. Removing staging is the user's explicit decision, not a terseness trade. The install flow is now one straight sequence of writes. |
| `environment/src/transactions.rs`, `lib.rs` | Test public behavior | Accepted. The remaining tests drive `InstallationTransaction` and the initialization port and assert files on disk. |

The combinator rule is applied in `finish_committed_installation`, which checks the installed mod with `find`, `filter`, and `ok_or_else(...)?`.

### `exec --cwd` help and Phase spacing pass

- `mods exec --help` now describes `--cwd`: "Working directory for the program; defaults to the bound game installation directory. Program lookup still uses the caller's directory and PATH". The text comes from the field's doc comment in `commands.rs`. There were no help snapshot tests; the new test `exec_help_describes_the_working_directory_default` checks both parts of the text.
- Phase spacing: blank lines were added at phase changes in each file that the branch gate flagged for this rule. The files are `environment` (`conflict_scan.rs`, `derived_profile.rs`, `execution_preparation.rs`, `execution_preparation/inventory.rs`, `export.rs`, `lib.rs`, `manifest.rs`, `profile.rs`, `profile_activation.rs`, `snapshot.rs`, `transactions.rs`), `settings` (`lib.rs`, `layout.rs`, `manifest_writer.rs`), `game_platform` (`steam/validation.rs`, `profile_sources.rs`), `dependencies` (`export_environment.rs`, `execution_adapter.rs`, `execution_adapter/native.rs`), and `application/src/execution/execute_program.rs`. Only blank lines changed. The per-edit gate reviews after these edits report no Phase spacing finding. The full-branch review still reports Phase spacing at 0.21 to 0.44 in most of the same files, so the finding is not cleared there. `profile_activation.rs` no longer appears. The Part 2 and no-staging rows are replaced by this row:

| File | Rule | Disposition |
| --- | --- | --- |
| `settings/src/lib.rs`, `layout.rs`, `manifest_writer.rs`; `environment/src/lib.rs`, `transactions.rs`, `execution_preparation.rs`, `execution_preparation/inventory.rs`, `conflict_scan.rs`, `snapshot.rs`, `derived_profile.rs`, `profile.rs`, `export.rs`, `manifest.rs`; `game_platform/src/steam/validation.rs`, `profile_sources.rs`; `dependencies/src/export_environment.rs`, `execution_adapter.rs`, `execution_adapter/native.rs`; `application/src/execution/execute_program.rs` | Phase spacing (full-branch review, 0.21 to 0.44) | Accepted after one fix pass. Blank lines now separate guards, acquisition, transformation, writes, and output in the changed functions, and the per-edit reviews of these files are clean. The full-branch review gives no line numbers. The remaining finding cannot be traced to a specific block; adding more blank lines would split statements that form one operation. |

## INI text lines without an assignment

Exec failed with `environment_invalid` (phase `profile_ini`) on a real profile. The vanilla `Fallout.ini` and `FalloutPrefs.ini` continue the `SMasterMismatchWarning` value on two lines without `=`. The game's INI reader ignores such lines, so `domain::profile_ini_valid` now accepts them. It still rejects control characters, empty or unterminated section headers, and assignments with an empty key. No game behavior requires accepting those forms.

The derive and preserve editors already kept lines without `=` unchanged, so derived and preserved copies keep the block byte-for-byte. Effect on each `profile_ini_valid` caller:

- Execution INI creation (`ExecutionInis::create` through `ProfileIniInputs::read_mode`): accepts the block. Launch preparation itself does not call the validator.
- Postrun preservation (`preserve_inner`): accepts child INIs that keep the block. A child `[malformed` header is still rejected.
- Export (`ProfileIniInputs::read` and its `Fallout_default.ini` fallback): accepts the block in canonical INIs and in the game default.
- Init staging and settings do not call the validator. Init's `canonical_ini` derivation already kept such lines.

New tests use the exact three-line block in `Fallout.ini` and `FalloutPrefs.ini`: the domain validator and editors, launch preparation with INI creation and preservation plus the postrun check, and export capture of derived INIs. All three failed before the fix with the reported marker (`/tmp/ini-continuation-red.log`). No existing rejection test encoded the wrong behavior, so none changed.

Validation: focused domain and infrastructure-environment nextest passed 139 tests (`/tmp/ini-continuation-focused.log`). Full `bun run check` passed with 444 Rust tests, 2 release-version tests, and 119 tool tests (`/tmp/ini-continuation-check.log`). `git diff --check` passed.

The style gate reported these findings for this fix:

| File | Rule | Disposition |
| --- | --- | --- |
| `environment/src/execution_preparation.rs` | Phase spacing | Fixed between the derived-copy check and the child edit, and before the postrun check. Accepted for the remaining fixture setup: the block text, both INI texts, and their writes form one setup phase. |
| `environment/src/execution_preparation.rs` | Use-case parameters | Accepted as inapplicable. The change adds only a test function; no infrastructure or use-case signature changed. |
| `environment/src/export.rs` | Use-case parameters | Accepted as inapplicable. The change adds only a test function; no infrastructure or use-case signature changed. |

## Removed binding IDs

The later approved change removes `steam_app_id` and `observed_build_id` from both manifest schemas and their writers. `GameBinding` now contains only the game directory. No migration or automatic rewrite was added. Both readers retain `deny_unknown_fields`, so old manifests with either key fail as invalid. Tests cover each removed key separately.

Steam discovery and strict installation validation still check the fixed Fallout New Vegas app identity. Steam `buildid` is no longer required or compared. Build mismatches no longer block strict consumers; their unrelated installation, filesystem, and source-change checks remain.

The CLI no longer accepts `steam-app-id` or `observed-build-id` as config keys. Settings output contains `schema-version`, `name`, and `game-dir`. The set-game-directory result no longer carries a recorded build or build-mismatch warning. The `game_build_mismatch` error code and expected/actual build fields are removed. Invalid or missing effective overrides still produce the existing `EffectiveBinding::Invalid` outcome. Stored/effective path shadowing and durable settings publication remain.

## Command-scoped settings

CLI composition starts settings loading after parsing, root selection, and diagnostic setup. `Resources::system` does not read the manifest. Composition calls `load_settings` once, then supplies records to get/list and binds the effective game directory into command-local infrastructure ports. There is no `LoadSettings` application port, global singleton, or lazy downstream resolution.

Environment conflict scans, snapshots, plugin maintenance, and installation transactions receive the supplied binding explicitly. The native exec adapter stores that binding instead of a settings reader. Exec preparation no longer reads `mods.toml`. Strict export and initialization still parse manifests for source/staged integrity validation, not to choose a different binding.

The settings snapshot retains stored/effective provenance, captured overrides, and original source bytes. Mutation preview computes proposed typed values without reading configuration again. Publication compares the original bytes before staging and immediately before rename. This is not an atomic filesystem compare-and-swap; it retains safe file opens and durable publication. Export compares captured manifest bytes and retains strict source capture/revalidation rather than resolving settings again.

Read modes retain separate pending-work and profile-validation policies. Config get/list and updates keep their layout checks. Exec and export defer profile work to their own owners. Conflict scans and install previews do not gain a full settings-layout scan. Init uses only its override reader and requires no manifest. Help/version stop before resource construction. Errors at the moved load boundary retain allowlisted output and command-specific cancellation/exec exit codes.

## Temporary INIs and compatibility

The shared domain INI transform writes managed archive and save settings into a temporary FalloutCustom.ini. It uses an existing Custom.ini as the basis or creates a temporary file when absent. It retains unrelated settings and maps the temporary file through the existing profile mapping plan. Custom.ini's archive list takes precedence over Fallout.ini, including an explicitly empty value. If neither file has a list, exec uses the embedded verified default list. The transform retains the selected list and appends the invalidation archive once. It does not read an external game-default INI on the launch path.

JIP LN NVSE is a required runtime prerequisite, not a new plugin-detection gate. GECK support for the new override mechanism is deferred. [JIP's loader source](https://github.com/jazzisparis/JIP-LN-NVSE/blob/5a30ac4356ea0e93b9ff357b5031b1e420240a4d/internal/patches_game.h#L5047-L5097) assigns registered settings. It does not prove that archive loading or save-directory initialization occurs after those assignments. The compatibility investigation is `/tmp/mods-custom-ini-compatibility.md`. No Windows runtime evidence was collected.

Postrun preservation still checks concurrent canonical edits, validates child INIs, restores managed canonical keys, and publishes through the existing durable helpers. A canonically absent FalloutCustom.ini stays absent. Existing-file unrelated child edits persist. Uncertain Job drain, malformed edits, deletion, and publication errors retain temporary INIs under the existing policy.

## Operation evidence

The existing flat fixture contains base Data, one enabled mod, one disabled mod, and empty Overwrite. Both mods have metadata and an asset. Two files win.

| Launch preparation operation | Previous preparation | New launch preparation |
| --- | ---: | ---: |
| Provider directory enumerations | 4 | 3 |
| Provider metadata content reads | 2 | 1 |
| Full prelaunch preparation passes | 1 | 0 |
| Validation-only asset opens in the inventory | Per asset | 0 |
| Asset size/mtime collection in the inventory | Per asset | 0 |

The directory/read counts come from test-only counters through public preparation. The prior counts are recorded in [single-pass provider inventory](single-pass-provider-inventory.md). `cargo test -p infrastructure-environment execution_collects_each_provider_once -- --nocapture` printed `provider inventory: (3, 1), elapsed 1.00425ms`. This is one small macOS debug fixture, not a benchmark or Windows startup measurement. The counters do not count all filesystem calls, native VFS recursion, config reads, or safe metadata creation.

## Validation

- Focused nextest passed for domain, infrastructure-environment, and infrastructure-execution. The latest run includes the extra-entry/corrupt-BSA acceptance regression.
- Full `bun run check` passed after the settings expansion with 436 Rust tests and 119 tool tests. The consolidated repair passed with 438 Rust tests and 119 tool tests. Logs: `/tmp/streamline-check.log` and `/tmp/streamline-repair-check.log`.
- New tests cover one source resolution across preview/store, distinct composed bindings without downstream manifest parsing, concurrent settings edits, help/version without resource loading, init without a manifest, and moved-boundary error status.
- Focused operation counter passed with three provider walks and one metadata read.
- Public preparation tests cover empty/full modsets, duplicate/spelling mismatches, disabled malformed metadata and cycles, priority/subtree reinstatement, file/directory collisions, missing/concurrent metadata, hard links, linked provider directories, bounded cycles, large configuration, retained postrun edits, and pending temp ownership.
- INI tests cover canonical absence, existing Custom.ini precedence, one invalidation entry across repeated preparation, unrelated child edits, encoding, concurrent edits, and retained failure state.
- Archive-list fallback tests cover both lists absent across two preparations: the temporary Custom.ini gets the six default archives plus one invalidation entry. Separate cases show that an empty Custom.ini list and an empty Fallout.ini list each take precedence over the defaults. Every case leaves canonical files unchanged, and an absent canonical Custom.ini stays absent. The game fixture has no `Fallout_default.ini`.
- Archive-list fallback: focused domain and infrastructure-environment nextest passed 135 tests, `/tmp/archive-fallback-focused.log`. Full `bun run check` passed with 440 Rust tests, 2 release-version tests, and 119 tool tests, `/tmp/archive-fallback-check.log`. `git diff --check` passed.
- Strict export tests pass unchanged. A regression also confirms strict preparation does not create missing metadata and rejects the relaxed fixture.

Windows-only native adapter/lookup code is not compiled or exercised by macOS checks. Actual Windows process errors, linked-provider native recursion, JIP archive timing, and Custom.ini save timing remain unverified. No new platform test setup was added.

## Coding-style gate dispositions

The gate remains non-clean. The following dispositions cover changed-file findings reported during implementation, including findings on earlier intermediate versions. Existing baseline dispositions remain in [single-pass provider inventory](single-pass-provider-inventory.md) and [issue 31 execution composition](issue-31-execution-composition.md). They are not converted into clean findings here.

Paths below omit `src/infrastructure/` unless stated otherwise.

| File | Rule | Disposition |
| --- | --- | --- |
| `environment/src/snapshot.rs` | Capability modules and public APIs | Accepted. Only the existing metadata parser/result becomes crate-visible so strict and lightweight readers share schema rules. The leaf module remains private and no external API is added. |
| `environment/src/snapshot.rs` | Focused use-case orchestration | Accepted as inapplicable. This is the existing infrastructure snapshot capability, not an application use-case file. The parser is now reused. |
| `environment/src/snapshot.rs` | Use-case parameters | Accepted as inapplicable. Strict walker order remains. Helpers now take the command binding explicitly; cancellation remains last. |
| `environment/src/execution_preparation.rs` | Use-case parameters; Use-case declaration order | Accepted as inapplicable. These are infrastructure adapter methods and DTOs, not application entry points. Cancellation remains last. |
| `environment/src/execution_preparation.rs` | Narrow custom implementations | Accepted. Preparation composes project-owned environment rules with standard collections and filesystem APIs. It does not replace a runtime or general-purpose library. |
| `environment/src/execution_preparation.rs` | Test public behavior | Accepted. Tests call public preparation/check methods. Counters establish the requested operation budget; a test-only callback forces the project-owned create-new race. No production helper exists solely for testing. |
| `environment/src/execution_preparation.rs` | Phase spacing | Fixed around acquisition, profile reads, projection, and output. Accepted for adjacent fixture setup/assertions that belong to one scenario phase. |
| `environment/src/execution_preparation/inventory.rs` | Guard clauses; Choose the narrow conditional form | Accepted. Traversal failures return or continue. Metadata read/create recovery requires distinct success, absence, concurrent creation, and error outcomes. The remaining value-producing branches both continue. |
| `environment/src/execution_preparation/inventory.rs` | Phase spacing | Fixed in the consolidated repair. Added separators after modlist lookup construction, provider traversal, and metadata recovery before tombstone parsing. Focused and full checks are recorded below. |
| `environment/src/execution_preparation/inventory.rs` | Use-case parameters | Accepted as inapplicable. This private infrastructure algorithm needs explicit recursive budgets; it is not an application use case. |
| `environment/src/execution_preparation/inventory.rs` | Narrow custom implementations | Accepted. Priority projection and tombstone suppression are project rules. The implementation uses standard filesystem APIs, HashMap/HashSet, existing path identities, and the existing TOML parser. |
| `environment/src/execution_preparation/inventory.rs` | Pre-MVP test placement; Test public behavior | Accepted. No separate test target or cross-file test module was added. The parent preparation module owns the public-method tests and one test-only creation callback. It exercises the create-new race through preparation. |
| `environment/src/derived_profile.rs` | Guard clauses | Fixed, then superseded by the approved fallback. The refusal guard is removed. Archive selection is one value-producing `if`/`else if`/`else` chain with no exiting branch. |
| `environment/src/derived_profile.rs` | Phase spacing | Accepted. Reads, text validation, archive selection, staging, and durable publication retain separate phases. Per-file operations remain together. For the fallback change, the gate flagged this rule at low confidence. Blank lines now separate each new test's setup, derivation checks, `preserve` call, canonical-file assertions, and final game-directory check. The recheck after that edit no longer reports this rule. |
| `environment/src/derived_profile.rs` | Use-case parameters | Accepted as inapplicable. These are infrastructure INI owner methods, not application use cases. The fallback change keeps every signature. The gate still reports this rule after that change. |
| `environment/src/profile.rs` | Focused use-case orchestration; Cohesive orchestration | Accepted. The extracted text validator is reused by strict bounded reads and lightweight already-read texts. This is an infrastructure capability, not an application use-case file. |
| `environment/src/profile.rs` | Phase spacing | Accepted. The shared text-validation call follows decoding as one acquisition/validation phase. The abandoned structural-only launch validator was removed. |
| `environment/src/profile.rs` | Guard clauses | Accepted for existing multi-way profile handling. No new successful path remains below an exiting opposite branch. |
| `environment/src/profile.rs` | Use-case parameters | Accepted as inapplicable. These are infrastructure profile functions. |
| `environment/src/manifest.rs` | Focused use-case orchestration; Use-case parameters | Accepted as inapplicable. The parser remains an infrastructure integrity validator for initialization and strict export. Binding discovery was removed; exec no longer parses it. |
| `dependencies/src/execution_adapter/native.rs` | Language-neutral review priorities | Accepted. The removals implement the approved policy, not an unapproved safety optimization. Existing Job supervision, cleanup, and preservation remain. |
| `dependencies/src/execution_adapter/native.rs` | Use-case parameters; Use-case declaration order | Accepted as inapplicable. This is an infrastructure adapter method with its original signature. |
| `execution/src/launch_inputs/windows_inputs.rs` | Preserve causes at owned boundaries | Accepted. Candidate metadata errors do not become returned launch failures. Non-missing candidates proceed to Windows, whose launch error the adapter preserves. This deliberate lookup policy avoids falling through to a different executable on access failure. |
| `execution/src/launch_inputs/windows_inputs.rs` | Guard clauses; Choose the narrow conditional form | Fixed. Missing candidates now continue before the successful selection path. The earlier multi-arm selection match was removed. |
| `execution/src/launch_inputs/windows_inputs.rs` | Language-neutral review priorities | Accepted. The user explicitly chose to defer access/format failures to Windows. Captured caller lookup and string constraints remain; native behavior is a reported platform gap. |
| `execution/src/launch_inputs/windows_inputs.rs` | Phase spacing | Fixed around lookup selection, cwd resolution, command encoding, and output. |
| `execution/src/profile.rs` | Use-case parameters | Accepted as inapplicable. The existing infrastructure profile builder keeps its input DTO. |
| `src/domain/src/profile_state.rs` | Self-explanatory code; Reason comments; Rustdoc format | Accepted. The Rustdoc records the JIP prerequisite, deferred GECK support, and caller materialization obligation. These are external compatibility contracts, not narration of the implementation. No error section is needed for this infallible transform. |
| `src/domain/src/profile_state.rs` | Phase spacing | Fixed between archive selection and emission. The remaining statements form one managed-section transformation. |

| `game_platform/src/resolution.rs` | Language-neutral review priorities | Accepted. Removing the build comparison is an explicit user requirement. Strict path validation and environment/game separation remain. |
| `game_platform/src/resolution.rs` | Use-case parameters | Accepted as inapplicable. Existing infrastructure adapter signatures remain; cancellation is last where present. |
| `game_platform/src/steam/validation.rs` | Language-neutral review priorities | Accepted. Build freshness is no longer a policy. App identity, install directory, executable, default INI, and safe filesystem validation remain. |
| `game_platform/src/bound_game.rs` | Test public behavior | Accepted. The test establishes the project-owned policy that reopening accepts a changed Steam build, rather than testing a dependency. It exercises the capability boundary used by installation. |
| `game_platform/src/bound_game.rs` | Use-case parameters | Accepted as inapplicable. The existing infrastructure function receives the expected binding then final cancellation token. |
| `settings/src/lib.rs` | Use-case parameters | Accepted as inapplicable. Infrastructure loading returns a command snapshot; publication receives that explicit snapshot. These are capability methods, not application use-case entries. |
| `environment/src/export.rs` | Use-case parameters | Accepted. Only test fixtures lose the obsolete constructor argument. Production parameter order did not change. |
| `environment/src/conflict_scan.rs` | Use-case parameters | Accepted. The infrastructure scan and content-reader receive the binding explicitly before final cancellation. They do not discover settings. |
| `environment/src/lib.rs` | Use-case parameters | Accepted. This infrastructure adapter binds the explicitly supplied game directory into command-local ports. Application-owned callable ports still use `.call` at invocation. |
| `src/application/src/installation/install_archive.rs` | Use-case parameters | Accepted. The production use case retains dependencies first and cancellation last. Only obsolete binding fixture arguments changed. |
| `src/application/src/environment/initialize_environment.rs` | Use-case parameters | Accepted. Dependencies remain first and cancellation last. Only fixture binding construction and its assertion changed. |
| `src/application/src/settings/set_game_directory.rs` | Use-case declaration order | Accepted as a false positive. Declarations remain Dependencies, Output, Error, then the use-case function. Removing build-specific fields does not alter that order. |
| `src/presentation/cli/src/output.rs` | Language-neutral review priorities | Accepted. The build-warning formatter intentionally returns empty output after the approved warning removal. It retains the existing quiet successful-mutation contract, with a regression test. No unrelated diagnostic policy changed. |

### Settings-expansion dispositions

These include intermediate findings as well as the final diff. The gate is still non-clean. An enforce-mode override remains required; this table does not clear it.

| File | Rule | Disposition |
| --- | --- | --- |
| `src/application/src/ports/settings.rs` | Use-case parameters | Accepted as inapplicable. This file declares application callable-port types rather than a use case. Existing binding inputs and final cancellation remain; only LoadSettings was removed. |
| `src/application/src/settings/get_setting.rs` | Use-case parameters | Accepted as a false positive. Dependencies are the first by-value argument. Supplied setting records and the requested key are separate typed business arguments. This read-only use case has no cancellation parameter. |
| `src/application/src/settings/get_setting.rs` | Import placement and use | Accepted as a false positive. Imports remain at module scope, including cfg(test) atomics and test-module imports. No function-local import was added. Qualification in tests distinguishes the function under test or a fixture namespace. |
| `src/application/src/settings/list_settings.rs` | Use-case declaration order | Accepted as a false positive. The file declares ListSettingsDependencies, ListSettingsOutput, ListSettingsError, then the instrumented async function in that order. |
| `src/application/src/settings/list_settings.rs` | Use-case parameters | Accepted as a false positive. Dependencies are the first by-value argument. Supplied setting records and the requested key are separate typed business arguments. This read-only use case has no cancellation parameter. |
| `src/application/src/settings/list_settings.rs` | Preserve causes at owned boundaries | Accepted as inapplicable. This use case now receives records and performs no fallible load. The moved presentation load boundary retains the rootcause report and renders only its allowlisted marker; regression coverage replaces the obsolete load-port cause test. |
| `src/application/src/settings/list_settings.rs` | Language-neutral review priorities | Accepted. Removing the hidden settings load is expressly approved architecture, not a terseness optimization. List output and provenance remain unchanged; source-load errors are tested at their new presentation owner. |
| `dependencies/src/execute_program.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/execute_program.rs` | Dependency direction and composition roots | Accepted as a false positive. CLI remains the composition root. Infrastructure dependencies composes application-owned ports from infrastructure adapters and domain bindings. Workspace dependency checks pass; no dependency was added. |
| `dependencies/src/execution_adapter.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/explain_path.rs` | Callable port invocation | Accepted as a false positive after direct inspection and an ast-grep call search. This file calls inherent adapter factory methods to construct ports; it never invokes an application-owned callable port. |
| `dependencies/src/explain_path.rs` | Use-case declaration order | Accepted as inapplicable. This is an infrastructure capability or composition method, not a use-case file with Dependencies/Output/Error declarations. |
| `dependencies/src/explain_path.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/explain_path.rs` | Dependency direction and composition roots | Accepted as a false positive. CLI remains the composition root. Infrastructure dependencies composes application-owned ports from infrastructure adapters and domain bindings. Workspace dependency checks pass; no dependency was added. |
| `dependencies/src/export_environment.rs` | Use-case declaration order | Accepted as inapplicable. This is an infrastructure capability or composition method, not a use-case file with Dependencies/Output/Error declarations. |
| `dependencies/src/export_environment.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/export_environment.rs` | Phase spacing | Accepted. Supplied-binding validation, capture, publication wrapping, and output construction remain separate phases. Closure captures belong to their single composition operation. |
| `dependencies/src/get_setting.rs` | Callable port invocation | Accepted as a false positive. This composition method constructs a dependency value containing only report_progress. It does not invoke a port. |
| `dependencies/src/inspect_mod_conflicts.rs` | Callable port invocation | Accepted as a false positive after direct inspection and an ast-grep call search. This file calls inherent adapter factory methods to construct ports; it never invokes an application-owned callable port. |
| `dependencies/src/inspect_mod_conflicts.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/inspect_mod_conflicts.rs` | Dependency direction and composition roots | Accepted as a false positive. CLI remains the composition root. Infrastructure dependencies composes application-owned ports from infrastructure adapters and domain bindings. Workspace dependency checks pass; no dependency was added. |
| `dependencies/src/install_archive.rs` | Callable port invocation | Accepted as a false positive after direct inspection and an ast-grep call search. This file calls inherent adapter factory methods to construct ports; it never invokes an application-owned callable port. |
| `dependencies/src/install_archive.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/lib.rs` | Use-case declaration order | Accepted as inapplicable. This is an infrastructure capability or composition method, not a use-case file with Dependencies/Output/Error declarations. |
| `dependencies/src/lib.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/lib.rs` | Dependency direction and composition roots | Accepted as a false positive. CLI remains the composition root. Infrastructure dependencies composes application-owned ports from infrastructure adapters and domain bindings. Workspace dependency checks pass; no dependency was added. |
| `dependencies/src/list_effective_conflicts.rs` | Callable port invocation | Accepted as a false positive after direct inspection and an ast-grep call search. This file calls inherent adapter factory methods to construct ports; it never invokes an application-owned callable port. |
| `dependencies/src/list_effective_conflicts.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `dependencies/src/set_game_directory.rs` | Callable port invocation | Accepted as a false positive after direct inspection and an ast-grep call search. This file calls inherent adapter factory methods to construct ports; it never invokes an application-owned callable port. |
| `dependencies/src/set_game_directory.rs` | Use-case declaration order | Accepted as inapplicable. This is an infrastructure capability or composition method, not a use-case file with Dependencies/Output/Error declarations. |
| `dependencies/src/set_game_directory.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `environment/src/conflict_scan.rs` | Use-case declaration order | Accepted as inapplicable. This is an infrastructure capability or composition method, not a use-case file with Dependencies/Output/Error declarations. |
| `environment/src/conflict_scan.rs` | Preserve causes at owned boundaries | Accepted for test-fixture error conversion only. The new propagation test maps setup/scan failures to fixture failure text, following the existing test convention. Production scan/content boundaries still preserve underlying causes. |
| `environment/src/conflict_scan.rs` | Phase spacing | Accepted. The supplied-binding test separates fixture setup, port invocation, and output assertions; provider traversal phases are unchanged. |
| `environment/src/lib.rs` | Import placement and use | Accepted as a false positive. Imports remain at module scope, including cfg(test) atomics and test-module imports. No function-local import was added. Qualification in tests distinguishes the function under test or a fixture namespace. |
| `environment/src/profile.rs` | Use-case declaration order | Accepted as inapplicable. This is an infrastructure capability or composition method, not a use-case file with Dependencies/Output/Error declarations. |
| `environment/src/transactions.rs` | Use-case declaration order | Accepted as inapplicable. This is an infrastructure capability or composition method, not a use-case file with Dependencies/Output/Error declarations. |
| `environment/src/transactions.rs` | Use-case parameters | Accepted as inapplicable. This is infrastructure capability/composition code, not an application use-case entry point. The supplied binding or snapshot is explicit, and cancellation remains final where supported. |
| `environment/src/transactions.rs` | Test public behavior | Accepted. Existing fault-injection tests verify project publication/cleanup policy. Their only new input is the explicit fixture binding; they were not relaxed. |
| `settings/src/lib.rs` | Narrow custom implementations | Accepted. The new state is a command-local immutable settings snapshot and byte comparison around existing durable publication. Parsing still uses Config/TOML; no replacement parser, general cache, or singleton was added. |
| `settings/src/lib.rs` | Import placement and use | Accepted as a false positive. Imports remain at module scope, including cfg(test) atomics and test-module imports. No function-local import was added. Qualification in tests distinguishes the function under test or a fixture namespace. |
| `settings/src/lib.rs` | Phase spacing | Fixed acquisition/resolution/output spacing. Accepted remaining adjacency where a read and its source comparison form one integrity operation, or a proposed binding and its provenance form one transformation. |
| `settings/src/lib.rs` | Test public behavior | Accepted. Tests exercise project-owned snapshot reuse, source-byte concurrency, and publication refusal. The source counter is test-only and asserts the requested one-load policy rather than dependency behavior. |
| `settings/src/manifest_writer.rs` | Use-case parameters | Accepted. This is the existing private infrastructure writer, not a use case. Its validation callback stays after cancellation to preserve the established local API; expected source bytes are explicit. |
| `settings/src/manifest_writer.rs` | Phase spacing | Fixed in the consolidated repair. Added a blank line after drop(staged) before reading the canonical manifest. This separates staged validation from the source-byte concurrency check. |
| `src/presentation/cli/src/main.rs` | Use-case parameters | Accepted as inapplicable. This is presentation dispatch/composition, not an application use case. Existing root/startup inputs and the command-owned composition callback remain explicit. |
| `src/presentation/cli/src/main.rs` | Phase spacing | Fixed in the consolidated repair. Added a blank line after the returning initialization guard, before settings-load mode selection. |
| `src/presentation/cli/src/runner.rs` | Callable port invocation | Accepted as inapplicable to the dependency_factory callback. That callback is presentation-owned composition, not an application port. Application use cases are ordinary functions; their internal ports use `.call`. |
| `src/presentation/cli/src/runner.rs` | Use-case declaration order | Accepted as inapplicable. Presentation dispatch owns command/resource variants, not application use-case declarations. |
| `src/presentation/cli/src/runner.rs` | Use-case parameters | Accepted as inapplicable. This is presentation dispatch/composition, not an application use case. Existing root/startup inputs and the command-owned composition callback remain explicit. |
| `src/presentation/cli/src/runner.rs` | Guard clauses | Accepted. Composition/dispatch matches produce alternative command results; neither branch is an exiting guard followed by a nested success path. Existing early errors still return. |
| `src/presentation/cli/src/runner.rs` | Narrow custom implementations | Accepted. Two command-dependency variants distinguish initialization from commands that require settings. This is project dispatch policy, not a new framework or runtime. |
| `src/presentation/cli/src/runner.rs` | Phase spacing | Accepted. Parsing, root selection, diagnostics, command composition, dispatch, and output cleanup retain separate phases. New tests separate setup/action/assertion. |
| `src/presentation/cli/src/runner.rs` | Test public behavior | Accepted. The runner is the presentation boundary under test. Tests assert visible exit status, redacted output, init without a manifest, and help/version bypass; no dependency internals are inspected. |

### Parent gate reconciliation

The parent gate snapshot `/tmp/streamline-parent-latest-gate.txt` also reports the following pairs. Unchanged files are identified against the frozen base, not treated as clean.

| File | Rule | Disposition |
| --- | --- | --- |
| `src/application/src/execution/mod.rs` | Use-case declaration order | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. This parent module only declares private leaves and re-exports their API; it is not the use-case implementation. |
| `src/application/src/export/export_environment/inventory.rs` | Use-case parameters | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. This is an inventory helper, port/type declaration, or presentation formatter rather than an application entry point. |
| `src/application/src/export/export_environment/inventory.rs` | Phase spacing | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. The existing inventory/formatting statements retain their original phase grouping; no edit was made here. |
| `src/application/src/export/mod.rs` | Use-case declaration order | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. This parent module only declares private leaves and re-exports their API; it is not the use-case implementation. |
| `src/application/src/export/types.rs` | Use-case parameters | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. This is an inventory helper, port/type declaration, or presentation formatter rather than an application entry point. |
| `src/domain/src/profile_state.rs` | Focused use-case orchestration | Accepted as inapplicable. This domain INI transformation is not a use case. It owns the archive/save override policy shared by execution and export. |
| `src/domain/src/profile_state.rs` | Guard clauses | Accepted. The archive-source selection returns a value from alternative inputs. There is no exiting opposite branch nesting the success path. |
| `src/domain/src/profile_state.rs` | Narrow custom implementations | Accepted. This is the game-specific Custom.ini override policy using the existing IniDocument model, not a general parser. |
| `dependencies/src/execution_adapter/native.rs` | Readability before secondary cleanup | Accepted. Existing comments still explain Job lifetime, cleanup, and retention invariants. The change removes preflight work without secondary stylistic cleanup. |
| `dependencies/src/execution_adapter/native.rs` | Phase spacing | Accepted. Caller lookup, supplied-binding preparation, profile construction, execution, and postrun preservation remain separate blocks. |
| `dependencies/src/export_environment.rs` | Narrow custom implementations | Accepted. The wrapper binds one supplied game path and compares original source bytes before strict export. It reuses existing validation/publication ports instead of implementing a new export engine. |
| `environment/src/derived_profile.rs` | Narrow custom implementations | Accepted. Temporary INI generation and managed-key restoration are project-owned compatibility rules. Existing domain INI parsing and durable filesystem helpers remain. |
| `environment/src/derived_profile.rs` | Test public behavior | Accepted. Tests exercise derive/preserve operations, absent canonical Custom.ini, child edits, and refusal/retention policy. They do not recreate the INI dependency test suite. |
| `environment/src/export.rs` | Use-case declaration order | Accepted as inapplicable. This is the existing infrastructure exporter. Its application use-case declarations live in application/export/export_environment.rs. |
| `environment/src/export.rs` | Narrow custom implementations | Accepted as unchanged baseline behavior. Only obsolete binding fixture arguments were removed. Strict export still uses its existing bounded source/publication capability. |
| `environment/src/export.rs` | Phase spacing | Accepted as unchanged baseline behavior. The only diff removes binding fixture fields; export capture/publication phase spacing is unchanged. |
| `environment/src/export.rs` | Test public behavior | Accepted. Existing public export/fault-injection tests cover project-owned source-change and durable-publication policy. No assertion was relaxed for exec simplification. |
| `environment/src/export_publication.rs` | Rustdoc format | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. This private platform publication helper retains its existing safety comment; its documentation was not edited. |
| `environment/src/safe_fs.rs` | Narrow custom implementations | Fixed after spec review. The uncapped read now acquires the existing checked regular handle and calls the standard read_to_end API. No asset-validation open, custom read loop, or byte cap was restored. |
| `environment/src/safe_fs.rs` | Phase spacing | Fixed in the consolidated repair. Checked-handle acquisition, standard whole-file reading, and output have separate blocks. |
| `environment/src/snapshot.rs` | Narrow custom implementations | Accepted. This is the retained strict inventory capability. The changes expose existing metadata schema parsing and pass a supplied binding; they do not add a general-purpose facility. |
| `environment/src/snapshot.rs` | Phase spacing | Accepted. Provider acquisition, projection, and dependency evaluation retain separate phases. Explicit binding propagation replaces the old local discovery branch in place. |
| `environment/src/snapshot.rs` | Test public behavior | Accepted. Existing scenario tests now supply their fixture binding explicitly. They still assert inventory/dependency policy rather than implementation collection shapes. |
| `src/presentation/cli/src/error.rs` | Phase spacing | Accepted. Removing the build-ID rendering branch leaves the existing optional allowlisted field assembly contiguous as one output phase. |
| `src/presentation/cli/src/export_output.rs` | Use-case parameters | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. This is an inventory helper, port/type declaration, or presentation formatter rather than an application entry point. |
| `src/presentation/cli/src/export_output.rs` | Import placement and use | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. Imports are at module scope in the existing formatter and its test module. |
| `src/presentation/cli/src/export_output.rs` | Phase spacing | Accepted as unchanged baseline, outside this implementation diff against `5eb0f192`. The existing inventory/formatting statements retain their original phase grouping; no edit was made here. |
| `src/presentation/cli/src/main.rs` | Use-case declaration order | Accepted as inapplicable. main.rs owns binary composition, not a Dependencies/Output/Error application use case. |

## Consolidated review repair

The independent [spec review](streamline-exec-spec-review.md) found one safe-open regression. The independent [standards review](streamline-exec-standards-review.md) found three phase-spacing breaches and includes the full 95-pair gate audit. These linked reviews describe the pre-repair snapshot. They are not claims that reviewers reran the repaired code.

`SafeDir::read` now opens through `open_regular` before calling the standard uncapped `read_to_end`. This restores no-follow, regular-file, reparse-point, and single-link checks for required INI content and postrun preservation. It does not restore asset validation, configuration caps, chunk loops, or freshness checks.

New tests call public prepare/derive/preserve operations. They add hard links to canonical or staged Fallout.ini after derivation and modify child INI content. Preservation must fail, leave canonical bytes unchanged, retain child edits and the temporary directory, and report the typed retained path plus the I/O cause. A Unix regression also verifies that required Fallout.ini cannot be a relative symlink. Both tests failed before the fix and passed afterward.

The three reviewed phase boundaries are fixed in `settings/src/manifest_writer.rs`, `execution_preparation/inventory.rs`, and CLI `main.rs`. The inventory also separates lookup construction and final modset comparison. Their earlier accepted spacing dispositions are superseded by fixed/rechecked entries above.

- Red regression run: two failures reproduced the safe-open regression, `/tmp/streamline-repair-red.log`.
- Focused workspace run: 158 tests passed, `/tmp/streamline-repair-focused.log`.
- Full repair check: 438 Rust tests and 119 tool tests passed, `/tmp/streamline-repair-check.log`. `git diff --check` passed.
- A package-only focused command could not compile the earlier `tokio::test` composed-port test because that package relies on workspace feature unification. The focused check uses the documented workspace environment instead. No dependency change was made in this bounded repair.

The user later approved the archive-list fallback in [Status and scope](#status-and-scope). The Windows/JIP evidence gaps and non-clean style-gate status remain. No second repair cycle or unrelated refactor was made.

Evidence prose was edited with the local Unslop process. No commit, push, PR, or external repository message was made.
