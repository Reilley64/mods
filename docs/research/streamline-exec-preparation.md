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

Composition runs the whole use case on one `spawn_blocking` thread with a current-thread Tokio runtime. `ExecutionAdapter::into_execute_program` builds the ports and calls the use case on that thread. It keeps the tracing dispatcher and span handoff that the old adapter wrapper used. A cancelled request returns `operation_cancelled` before the thread starts. Non-Windows builds return `program_unsupported` at the same point as before. The CLI holds this entry point as a `Send` boxed `FnOnce`.

`VirtualGameView` is now `Send` but not `Sync` (`unsafe impl Send` in `usvfs/mod.rs`). As a result, `HookedProcess` is also `Send`. Evidence from the pinned `usvfs-rs@c23705c` source:

- `usvfsConnectVFS` and `usvfsDisconnectVFS` store and delete the process-global static `context` and `manager` (`src/usvfs_dll/usvfs.cpp`).
- Every API call locks `HookContext` through `readAccess` or `writeAccess`, and its scoped pointer unlocks before the call returns (`src/usvfs_dll/hookcontext.cpp:131-146`).
- The RecursiveBenaphore records a thread owner only while one call holds it (`src/usvfs_dll/semaphore.cpp`).
- The controller path and the shim (`rust/usvfs-sys/native/barrier.cpp`) have no thread-local state and start no threads. The hook manager exists only inside injected children.

So upstream needs calls that never overlap, not a fixed thread. Overlap is prevented by `SESSION_ACTIVE` (one session per process), by the view staying `!Sync`, by every native call taking `&mut self` or `self`, and by the single execution thread.

Residual risk: this conclusion comes from reading source, not from running on Windows. The Windows-only test `session_moves_between_threads_for_configure_launch_and_close` covers it. It configures a real view on one thread, launches a hooked `cmd.exe` on a second thread that reads a mapped file, and finishes the process and closes the session on a third thread. It runs three cycles, then closes an unlaunched view on another thread. The test was not compiled or run on macOS. The parent will run `cargo test --package infrastructure-execution` on Windows for i686 and x86_64. The test and the existing session test share a lock, because `SESSION_ACTIVE` is process-global.

### Windows-only code touched

None of this code compiles on macOS. It was checked by reading only.

- `src/infrastructure/dependencies/src/execution_adapter/native.rs`: rewritten. `ExecutionAdapter::dependencies(&self) -> ExecuteProgramDependencies` replaces `ExecutionAdapter::execute`. New private step functions, plus `NativeLaunchTarget`, `ProgramStreams`, and `NativeProgram`.
- `src/infrastructure/dependencies/src/execution_adapter.rs`: the `#[cfg(windows)]` branch of `into_execute_program` (replaces `run_port`).
- `src/infrastructure/execution/src/usvfs/mod.rs`: `unsafe impl Send for VirtualGameView`. The `_thread: PhantomData<Rc<()>>` field is removed. New cross-thread test and a shared session test lock.
- `src/infrastructure/execution/src/launch_inputs.rs` and `lib.rs`: `ResolvedLaunch` is re-exported under `cfg(windows)`.

### Validation

- The application use-case tests use fake ports. They cover step order and progress events, warning mapping, Overwrite and Data mod output targets, missing and disabled output targets, cancellation before start and after VFS creation (handle dropped, no launch, retained INIs), an undrained Job, an unknown drain state, and a failed undrained supervision (all retained). They also cover a drained supervision failure (preserved first, not retained), failed preservation (retained), forced cancellation (after preserve, finish, and an uncancelled post-run check), and cause preservation. 9 tests.
- CLI runner tests use the new entry point.
- Focused run (use case and CLI exec tests): 16 passed, `/tmp/exec-ports-focused.log`. Full `bun run check` passed with 451 Rust tests, 2 release-version tests, and 119 tool tests, `/tmp/exec-ports-check.log`. `git diff --check` passed. Windows compilation and the new Windows test remain unverified here.

### Coding-style gate dispositions

The gate remains non-clean. The CLI crate now enables `fn_traits`, so `runner.rs` calls the exec entry point with `.call_once(...)`.

| File | Rule | Disposition |
| --- | --- | --- |
| `src/application/src/execution/execute_program.rs` | Cancellation state preservation | Accepted. An unlaunched view is dropped on cancellation, which releases the native session the same way the old explicit close did; the user approved not reporting its teardown error. Forced cancellation reports `cleanup` after preservation and the post-run check. The staged profile is retained, not removed. |
| `src/application/src/execution/execute_program.rs` | Cancellation propagation and checkpoints | Accepted. The parent contract assigns checks between steps to the use case. Each is an inline `is_cancelled()` guard. The original token reaches every port. |
| `src/application/src/execution/execute_program.rs` | Test public behavior | Accepted. Tests call the public use case with fake ports. They assert project-owned order, retention, and warning policy, not dependency internals. |
| `src/application/src/execution/execute_program.rs` | Phase spacing; Narrow custom implementations; Use-case declaration order | Phase spacing fixed between resolve, prepare, projection, and warning mapping. The rest is accepted: the file declares Dependencies, Output, Error, then the instrumented function, and it adds no general-purpose facility. |
| `src/application/src/ports/execution.rs` | Capability modules and public APIs | Accepted. The ports module is the existing capability interface. The handle types are opaque on purpose; their state is private to adapters. |
| `src/application/src/ports/execution.rs` | Narrow custom implementations | Accepted. `AdapterState` only wraps `Box<dyn Any + Send>` so application signatures carry no infrastructure types. It is not a runtime primitive. |
| `src/application/src/ports/execution.rs`, `ports/mod.rs` | Use-case parameters | Accepted as inapplicable. These are port type declarations. Every port that takes cancellation takes it last. |
| `src/infrastructure/dependencies/src/execution_adapter/native.rs` | Use-case parameters; Use-case declaration order | Accepted as inapplicable. These are infrastructure adapter functions. Cancellation is last where present. |
| `src/infrastructure/dependencies/src/execution_adapter/native.rs` | Cancellation propagation and checkpoints | Accepted. The checkpoint between view validation and native setup is the existing infrastructure checkpoint, moved unchanged. |
| `src/infrastructure/dependencies/src/execution_adapter/native.rs` | Phase spacing; Narrow custom implementations | Phase spacing fixed around projection input, view configuration, and the drain query. The rest is accepted: the code moved from the old adapter and adds no new facility. |
| `src/infrastructure/dependencies/src/execution_adapter.rs`, `execute_program.rs` | Use-case parameters; Use-case declaration order; Phase spacing | Accepted as inapplicable. These are composition methods, not application use cases. The thread entry keeps the old wrapper's order. |
| `src/infrastructure/execution/src/usvfs/mod.rs` | Rustdoc format | Accepted as a false positive. The `// SAFETY:` comment before `unsafe impl Send` is the proof format that CODING_STYLE requires, not item documentation. |
| `src/infrastructure/execution/src/usvfs/mod.rs` | Test public behavior; Phase spacing | Accepted. The Windows test exercises the native session's thread contract, which is a project boundary. Setup, each thread step, and the assertions are separate blocks. |
| `src/presentation/cli/src/main.rs` | Callable port invocation | Accepted as a false positive. `resources.execute_program(...)` is an inherent composition factory, not an application port call. |
| `src/presentation/cli/src/runner.rs`, `main.rs` | Use-case parameters; Use-case declaration order; Narrow custom implementations | Accepted as inapplicable. The runner holds a presentation-owned entry-point type and passes cancellation last. |
| `src/infrastructure/environment/src/execution_preparation.rs`, `export.rs` | Phase spacing; Use-case parameters | Already recorded for the INI fix in commit `1e37225`. Unchanged here. |
| `src/application/src/execution/execute_program.rs` | Cancellation propagation and checkpoints; Test public behavior; Use-case parameters | Close-port removal: accepted. The checkpoint after file-system creation is still an inline guard owned by the use case, as the parent contract requires. The test fixture's drop recorder shows that the use case drops the handle; it does not inspect adapter internals. Dependencies stay first and cancellation stays last. |
| `src/application/src/ports/execution.rs`, `native.rs` | Use-case parameters | Close-port removal: accepted as inapplicable. Only a port type and its adapter were removed. |

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
