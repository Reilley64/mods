# Exec performance discussion

## Working agreement

Record this discussion here as it continues. Do not implement any proposed change until the user explicitly says to implement. Discussion, suggestions and agreement on a design are not implementation authorization. Documentation updates are authorized.

This record concerns installed PR #141 revision `5eb0f192d2a7c5346d66280c4a46fe6c6a894b7b`. No game should be launched without separate authorization. Preserve previous installations and the user's environment.

## Implementation authorization and frozen scope

The user has now explicitly authorized implementation of the recorded decisions. Earlier statements that implementation is paused describe the discussion history, not the current authorization.

Implement on a separate branch above PR #141. Scope is exec preparation and launch: remove the rejected Steam prerequisites and repeated binding checks, prelaunch change detection, inventory ancestry/link rejection, validation-only data-file opens and size/mtime bookkeeping. Build case-insensitive priority winners during traversal and collect the discovered mod-folder set in that same traversal. Keep explicit plugin order and use collection order for unlisted plugins. Create missing mod-root metadata when exec requests it. Simplify preliminary executable validation while preserving caller PATH/cwd and quoting semantics. Implement shared temporary FalloutCustom.ini archive/save overrides subject to verified INI behavior; do not silently break vanilla archive loading.

Keep CLI syntax, existing manifest readability, export/install and dry-run behavior, tombstone/namespace semantics, cancellation, native session/Job supervision and post-run INI preservation. Existing build IDs remain readable metadata; exec must not query Steam to validate them. No format migration, dependencies, native runtime redesign, bulk environment mutation, game launch or installation as part of this implementation. Directory links may be followed only under existing finite traversal limits; native compatibility remains a validation concern.

Expected modules: execution adapter/preparation, snapshot collection or a precise private execution-inventory module, launch inputs, execution profile/derived-profile logic, shared INI domain helpers, tests and evidence. Internal execution DTO changes needed to remove metadata/freshness fields are in scope; CLI/application port and manifest wire formats remain frozen. Run focused regressions and full existing checks, then independent read-only spec and standards reviews and one consolidated repair cycle. Real Windows behavior not exercised must be reported as unverified.

## Current implementation and completed work

- Optional mod metadata is in PR #140. Missing `meta.toml` means empty metadata; present metadata still validates and applies tombstones.
- PR #141 collects one validated inventory per provider per preparation and resolves analytical winners from those inventories in priority order. Disabled providers still validate but contribute no winners.
- Folder discovery builds a HashSet of exact folder names and compares it with modlist names. Duplicate and case-collision checks remain. The ordered modlist separately retains priority and enabled state.
- The small fixture changed from 7 provider directory enumerations, 5 metadata reads and 2 winning-file reopens to 4, 2 and 0. This is not a production Windows timing result.
- Build `5eb0f19` is installed for `reill`; package checks and CI passed. The user still reports slow execution.

## Tokio filesystem discussion

The repository does not currently enable Tokio's `fs` feature or use `tokio::fs`. Its safe handle-based filesystem operations are synchronous. Execution already moves onto `spawn_blocking`, with a current-thread Tokio runtime inside that worker to preserve the thread-affine native session and support supervision timers.

The user accepted keeping these synchronous operations rather than requiring a Tokio filesystem rewrite. Tokio filesystem operations themselves currently use blocking workers; merely enabling `fs` would not eliminate repeated scans. No async filesystem rewrite is approved.

## Executable validation discussion

The user asked what opening the executable means. It opens a file handle to the `.exe`, not a process. The code checks file type, resolves the final path and retains the handle until launch. It does not read the entire executable or execute it during this step.

The user questioned why these checks are needed instead of letting Windows launch the requested program and report errors.

Proposed simplification, not implemented:

1. Preserve argument quoting and intended PATH and working-directory semantics.
2. Resolve the executable path only as needed.
3. Let Windows process creation validate access, executable format and launchability, and report its errors.
4. Reassess the preliminary executable and working-directory handle checks rather than assuming all are needed.

The current checks provide early failure before costly preparation and explicit caller-based resolution. The retained handle is not passed to Windows as the executable to run: launch ultimately uses a path. It is not equivalent to execution by verified file descriptor. Exact sharing restrictions and any race guarantees must be checked before changing this behavior.

No decision has been made about the precise replacement resolution algorithm. This is a simplification candidate, not a proven major speedup. Slow PATH locations could matter, but executable lookup examines far fewer files than provider inventory.

## Game directory after initialization

The user asked whether the game directory matters after initialization copies INIs. Yes: initialization does not copy the game installation into the environment.

- Game `Data` remains the base provider for analytical winner resolution, including vanilla files not replaced by mods.
- Runtime mod overlays target the real game `Data` path, so the launched game sees the virtualized files where it expects them.
- Execution still validates the bound Steam installation/build and environment/game separation. Needing the directory does not establish that every repeated validation is necessary.
- Derived execution INIs can still read `Fallout_default.ini` from the game directory as an archive-list fallback when canonical profile INIs do not supply it.
- The requested executable can be a tool elsewhere; the bound game directory still supplies the base data and mapping destination. Executable resolution is separate from the game binding.

Discussion distinction: retaining the game-directory binding is necessary under the current overlay model; repeatedly scanning or fully validating it is a separate optimization/policy question. No change is authorized.

### Current game-directory validation details

The user asked which checks actually run. Steam installation validation in `game_platform/src/steam/validation.rs` requires a `steamapps/common/<game>` layout, safely opens directories and checks that reopening the supplied path identifies the same game directory. It opens `FalloutNV.exe` and `Fallout_default.ini` as regular files and requires `Data` to be a directory. It enumerates the top level of Data to reject the reserved `Fallout - Invalidation.bsa` name, case-insensitively. It reads `appmanifest_22380.acf`, validates its structure, requires app ID 22380, matches `installdir` to the game folder, and validates the build ID. Effective binding validation compares the observed build with the recorded expected build. Directory-separation checks reject either the game or environment containing the other, including equality.

These installation checks are distinct from recursive base-provider validation. Snapshot preparation separately walks game Data, checking names, case collisions, file/directory types, reparse points, hard links, ancestry and traversal budgets while collecting size/mtime and base winners. It does not hash all game assets or verify their contents against Steam. The installation check opens the executable but does not validate executable bytes or its version. Native exec repeats binding validation twice before launch and performs base-provider inventories in initial, prelaunch and successful post-run preparation.

### Decision: remove these game-installation prerequisites

The user explicitly decided that the following checks are unnecessary:

- Requiring the `steamapps/common/<game>` path layout.
- Requiring `FalloutNV.exe` and `Fallout_default.ini` to be regular files as installation prerequisites.
- Enumerating Data's top level to reject `Fallout - Invalidation.bsa`.
- Reading `appmanifest_22380.acf` and performing its app ID, installation-folder and build checks.

Record these as decisions for future implementation, not merely optimization candidates. Implementation remains paused until explicitly authorized.

Scope distinctions to resolve when implementing: removing the reserved-archive installation check alone would leave the separate base-provider inventory rejection in place. Removing Steam manifest/build checks also affects how the existing binding's required build ID is obtained and interpreted. Optional fallback reads of `Fallout_default.ini` are separate from requiring that file during installation validation. Do not infer that discovery, all safe file-read checks, Data-directory existence, environment/game separation, or recursive provider validation are also to be removed; those were not decided in this message. The scope across init, exec and other commands still needs to be made explicit before implementation.

### INI fallback explanation

The user asked what INI fallback means. `ProfileIniInputs::read` selects `[Archive] sArchiveList` from profile `FalloutCustom.ini` first, then profile `Fallout.ini`. Only if neither contains the key does it read `game/Fallout_default.ini` and take that key there, or use an empty list if the successfully read and validated fallback lacks it. An explicitly empty profile value counts as present and does not trigger fallback. A missing or unreadable fallback file currently fails this path.

This is not a replacement of the entire profile INI and not a recopy of all defaults. The selected archive list feeds derived INI generation, which manages the invalidation archive alongside the existing list. This fallback is current behavior, not a new proposal or a decision to retain it. No change was requested or implemented in this explanation.

### Decision: use a shared temporary FalloutCustom.ini override mechanism

The user wants to replace the archive-list fallback approach with overrides in `FalloutCustom.ini`. If the file exists, use it as the basis for the execution override. If it does not exist, create a temporary execution file containing the required settings. Apply the same mechanism to save-folder routing and share the code for these operations rather than implementing separate special cases. Implementation remains paused.

Desired behavior: preserve unrelated user settings, add or update managed archive-invalidation and save-routing settings in the temporary execution copy, and map that copy for the launched process. Do not infer permission to create or modify the canonical profile file when it was originally absent. Repeated preparation must not accumulate duplicate keys or archive entries.

Open semantic issue: appending an archive filename to an existing `sArchiveList` value is different from adding an `sArchiveList` assignment to an INI. An assignment in FalloutCustom.ini may replace the effective list rather than append to an inherited list. Verify game INI precedence and whether an invalidation-only assignment preserves vanilla archive loading before selecting exact emitted values. Also verify that the relevant save-routing keys are honored in FalloutCustom.ini for supported launch targets, including any GECK-specific differences. These verification needs do not reinstate the rejected game-default fallback as an agreed design.

This decision revises the earlier canonical-routing-only-in-Fallout.ini model for execution overrides; exact canonical validation, export behavior and post-run preservation rules need reconciliation before implementation. The intended common code accepts managed settings and handles optional source INI, temporary materialization and runtime mapping. No new implementation or public API design has been authorized.

### Why execution collects file size and modification time

The user asked why these fields are recorded. In execution, winning-file sizes and modification times participate in `PreparedExecution` equality between initial preparation and the fresh prelaunch preparation. They detect some file edits without reading and hashing file contents. This is not a content-integrity guarantee: same-size, same-mtime edits can escape it.

Modification time also orders visible plugins absent from the explicit plugin order, with a path tie-breaker, in the advisory profile projection. This does not enforce runtime load order. File size is not needed to decide which provider wins a path or to configure the directory overlays. Current inventory collects details for losing and disabled files too, although only winning details enter the prepared comparison.

Potential narrower design: if the execution file-change comparison is removed or reduced, reconsider size collection and obtain timestamps only for plugins that need the ordering fallback. This is a discussion candidate, not a user decision to remove revalidation. Export's separate size/count and timestamp requirements must not be changed implicitly. No implementation authorized.

### Decision: remove prelaunch change detection and timestamp plugin ordering

The user explicitly requested dropping detection of changes before launch entirely. Future implementation should remove the fresh prelaunch environment comparison and other checks whose sole purpose is detecting changes since preparation, rather than replacing them with another fingerprint or change detector. This supersedes the earlier requirement to retain prelaunch freshness checks. Initial preparation/validation is a separate concern; post-run validation and concurrent-edit protection during INI preservation were not removed by this decision.

The user also requested that unlisted plugins remain in the order they were read, without modification-time sorting. This removes both stated execution-specific reasons for collecting file lengths and modification times. Remove that execution bookkeeping where no other execution consumer requires it; do not implicitly remove export metadata requirements or file metadata needed for remaining type/safety validation.

Ordering clarification from the user: no explicit discovery-order preservation is needed. Unlisted plugins should use whatever iteration order the existing dictionary or array supplies, without an additional timestamp sort or ordering bookkeeping. HashMap iteration may vary between runs; stable ordering is not required for unlisted plugins. This supersedes the earlier suggestion to preserve discovery order explicitly. Explicitly listed plugin order remains unchanged.

These are recorded design decisions only. No implementation until the user explicitly authorizes it.

### Why ancestry is validated

The user asked why ancestry checks exist. Provider traversal uses handle identity and parent walks to check that an opened directory is still below its expected provider root. A path spelling alone does not prove containment, especially if a directory is moved or replaced during traversal. No-follow opens reject symlink/reparse traversal, while ancestry checks address actual directory relationships. These checks do not make the complete scan atomic and cannot prevent all later changes.

A separate game/environment ancestry check rejects overlapping roots, so neither contains the other. This is distinct from repeated ancestry validation inside the file inventory.

Cost: repeated parent-directory opens and identity queries, increasing with depth. Whether this defensive behavior is needed for a local trusted-tree exec path is a policy question. Export/install write boundaries need separate consideration; do not infer their protections should be removed. No decision to remove ancestry validation has been made, and no implementation is authorized.

### Decision: remove execution ancestry validation

Following the explanation of provider ancestry checks and game/environment separation checks, the user said to drop this validation. Record removal of both described ancestry checks from the exec path: repeated provider-directory parent/identity walks and the game/environment containment validation. This removes those execution-time containment guarantees. Initial type checks, no-follow/reparse checks and other validation are separate and are not implicitly removed.

This is an exec discussion decision, not permission to remove shared protections globally. Export/install write-boundary checks and checks in other commands must remain outside this change unless separately approved. Implementation remains paused until explicit authorization.

### Decision: create default mod metadata when requested and absent

The user requested that asking for a mod's metadata create the default metadata file if it does not exist. The location is that mod's root directory, `<mod-root>/meta.toml`, not the environment root. The current minimal default is `schema_version = 1` with no tombstones.

This revises the earlier absent-metadata behavior: instead of only treating absence as empty in memory, the requested mod-metadata access should materialize that default on disk. Preserve existing metadata unchanged; invalid or unreadable existing metadata is not absence and must not be overwritten. Creation should not overwrite a file created concurrently. The request does not imply bulk creation before metadata is requested, nor creation inside game Data or other provider types.

This introduces a write during metadata access, which previously could be read-only. Before implementing shared consumers, reconcile this with explicit dry-run/read-only contracts rather than silently changing those commands. Exact command coverage remains to be resolved. This is a recorded behavior decision only; implementation is still paused until explicitly authorized.

### Why inventory opens every file, in simple terms

The user asked for an ELI5 explanation. Directory enumeration is like reading a list of names; opening a file obtains a handle to the actual object so the code can check that it is a regular file, not a reparse point or hard link, rather than relying only on previously read directory information. Opening does not mean reading its contents. The current implementation also uses the handle to collect size/mtime, whose execution bookkeeping has now been selected for removal.

Opening every data file is an implementation of the current defensive validation policy, not inherently required to select winners by path and provider priority or to ask usvfs to map directories. With prelaunch change detection removed, remaining reasons include open-time type/link validation and early access-error detection. Metadata and INI contents still need actual reads. No decision to remove all per-file opens or remaining link/type checks has yet been made. Only documentation updated.

### Decision: remove execution inventory link rejection and validation-only file opens

After discussing shortcuts and filesystem links, the user requested removal of that validation. For the exec provider inventory, do not reject files merely because they are hard links or symbolic links/reparse points, and remove per-data-file opens whose only purpose is the rejected link/type validation. This is not a request to stop reading actual metadata or INI contents when needed. Ordinary Windows `.lnk` files are regular files, not filesystem redirections; the earlier shortcut analogy was too broad.

Directory-link traversal policy still needs an explicit choice. Removing link rejection must not silently imply unbounded recursive traversal through junctions or symlink cycles. Decide whether to follow or skip directory links and how to bound traversal before implementation. Native usvfs behavior with linked providers also needs verification, not an assumption of compatibility. Existing traversal limits are not removed by this decision.

This decision applies to the exec inventory discussion, not wholesale removal of safe filesystem checks from export/install writes, metadata creation or INI preservation. Implementation remains paused until the user explicitly says to implement.

### Decision: build winners and the mod-folder set in the same traversal

The user described the desired algorithm explicitly:

- Use a dictionary keyed by the file's virtual Data-relative path, for example `Meshes/CoolMesh.idkextension`.
- Store the winning mod's priority and folder name as the value, conceptually `{ priority: 0, name: "CBBE" }`. Overwrite files use the name `overwrite`.
- While recursively traversing children of the mods directory, retain the owning top-level mod folder and look up its enabled state and priority from the parsed modlist.
- For an eligible file, insert it if the dictionary has no entry. If another provider already owns that path, compare priorities and replace only when the incoming provider wins. The existing priority convention must define winning direction; directory enumeration order must not decide winners.
- In the same traversal, record each discovered top-level mod folder in a HashSet, including empty and disabled folders, and compare it with a HashSet of the full modlist. Descendant directory names are not mod names. Keep duplicate/case-collision rejection before converting the modlist to a set.
- Normalize dictionary keys using the existing case-insensitive Data-path policy so spelling differences do not bypass conflict detection. Preserve original path spelling if needed downstream. Data-relative means the virtual path; mod roots already supply Data contents, so do not require another physical Data directory inside every mod.
- Overwrite is a sibling of mods, not a child; traverse it separately through the same collection routine and give it highest precedence. Game Data is also outside mods and remains the base provider unless separately changed.
- Disabled mods participate in the folder-set check but do not contribute winners, retaining the prior enabled-state decision.

This replaces the conceptual collect-all-inventories-then-build-winners approach with winner updates during traversal. Do not add another filesystem traversal solely for folder matching or conflict detection.

The dictionary handles ordinary file conflicts. Existing tombstones and file/directory collision rules still need an explicit integration plan; they were not removed by the user. In particular, recursive directory traversal order cannot be assumed to match mod priority, so suppression must respect priority independently of visit order. Do not silently drop these semantics while implementing the requested dictionary. No implementation authorized yet.

### Decision: filter enabled mods before recursive traversal

Before implementation started, the user added that the parsed modlist must supply an enabled-mod HashSet before the traversal loop. Only mods in that set contribute files. At each top-level mod directory, record its name in the discovered-folder set for comparison against the full modlist, then skip its descendant traversal if it is not enabled. This avoids file work for disabled mods, rather than scanning their files and discarding the results afterward.

This supersedes earlier discussion retaining full disabled-provider validation. Empty and disabled top-level folders still participate in exact full-modlist matching. The enabled set does not replace the priority lookup. Do not read or create metadata for skipped disabled mods merely as part of exec inventory; the create-on-request rule still applies when their metadata is actually requested elsewhere. No Rust implementation edits had started when this clarification arrived.

### What read_bounded does

The user asked about `read_bounded`. In environment `safe_fs.rs`, it opens the named file through `SafeDir::open_regular`, reads its contents into a byte vector in 64 KiB chunks, checks cancellation around operations, and errors if the accumulated bytes would exceed the caller's limit. It returns the full contents on success, not a truncated prefix. The limit bounds accepted content/memory growth; it is not a timeout or an async operation. One extra chunk can be read before an over-limit error is detected.

This helper is used when actual contents are needed, such as INIs or metadata, unlike validation-only opens of ordinary assets. Its current safe-open checks and the byte limit are separate responsibilities. No decision to remove the size bound has been made.

### Decision: JIP execution overrides, defer GECK, remove exec read size limits

The user chose to require JIP for the FalloutCustom.ini approach and explicitly deferred all GECK-specific work. Implement the game override path with that documented prerequisite; do not introduce GECK detection, new GECK overrides or a GECK compatibility project. Existing unrelated GECK behavior is not a reason to expand scope.

The user also requested simplifying `read_bounded` to use provided read functions without maximum-size validation. Within the frozen exec scope, use standard/library whole-file read APIs for required configuration contents and remove exec maximum-size rejection and custom chunk-reading code. Do not remove shared export/install limits implicitly; separate exec reads where necessary. Ordinary I/O errors still propagate. Whole-file reads can allocate memory proportional to input size and cancellation cannot interrupt a single synchronous read; document these tradeoffs.

JIP resolves Custom.ini support, but does not change its replacement semantics for sArchiveList. Preserve the existing effective archive list from canonical profile Custom.ini/Fallout.ini when available, append invalidation without duplication, and do not restore the rejected game-default fallback. If neither profile contains the list, the archive baseline policy still needs an explicit resolution before silently emitting an invalidation-only value. Implementation of independent approved exec changes may proceed while resolving that remaining detail.

### What validate_bsa_file and validate_exact_entries do

The user asked about these helpers. `validate_bsa_file` opens only the generated `cache/Fallout - Invalidation.bsa` and compares its entire content with `empty_bsa_bytes()`. It reads up to expected length plus one byte so extra bytes also fail. Missing, unreadable, truncated or different contents fail. This is not a scan or validation of all vanilla/mod BSA archives; it checks the small generated invalidation artifact.

`validate_exact_entries` enumerates the immediate children of a directory, rejects names outside a supplied allowlist or repeated exact names, caps observed entries at the allowlist length, and requests metadata for each entry. Despite its name, it does not check that all allowed names are present: it does not require the remaining-name set to be empty. Other open/read checks establish required entries. It is nonrecursive and does not read file contents or enforce file type itself. An empty allowlist requires an empty directory. No decision to remove these checks was made in this question.

### Runtime validation versus unit tests for cache and directory entries

The user asked whether the BSA/exact-entry checks are needed at runtime or could just be unit tests. A unit test can verify that the archive generator emits the correct BSA bytes and initialization creates the intended layout. It cannot prove that files on a user's disk remain unchanged later.

For the trusted local exec policy being chosen, removing repeated generated-BSA byte comparison and unrelated-entry allowlist rejection is a reasonable option. Required files would still need to be read or mapped, and actual I/O failures would still be reported. The tradeoff is giving up early corruption/foreign-entry detection; runtime failure may happen later. Tests would cover generation and initialization, not substitute for on-disk validation. This question alone is not treated as authorization to remove these additional checks. Export/install validation remains separate.

### Approved scope addition: remove exec BSA-content and entry-allowlist checks

The user explicitly authorized the preceding recommendation. Remove generated invalidation BSA byte-for-byte validation and exact-entry allowlist rejection from the lightweight exec preparation path, including its post-run use. Unrelated directory entries should not make exec fail just because their names are outside a fixed allowlist. Keep unit tests for the BSA generator and initialization layout. Keep ordinary required-file reads/mapping failures and the independent pending-operation/owned-temp lifecycle rules; do not conflate those with unrelated-entry rejection.

Export/install strict paths and their validation remain unchanged. Add regression evidence that lightweight preparation tolerates unrelated root/cache/profile entries and does not perform the generated BSA content comparison while strict behavior remains intact. Existing full-check results precede this addition and need rerunning after implementation. Missing profile archive-list policy remains unresolved.

### Stored Steam identifiers

The user asked whether Steam app/build IDs are stored. Yes: environment-root `mods.toml` persists `steam_app_id` and `observed_build_id` alongside `game_dir` and `schema_version`. Initialization serializes these from the game binding. Existing schema still requires valid stored identifiers; removing exec's live Steam manifest/build comparison does not remove these fields or migrate manifests. No request to remove storage has yet been made.

### Approved scope addition: remove stored Steam app/build identifiers

The user explicitly requested removing `steam_app_id` and `observed_build_id` and stated that no migration work is needed before 1.0.0. This expands the earlier frozen manifest/domain compatibility scope: remove those persisted fields and the associated required binding identifiers/checks rather than preserving them for compatibility. Do not add migration machinery, schema-conversion commands or automatic rewrites of the user's environments. Pre-1.0 manifest compatibility may break; document the resulting accepted format and existing-file behavior.

Update affected domain binding, manifest serialization/parsing, settings and initialization consumers/tests as necessary. Steam-specific discovery may retain its own app identifier where needed to locate the supported game; removing stored environment identifiers does not imply removing Steam discovery itself. Remove build-ID requirements/comparisons made obsolete by this decision, but preserve unrelated export/install filesystem protections. Report exact behavior for old manifests rather than claiming compatibility. Missing archive-list policy remains unresolved.

### Config crate usage for the manifest

The user asked whether the manifest uses the config crate. Settings loading uses `config::Config::builder` with the manifest TOML source and environment overrides in `infrastructure/settings/src/config_source.rs`, then deserializes. Manifest creation/writing uses `toml::to_string_pretty` and filesystem publication, not the config crate. Environment-side manifest validation separately uses `toml::from_str`; this is not a single shared parsing route today. No consolidation was requested by this question.

### Manifest parsing distinction and identifier-removal impact

Environment-side TOML parsing validates stored file contents, while settings Config loading merges overrides and also produces manifest-only values. These are separate semantic views, but do not inherently require duplicated schema/parser code. Consolidating them is a possible future change, not authorized by the explanation alone.

The approved Steam identifier removal affects domain GameBinding, environment/settings manifest schemas, settings outputs and obsolete build warnings, Steam validation, CLI formatting and fixtures across consumers. Existing deny_unknown_fields means manifests with the removed fields will be rejected; no migration or real-environment rewrite is planned. This is the accepted pre-1.0 format break, not a claim of backward compatibility.

### Application use cases and loaded settings

The user asked whether application use cases consume settings loaded through Config or whether only infrastructure does. Application never consumes `config::Config` directly. The application-owned `LoadSettings` port returns application `ResolvedSettings`; `list_settings` reads its settings records and `get_setting` selects a requested record. Binding-related use cases also consume typed bindings from ports. Thus typed settings results do cross into application use cases, while parsing/merging and the Config dependency remain in infrastructure. Native exec loads its binding inside the infrastructure adapter; its application use case forwards through RunManagedProgram instead of loading Config itself. No architecture change requested by this question.

### Proposed architecture: load settings at presentation entry, pass down

The user proposed loading settings only in the presentation layer and passing them down the call tree. Interpret the desired lifetime as one command-scoped load at the presentation/composition entry point, followed by explicitly supplied typed settings or the narrow values each use case/adapter needs. Downstream adapters would not independently reload settings or re-read the manifest to rediscover configuration.

Parsing and Config builder mechanics can remain implemented in infrastructure/settings and be invoked by the composition boundary; application/domain code should not depend on config::Config or CLI types. This distinguishes where loading is initiated from where the parsing implementation belongs. Do not use a global settings singleton or pass an unrelated configuration bag everywhere.

This is a proposed broader architecture change, not yet a frozen implementation addition. Clarify approval before changing application ports/composition across commands. Settings inspection/update commands still need to distinguish stored values from effective overrides; initialization must work before a manifest exists. Export's deliberate source-byte capture/revalidation is not an independent settings reload and must not be silently removed. Writer was told to assess and report the impact, not implement this proposal yet.

### Approved scope addition: presentation-initiated settings load

The user answered yes to including the broader load-once architecture in this implementation. This supersedes the earlier frozen application-port/composition interfaces where changes are necessary to pass settings down. Presentation/composition initiates one command-scoped settings load through infrastructure/settings; Config builder/parsing implementation stays there. Pass typed resolved settings or narrow required values explicitly through application use cases and infrastructure ports. Downstream adapters must not independently reload configuration or reparse the manifest to rediscover settings.

Cover existing CLI command paths that consume settings, including exec, settings inspection/update, install and export. Initialization must continue to work without an existing manifest; help/version must not acquire a manifest prerequisite. Preserve stored/effective provenance and override semantics. Keep export source capture/revalidation and settings-write concurrent-change checks as explicit integrity operations, not configuration reloads. No singleton, global configuration bag, new dependency, migration or real-environment mutation. Expected additional files include CLI runner/composition, application use cases/ports, infrastructure dependencies/settings and downstream adapter signatures/tests. Verify one-load behavior and supplied-value propagation at public seams, plus existing command regressions. Earlier successful checks predate this expansion and must be rerun.

### Approved archive-list fallback: embedded verified game defaults

The user rejected failure when neither profile INI contains sArchiveList, then approved an embedded known-default archive list. Final exec precedence: present FalloutCustom.ini list, otherwise present Fallout.ini list, otherwise verified embedded Fallout New Vegas default list in its verified order. Explicit empty values count as present. Append the managed invalidation archive once using existing transformation semantics. Do not enumerate all BSAs as a replacement list and do not read Fallout_default.ini on each launch. Verification of the constant may consult authoritative default data during development; this is not a runtime fallback.

This supersedes the temporary profile_archive_list_missing refusal and the unresolved archive-policy notes in earlier reports. Implement this narrowly in exec fallback plus regression tests; preserve export defaults behavior, JIP prerequisite, deferred GECK work and safe INI preservation. Verify exact names/order before coding rather than guessing. The requested Windows installation remains pending the enforced style-gate override and Windows build/package validation; this approval is not a gate override.

### Verified default archive list

Read-only lookup of the installed Steam copy `C:\Games\Steam\steamapps\common\Fallout New Vegas\Fallout_default.ini` (SHA-256 `A701C3A96AF26F83BA6399B4A579AF59FA075868949519F4DEC45BF47BF7F95D`), line 706 under `[Archive]`:

`SArchiveList=Fallout - Textures.bsa, Fallout - Textures2.bsa, Fallout - Meshes.bsa, Fallout - Voices1.bsa, Fallout - Sound.bsa,  Fallout - Misc.bsa`

The original value contains a double space before `Fallout - Misc.bsa`. This is the verified source for the embedded fallback: six archives in this order. It comes from the user's English Steam installation; other localized editions were not checked. No game was launched.

### Proposed redesign: application composes exec through ports

The user identified a layering problem: the application `execute_program` use case forwards straight to one `RunManagedProgram` port, and `dependencies/src/execution_adapter/native.rs::execute` composes every infrastructure step itself. The application layer should compose infrastructure through ports. User example order: resolve conflicts, set up the temporary profile, create the VFS, run the managed program. Names and step list are illustrative, not fixed. Work happens in this worktree/branch after `211ad59`.

Current steps inside `native.rs::execute`, in order:
1. Resolve the launch target from caller PATH/cwd and arguments.
2. Capture inherited standard streams.
3. `prepare_launch`: modlist, providers, streaming winners (conflict resolution), profile files.
4. Validate the `DataMod` output target (exists, enabled). This is a business rule.
5. Resolve platform profile directories and `build_profile_configuration` (plugin projection, profile mappings, warnings); map warnings to `ExecutionWarning`.
6. `derive_execution_inis`: temporary profile INIs, retained on failure.
7. Build `ViewConfiguration` and `VirtualGameView::configure` (usvfs).
8. Launch inside the view; map native errors.
9. Report `ExecutionPrepared`; supervise until exit; check the Job drained.
10. Preserve INIs, finish private streams, post-run profile check (warning only), map forced cancellation.

Proposed application-owned ports (names provisional):
- `ResolveLaunchTarget`
- `PrepareLaunchPlan` (step 3; the conflict resolution)
- `ProjectExecutionProfile` (step 5)
- `StageExecutionProfile` (step 6), returning a staged-profile handle
- `CreateVirtualFileSystem` (step 7), returning a VFS handle
- `LaunchProgram` (step 8), returning a running-program handle
- `SuperviseProgram` (step 9)
- `PreserveExecutionProfile` (step 10)
- `CheckProfileState` (post-run warning)

The application use case then owns the order, the output-target rule, warning mapping, progress events, cancellation checks, and retention of the staged profile on failure.

Key constraint: the usvfs session is thread-affine. `usvfs/mod.rs` marks it `PhantomData<Rc<()>>`, so it is `!Send`. Today `execution_adapter.rs` runs the whole execution on one `spawn_blocking` thread with a current-thread runtime. Application `PortFuture` requires `Send`, so a `!Send` VFS or process handle cannot pass through the existing port type. Options:
- A. Run the whole exec use case on that dedicated thread. Composition moves the existing `spawn_blocking` plus current-thread runtime up from the adapter. Exec ports use a non-`Send` local future type, and VFS/process handles stay `!Send`, so the compiler enforces thread affinity. This gives the full step-by-step composition.
- B. Keep one port for VFS create, launch, and supervise (for example `RunInVirtualFileSystem`). Every other step moves to application ports. Smaller change, less granular.

Decision: the user chose A. Implement it on this branch after `211ad59` as a behavior-preserving refactor. Winner resolution stays behind the launch-plan port for now; moving it into a domain accumulator remains a separate open question.

Port shape: the user asked to keep exec ports the same shape as the other application ports (`Arc<dyn Fn + Send + Sync>` returning the Send `PortFuture`) if feasible, rather than `Rc` with a local future. Handles visible to the application become Send opaque tokens. The !Send native state stays in infrastructure on the dedicated execution thread. Thread affinity is then checked at run time rather than by the compiler.

usvfs thread requirement: the pinned usvfs-rs source (c23705c) keeps controller state process-global. The controller exports use that global state without taking a lock (the context locks appear only in the injected hooks), and the controller path has no thread-local state or creating-thread requirement; DllMain does nothing on thread attach. The standards review corrected an earlier claim here that each call takes its own mutex. The requirement is therefore "no concurrent access", not "same thread". Decision: make `VirtualGameView` Send but not Sync. This needs a documented `unsafe impl Send` in the usvfs FFI module, relying on the existing one-session-per-process guard. The token registry is dropped, and exec ports keep the normal Arc/Send shape. The whole use case still runs on one dedicated blocking thread. Validation: on the Windows machine at `c5e040f`, the cross-thread test passed for i686 and x86_64. It ran three cycles, each creating the session on thread A, launching a hooked child that read the mapped file on thread B, and closing on thread C. A same-thread control failed identically until the fixture mapping was made recursive, which showed the first failure was a fixture bug, not a threading one. Windows Clippy and 474 workspace tests also passed.

Still open: whether winner resolution itself should become domain logic. Infrastructure would stream entries into a domain accumulator, which keeps the approved single-traversal behavior. Status: option A implemented in `a449b66` (fixture fix `c5e040f`). The independent spec review found no user-visible behavior difference. The standards review found three issues, repaired in a follow-up commit: the incorrect locking claim in the SAFETY proof, the undocumented fresh cancellation token for the post-run check, and a history-only comment.

### Bug: vanilla multi-line INI text rejected

On the user's Windows profile, exec failed before launch with environment_invalid. `profile_ini_valid` rejects lines that are neither comments, section headers, nor `key=value`. The vanilla Fallout.ini and FalloutPrefs.ini contain the game's multi-line `SMasterMismatchWarning` text: two continuation lines without `=` (Fallout.ini lines 696–697, FalloutPrefs.ini lines 764–765). The game's reader ignores such lines. The user approved fixing this on this branch: accept non-assignment text lines and preserve them unchanged. The fix is committed separately, before the port-composition refactor continues.

### Crash diagnosis and default working directory

At 23:44 the game crashed before the menu: an access violation at `FalloutNV.exe+0x6087E5`. The Windows Error Reporting module list shows `usvfs_x86.dll` was injected, but NVSE (`nvse_1_4.dll`) and none of the 52 NVSE plugins loaded, and the NVSE logs were not updated. Masters were all present and correctly ordered, so load order was not the cause. The user identified the missing working directory: without `--cwd`, the child runs in the caller's directory, and New Vegas resolves `Data\` and its loaders relative to the working directory. A 23:47 launch through `nvse_loader.exe` loaded NVSE and its plugins through the VFS, and the game kept running.

Approved change: when `mods exec` has no `--cwd`, default the child's working directory to the bound game install directory. Program lookup keeps its current rules (caller directory plus PATH). An explicit `--cwd` is unchanged. This is committed separately, after the port-composition refactor.

### Modlist priority order: match MO2

The user copied `modlist.txt` from a working MO2 instance and saw Vanilla UI Plus and UIO warn about conflicts. MO2 writes `modlist.txt` from highest to lowest priority: the first line is the highest-priority mod. mods read it from lowest to highest, as `CONTEXT.md` defines. The copied file starts with "High Priority Trees/Core" and LOD outputs and ends with YUPTTW and Tale of Two Wastelands. mods therefore inverted every conflict: base mods and late UI or framework mods overrode Vanilla UI Plus. Plugin masters and load order were unaffected.

Decision (user chose the product change): read `modlist.txt` in MO2 order. The first listed mod has the highest Mod Priority and the last has the lowest. Overwrite stays implicitly highest and game Data lowest. This applies to every reader (exec inventory, snapshot/export, conflict scan and explain, and installation). New installs must still receive the highest priority, so the writer inserts new entries at the top, after any leading comment lines, instead of appending. Update `CONTEXT.md` and the CLI skill references. No migration: environments written with the old order must be reversed by hand. This is committed separately, after the port-composition refactor and the working-directory default.

### Queued: close the VFS by drop

The user approved removing the explicit `CloseVirtualFileSystem` port, relying on `VirtualGameView`'s existing `Drop`. The only behavior change: a teardown failure on the cancel-after-VFS-creation path is no longer reported as `vfs_failed` (phase `cleanup`); the user sees `operation_cancelled`. This is done after the refactor build (`c5e040f`) is installed, before the working-directory default and MO2 modlist-order changes.

### Decision: drop SafeDir, use tokio::fs everywhere

The user decided to drop `SafeDir` (cap-std) and use `tokio::fs` throughout, including the places that currently use `std::fs`. The rationale: the planned git rollback ticket will cover recovery from bad writes, so the handle-relative, no-follow, reparse-point and hard-link protections are no longer needed, and the application use cases are already async.

Defaults unless the user says otherwise:
- Keep only `create_new` opens for no-overwrite creation (`meta.toml`).
- Drop the compare-before-publish checks for settings and INIs, as the user decided. Concurrent edits made while a command or the game runs may be overwritten; the planned git rollback ticket covers recovery.
- Drop durable publication (write, flush, then rename) for settings, INIs and exports, as the user decided. Files are written directly.
- Drop the no-follow and link checks, handle-relative directory walking, reparse/hard-link rejection, byte caps, and ancestry checks, except export's rule that the output must not be inside the environment, which prevents recursive copying.
- Enable tokio's `fs` feature.
- Measure the exec inventory walk before and after. `tokio::fs` sends every call through the blocking pool, and the user's setup has tens of thousands of files, so the walk may get slower.
- Do this on `perf/streamline-exec-preparation` itself, as its own commit, after the current build is installed.

### Queued: export shares ports with exec

The user reports that exec feels much better, and asked to look next at export sharing ports with exec. This comes after the tokio::fs change. Starting point for the design discussion: export still uses the strict `prepare_execution` / `PreparedExecution` path, while exec uses `prepare_launch` / `PreparedLaunch` behind the `PrepareLaunchPlan` and `ProjectExecutionProfile` ports. Removing SafeDir takes away most of the integrity differences between the two paths. User direction: export and exec should behave the same up to their final step. Both run the same shared preparation through the same ports: binding, the settings load, `PrepareLaunchPlan` (winner resolution), and `ProjectExecutionProfile` (INIs, plugins, archive list). After that the two commands split:
- exec stages the profile, creates the VFS, launches the program, supervises it, and preserves the INIs.
- export copies every winner and every projected profile file into the output directory.

Consequence: export drops its separate strict `prepare_execution` / `PreparedExecution` path and its extra integrity checks, and inherits exec's tolerances.

Export-only concerns that stay after the split: the output must not be inside the environment, `--include-saves`, and `--dry-run`.

### Design: export shares preparation with exec

Status: proposal, waiting for approval. No code changed yet.

#### Goal

Both commands run the same preparation through the same application ports. They differ only at the end:

- exec: stage the profile INIs, create the VFS, launch, supervise, and preserve the INIs.
- export: copy every winner and every profile file into the output directory.

An export folder then contains exactly what the game sees under exec.

#### Where they differ today

| Area | exec | export |
| --- | --- | --- |
| Use-case shape | 10 ports composed by `execute_program` | one `PrepareExport` port that does everything in infrastructure, then `PublishExport` |
| Preparation | `prepare_launch`: tolerant, refuses a non-empty `temp`, creates missing `meta.toml` for enabled mods | `prepare_execution`: strict; records consumed bytes and file lengths; rejects extra entries and a corrupt invalidation BSA |
| Archive-list fallback | embedded English Steam default | reads `Fallout_default.ini` at runtime |
| Plugin projection | projected, with advisory warnings | `plugins.txt` and `loadorder.txt` copied verbatim; no warnings |
| INI derivation | `ProfileIniPurpose::Execution`: only `FalloutCustom.ini` rewritten; saves routed to `__mods_saves\`; invalidation BSA appended last | `ProfileIniPurpose::Export`: Fallout and Custom rewritten; `SLocalSavePath=Saves\`; invalidation BSA first |
| Platform | ports are wired only in the Windows `native.rs`; profile projection (`build_profile_configuration`) is Windows-only | runs on every platform |

#### Proposed shape

Shared application ports, in a new capability module `application::ports::preparation`. The names become neutral because two commands use them:

- `PrepareEnvironmentPlan(CancellationToken) -> EnvironmentPlan`: today's `PrepareLaunchPlan` / `LaunchPlan`, backed by `prepare_launch`.
- `ProjectProfile(&EnvironmentPlan) -> ProfileProjection { profile, warnings }`: today's `ProjectExecutionProfile` without the Windows Documents and LocalAppData directories. Those move into exec's `CreateVirtualFileSystem`, the only step that needs them.

A shared application helper, `application::preparation::prepare_environment`, calls both ports and maps `ProfileWarning` to the user-facing warnings. Both use cases call it, as allowed by the "repeated callers justify a capability-level helper" rule.

exec after the change: resolve target, `prepare_environment`, output-target check, stage the profile, create the VFS, launch, supervise, preserve, finish output, post-run check. The steps and behavior stay the same; only the first two ports are renamed and shared.

export after the change:

1. `ValidateExportDestination(output)` (export only): the path is absolute, does not exist yet, and its parent is not inside the environment.
2. `prepare_environment` (shared).
3. `ListExportFiles(&EnvironmentPlan, &ProfileProjection, include_saves)` (export only): every winner except game Data, the profile files (INIs derived with `ProfileIniPurpose::Export`, plus `plugins.txt`, `loadorder.txt`, and `modlist.txt`), the invalidation BSA, and saves when `--include-saves` is given. Each file carries its size.
4. `plan_inventory` (application, unchanged): folds directory spelling case and checks for structural conflicts.
5. `--dry-run` stops here.
6. `WriteExport(files, output)` (export only): writes each file directly and sets the source mtime.

The `ExportEnvironmentOutput` gains `warnings`, the same list that exec reports.

Wiring: the shared ports move out of `native.rs` into a platform-neutral adapter in `infrastructure-dependencies`. The profile-projection module in `infrastructure-execution` stops being Windows-only. `native.rs` keeps the VFS, launch, supervision, and output steps.

#### Removed

- `PrepareExport`, `PreparedExport`, `PublishExport`, and the `ExportSnapshot` capture in `environment/src/export.rs`.
- `prepare_execution` and `PreparedExecution` (their only users are export and their own tests).
- The runtime read of `Fallout_default.ini` (`ProfileIniInputs` non-execution mode and its `fallback` field). Export uses the embedded default list, like exec.

#### Kept different on purpose

INI derivation keeps two purposes. The exported folder is a standalone layout, so it keeps `Saves\` and rewrites `Fallout.ini`. exec overlays a VFS and relies on JIP LN NVSE reading `FalloutCustom.ini`. This is the "last bit" of each command, not shared preparation.

#### User-visible changes for export

1. Export accepts whatever exec accepts: extra entries, and a generated BSA copied as-is.
2. Export creates missing `meta.toml` for enabled mods, because `prepare_launch` does. Export stops being strictly read-only.
3. When neither `FalloutCustom.ini` nor `Fallout.ini` sets `sArchiveList`, export uses the embedded default list instead of reading `Fallout_default.ini`.
4. Export prints the same plugin warnings as exec.
5. Export on macOS and Linux still works, because the projection becomes platform-neutral.

#### Decisions so far

- Export may create missing `meta.toml` for enabled mods (question 1: yes).
- Shared ports are neutral, composable, and reusable (question 3). CODING_STYLE now has the "Reusable capability ports" rule (main `4cdc383`), so the neutral names above are required.
- Export prints the same plugin warnings as exec (question 2).
- Export stages its files in `temp` and then copies them from `temp` into the output directory (question 4). Export reuses the shared staging step instead of deriving INIs in its own path. The stage port takes the profile purpose, so exported INIs keep the standalone form: `Fallout.ini` rewritten and `SLocalSavePath=Saves\\`. The writer checks why the invalidation BSA order differs between the two purposes, and makes it the same unless there is a reason not to.

#### Questions for the user

1. Is export allowed to create missing `meta.toml` (change 2)? The alternative is a read-only flag on `PrepareEnvironmentPlan`, which makes the two commands differ again.
2. Should export print the plugin warnings (change 4)?
3. Should the shared ports get the neutral names `PrepareEnvironmentPlan`, `EnvironmentPlan`, and `ProjectProfile`, or keep the exec names?
4. Is it right to keep the two INI derivation purposes separate?

#### Plan

On this branch, as separate commits:

1. Make the shared ports platform-neutral, with the renames, and move them out of `native.rs`. exec behavior does not change.
2. Rebuild export on the shared ports and delete the strict path.

Then an independent spec and standards review, Windows validation, and install.

### Decision: enforce load order through plugin and BSA times

The user approved one shared step that both commands run:

- Plugins (`.esm`, `.esp`) get modification times in `loadorder.txt` order, starting at 2000-01-01 00:00 UTC, one minute per position. Plugins missing from `loadorder.txt` come after the listed ones, in their current modification-time order.
- A BSA that loads through a plugin name (its name starts with the plugin's name, case-insensitive) gets the same time as its plugin. BSAs named in the derived `sArchiveList`, and any other BSA without a plugin, come before all plugins.
- exec runs the step before launch. It sets times on the real winning files, including files in the Game Installation's Data folder (approved), the Data Mods, and Overwrite. usvfs cannot present fake times.
- export runs the step after writing, on the copies in the output folder only. Export stops setting times on every other file, so fix G's per-file time copy goes away. The attributes-only file open stays for this step, because plugins and BSAs can be read-only.
- The `load_order_not_enforced` warning no longer applies once the order is enforced.

### Decisions on the tokio::fs follow-ups

- Walk timing accepted: the exec inventory walk over 20,000 files went from a median of 21.0 ms to 27.6 ms with tokio::fs.
- Walk resource limits stay removed: mod count, entry budget, and depth. A link cycle stops only when the OS reports an error.
- Init and install drop staging and write directly into their destination directories. If nothing stages any more, the pending-operation refusal has nothing to detect; the writer reports whether any of it remains.
- `--cwd` gets help text describing the game-directory default.
- Gate dispositions will match on file content hash as well as file and rule.

## Execution cost map and optimization candidates

The detailed read-only trace below explains the installed revision. Source-derived costs are hypotheses, not measured Windows bottlenecks.

Before launch there are two full environment preparations, with native usvfs recursive mapping between them. A successful run performs another full preparation after managed processes drain. Single pass means once per provider within a preparation, not once per entire command.

Candidates discussed, none authorized for implementation:

- Reuse the metadata returned by safe file opening instead of requesting handle metadata a second time for length and modification time.
- Index provider identities once instead of linear provider lookup for every winner in preparation and configuration validation.
- Reuse manifest, modlist and profile bytes within one preparation; keep independent freshness checks fresh.
- Measure ancestry checks and consider carrying validated traversal evidence without weakening path-race protection.
- Measure native recursive mapping, which rediscovers enabled provider files instead of consuming the analytical winner map. Reusing inventories would require preserving directory visibility, writes, runtime mapping and suppression semantics.
- Measure full native bundle hashing and process injection separately.
- Consider a specialized post-run validator rather than constructing execution output only to discard it.
- Consider narrower prelaunch change detection only with an explicit guarantee; reusing the initial inventory would remove the current freshness check.
- Skipping disabled-provider validation would change policy and is not part of the approved earlier optimization.
- Add stage durations and progress to distinguish scanning, mapping, injection, game lifetime and post-run work. Normal CLI execution currently has no progress callback.

Suggested timing boundaries: launch resolution; binding validation 1; preparation A; profile projection; binding validation 2; INI derivation; native artifact verification; native mappings; INI revalidation; preparation B; process creation/injection; first ResumeThread; Job empty; INI preservation; preparation C.

## Review status

The style gate remains non-clean with 51 reported file/rule findings. Existing dispositions are in the export, CLI, optional-metadata and single-pass implementation reports. Documentation of these decisions does not clear an enforce-mode block. No override has been used. This discussion does not authorize unrelated style repairs.

## Detailed source traces

The following read-only notes preserve the downstream analysis, source locations, snippets and limitations. Their historical test statements refer to the investigation, not new implementation work.

# PR 141 (`5eb0f19`) exec orchestration cost map

Read-only static trace. **No timings were collected**; all costs and hot spots below are hypotheses from source, not measured results. Windows is the supported execution path; non-Windows returns unsupported. Scope excludes the internals of `snapshot::load_execution` and native usvfs hooks (mapped separately).

## Ordered path and boundaries

1. **CLI entry/composition.** `src/presentation/cli/src/main.rs:25-47` captures argv and calls `runner::run_current_process`; `runner.rs:542-552` synchronously reads current directory and `LOCALAPPDATA`, then `runner.rs:532-539` parses clap input and enters async `execute`. `runner.rs:82-93,102-145` resolves and validates the environment root, starts diagnostics, then calls the dependency factory. `main.rs:28-45` constructs all command bundles, including exec, even when only exec is needed. `src/infrastructure/dependencies/src/execute_program.rs:28-39` creates the execution adapter. `execution_adapter.rs:41-55` captures caller startup path, entire inherited environment and PATH (`src/infrastructure/execution/src/launch_inputs/windows_inputs.rs:66-82`) and constructs settings adapter. This is synchronous CPU/allocation (environment strings); dependency creation itself does not execute the child. `runner.rs:416-447` validates target, cwd, program and arguments. cwd path resolution is lexical via `src/presentation/cli/src/path_resolution.rs:5+`; no executable resolution at this point. `runner.rs:449-458` creates two-stage Ctrl-C signal tokens (`src/presentation/cli/src/operation.rs:15-41`) and awaits the use case.

2. **Application forwarding.** `src/application/src/execution/execute_program.rs:35-65`: optional `PreparingExecution` callback awaited, then `RunManagedProgram.call((target,cwd,program,args,progress,cancellation)).await`, then optional `ExecutionFinished` callback awaited only on success. CLI factory sets `report_progress: None` (`dependencies/src/execute_program.rs:33-38`), so these callbacks do not run on normal CLI exec. The use case does not perform filesystem I/O itself. CLI success formats warning strings and uses child status; failures map diagnostic exit code (`runner.rs:460-491`).

3. **Thread handoff.** `dependencies/src/execution_adapter.rs:67-117` checks cancellation and, on Windows, `spawn_blocking` creates a new Tokio current-thread runtime with time enabled and `block_on(adapter.execute(...))`. All blocking filesystem calls below run on this dedicated blocking worker, not the CLI Tokio worker; the *async* `execute` function does not imply its filesystem calls yield. Async awaits are port forwarding, platform validation's already-computed future, progress callback, and process supervision. Non-Windows exits at lines 76-87. Per-exec runtime construction and captured tracing dispatcher/span add CPU overhead (unmeasured).

4. **Caller executable and child directory resolution first.** `dependencies/src/execution_adapter/native.rs:45-74` checks cancellation, clones OsString args, invokes `CallerSnapshot::resolve` and captures inheritable std streams if not in capture mode. `execution/src/launch_inputs/windows_inputs.rs:85-160` checks UTF-16/NUL, converts path-like programs relative to startup or expands every inherited PATH entry for bare names, tries original candidates then `.exe` candidates, opens until match (`119-140`), validates file metadata/type and canonical final path (`193-223`), opens and validates requested child cwd (`143-149`), encodes Windows command line (`150-159`); held file and cwd handles remain live until launch. Cost depends on number of PATH entries and missed file opens, plus validation syscalls. Child cwd does **not** participate in executable lookup. The standard-stream capture duplicates three handles (`windows_inputs.rs:225-266`). Thus bad executable/cwd fails before binding/environment scans.

5. **Effective binding and platform validation, first pass.** `native.rs:76-81` calls synchronous `SettingsAdapter::load_execution_binding` and awaits platform validation. `settings/src/lib.rs:83-104` checks pending operations before and after opening `mods.toml`, reads manifest and effective environment/settings override sources, resolves binding, checks cancellation. `game_platform/src/ports.rs:67-85` actually computes `validate_effective` *synchronously inside port invocation*, then wraps the result in an immediately-ready future. `game_platform/src/resolution.rs:88-111` validates Steam installation/build and environment separation. `game_platform/src/steam/validation.rs:36-96` opens Steam hierarchy/game, checks identity, FalloutNV.exe, Fallout_default.ini, enumerates Data for reserved invalidation BSA, reads/parses appmanifest_22380.acf; `game_platform/src/separation.rs:9-23` opens paths/ancestors and verifies noncontainment. Costs are directory opens, Data enumeration, manifest read/parser and handle checks, repeated below.

6. **First environment preparation and target selection.** `native.rs:80-100` calls synchronous `EnvironmentAdapter::prepare_execution`, then checks named output mod is present and enabled. `environment/src/execution_preparation.rs:75-220` calls `snapshot::load_execution` at line 91 (internals excluded); builds provider roots from installed mods (93-112), extracts/sorts analytical winners (114-130), finds provider and file metadata for each winner (131-151; linear `providers.iter().find` per winner), opens root/profile and reads bounded canonical Profile State files, modlist.txt and mods.toml (153-197), retains consumed bytes/state for later equality. Hypothesized allocation/CPU scales with winners and providers; disk reads are repeated during revalidation. `snapshot::load_execution` is likely material but its exact cost is not asserted here.

7. **Profile destinations/projection.** `native.rs:102-154` resolves Documents/LocalAppData via Windows Known Folders (`game_platform/src/profile_sources.rs:19-37`), clones profile text and visible-file metadata, calls `execution/src/profile.rs:118-320`. That routine validates canonical routing and TestFile settings (121-160), hashes all analytical visible files (162-174), parses plugins.txt/loadorder.txt with duplicate/stale warnings (176-226), sorts unlisted plugins by modification time (228-246), computes activation and advisory order (255-279), and constructs file, directory, saves and invalidation mappings (281-320). CPU/allocations grow with analytical files/plugin entries; no file reads here (already prepared). `native.rs:130-132` logs each projected plugin; warnings are converted at 134-154. Projection is advisory and does not enforce runtime plugin order.

8. **Binding recheck and staged INIs.** `native.rs:156-160` immediately repeats *all* binding-load and Steam/platform validation of step 5 and compares with first binding to detect changes. `native.rs:162-164` derives per-execution INIs. `environment/src/derived_profile.rs:37-101,124-175` rereads canonical INIs, possibly Fallout_default.ini, decodes/validates, opens temp and writes derived INIs to a new execution-inis temp directory. This intentionally gives the child disposable INI copies and preserves its changes only after Job drain. These reads overlap preparation's profile reads, but serve derivation and a fresh baseline.

9. **Mapping validation/configuration.** `native.rs:165-199` substitutes staged `.ini` sources, appends invalidation mapping, constructs `ViewConfiguration` and calls `VirtualGameView::configure`. `execution/src/configuration.rs:54-143` checks unique provider IDs/names and absolute/NUL-free paths, finds selected enabled target, ranks enabled overlays, validates winners against providers (linear search for each) and unique paths, then validates every mapping; `configuration.rs:145-162` applies overlays/profile directories/files/saves to native view. CPU grows with providers/winners/mappings; native configure cost is outside scope. Checking cancellation occurs before configure (`native.rs:193-195`).

10. **Immediate prelaunch revalidation.** `native.rs:200-212` calls `revalidate_execution_with_inis` after configuring view and closes view on failure; `environment/src/execution_preparation.rs:240-260` first rereads source INIs and optional fallback (`derived_profile.rs:181-186`), then performs a **fresh full** `prepare_execution_with_spool`, compares entire `PreparedExecution` (including consumed bytes, winner metadata and state), errors on changed state. The owned INI temp dir is allowed by this check. This is a deliberate TOCTOU guard, not merely redundant accidental work. Repeats snapshot, bounded profile reads and winner processing just before launch; likely a major prelaunch latency candidate, but unmeasured. Cancellation checked again after view close handling.

11. **Launch, await child, post-run validation/publication.** `native.rs:214-260` optionally prepares capture streams, launches resolved command/cwd through configured view, reports `ExecutionPrepared` if callback exists, and `supervise(...).await` waits/handles cancellation and forced termination. `native.rs:263-285` checks Job empty, drops process, preserves staged INI edits and closes capture streams. `derived_profile.rs:199-279` preservation rereads original canonical and staged files, validates edited INIs, writes preserved files and durably renames them; cost depends on INI sizes/edits and disk sync. `native.rs:288-298` performs another full `prepare_execution_with_spool` via `check_execution_with_spool` (`environment/src/execution_preparation.rs:288-298`) with a fresh uncancelled token, converting failure to `ProfileStateInvalid` warning. This is *post-run*, not prelaunch latency, and verifies retained state. `native.rs:299-317` returns status/warnings or cancellation/error, retaining staging path on uncertain failure.

## Repetition and interpretation

- Binding load + Steam/platform validation occurs twice before launch (`native.rs:76-79,156-159`); the second is an intentional change check. Each validation re-enumerates game Data and rereads appmanifest (`steam/validation.rs:73-94`). No caching is evident.
- Preparation occurs at least twice before launch (`native.rs:81,200-202`) and once after Job drain (`native.rs:288-295`), each via `execution_preparation.rs:84-220`, including the opaque snapshot operation. The second deliberately compares complete state against the first; the third checks retained state, so removing either changes guarantees.
- Canonical INI bytes are read in preparation, read again for derivation, read again in prelaunch INI revalidation and fresh preparation, and read after Job drain for publication/state check (`execution_preparation.rs:161-195,249-255,288-295`; `derived_profile.rs:37-101,181-186,210-259`). These reads have distinct validation/ownership purposes but may amplify cold disk I/O.
- No benchmark, tracing duration, sample filesystem size or elapsed time was provided. Do **not** call any stage a measured bottleneck. The likely scaling drivers are repeated full preparation, PATH failed opens, Steam Data enumeration, per-winner linear provider lookup, analytical file hashing/sorting, and staging/INI disk work. Instrument stage spans/counters on supported Windows before ranking optimization work.


# PR 141 `5eb0f19`: execution filesystem preparation (read-only trace)

Scope: `src/infrastructure/environment/src/{snapshot,execution_preparation,safe_fs,profile,derived_profile,manifest,lib}.rs`; call sites in `src/infrastructure/dependencies/src/execution_adapter/native.rs`. This is source tracing plus one synthetic fixture test, **not** a production profile or Windows syscall benchmark. No game launched. Repository files were not edited.

## Exact successful managed-execution sequence

1. `native.rs:76-81` loads effective settings, validates binding through game-platform adapter, and calls `EnvironmentAdapter::prepare_execution` (`execution_preparation.rs:75-91`). This makes full snapshot pass **A**. `native.rs:82-155` selects output target and constructs analytical VFS/profile configuration using the resulting providers, winners, visible file timestamps, and profile text. `native.rs:156-162` reloads settings/binding and calls `derive_execution_inis` (`execution_preparation.rs:224-236`), which reads canonical INIs again and writes derived INIs into a newly owned temp directory (`derived_profile.rs:125-175`). This step is a deliberate mutation of a **temporary derived profile**, not canonical provider files.
2. `native.rs:175-201` configures VFS then calls `revalidate_execution_with_inis`; `execution_preparation.rs:240-260` first calls `inis.revalidate` (re-reads five canonical INIs and possible game fallback; `derived_profile.rs:181-186,38-101`), then `prepare_execution_with_spool(..., inis.directory(), ...)` for full independent snapshot pass **B**, compares the entire `PreparedExecution`, and refuses changed state before `native.rs:220-254` launches. The owned INI temp directory is the only allowed pending entry here.
3. After supervision and proof that the Job is empty (`native.rs:260-278`), `inis.preserve()` (`native.rs:279`; `derived_profile.rs:210-278`) compares each original canonical INI and staged INI against baselines, validates changes, derives preserved keys and durably publishes changed files. It retains the temp directory on uncertain drain/error (`derived_profile.rs:282-287`). Then `native.rs:288-298` calls `check_execution_with_spool` using the owned **capture** spool if present and a fresh uncancelled token. `execution_preparation.rs:288-297` performs full snapshot pass **C**, discards its result, and emits a warning on error. This happens only on the successful branch past `outcome?` at `native.rs:286`; launch/drain/preservation failures skip it. `check_execution` at `execution_preparation.rs:305-312` is a separate public postrun API, not the native successful-path call. Without capture, pass C allows no spool. With capture, it allows only that capture spool. Captured output finish is at `native.rs:282-284`.

Thus a successful native run traverses all providers **three times** (A/B/C), not one, despite `5eb0f19` avoiding a second traversal *inside each snapshot*. During A/B it also reads canonical INIs through derivation and a separate prelaunch revalidation. Pass C checks retained postrun state (which is allowed to differ from launch input), not equality to pass A. All three inventories intentionally re-examine the live filesystem; replacing B with a cached result would weaken the immediately-before-launch change check.

## One snapshot: order and rationale

- `snapshot.rs:94-106`: execution selects mutation-level pending-work refusal plus supplied effective binding; `load_inner:122-186` opens root and temp safely and rejects every temp entry except the exact owned spool. Pending work is not silently repaired/deleted. `:189-204` validates the exact root namespace (seven names), parses `mods.toml`, validates `cache` contains only the 36-byte invalidation BSA and reads that archive, and opens optional `logs` safely. `manifest.rs:68-92` bounds manifest at 64 KiB and checks schema/binding.
- `snapshot.rs:205-226` opens `mods`, `profile`, `overwrite`; `profile.rs:123-228` checks profile namespace, saves directory, required files (missing plugins/loadorder allowed in execution), canonical routing and plugin-list grammar; execution skips recursive save walk (`profile.rs:200-202`) so opaque save payloads need not be scanned (`execution_preparation.rs:503-524`). Overwrite inventory is built **once**; optional `overwrite/meta.toml` bytes are retained for equality.
- `snapshot.rs:228-281`: enumerate every mod directory, case-fold/check names, reject reparse/non-directory, open no-follow, prove descendant identity, collect **every mod** inventory even if disabled; store raw `meta.toml` for each. `:283-314` separately reads/parses `profile/modlist.txt` and requires exact folder-name set/casing and ordering. This catches invalid disabled providers and metadata changes; disabled inventories are scanned/validated but skipped when applying winners (`:801-818`). `:316-337` uses supplied binding (no manifest-derived binding reread in execution) and builds winner map; skips `file_dependencies` and plugin activation traversal in execution.
- `snapshot.rs:772-843`: opens actual game `Data` and inventories it **once** if present; applies game base, enabled mods in modlist priority order, then overwrite. `:879-962` checks file-vs-directory namespace collisions, writes later winning files, applies exact-file and directory-subtree tombstones to existing winners, then inserts explicit absence records. Details for shadowed providers are collected but only winner details survive `:829-843`; absent tombstones remain in `current_winners` and are excluded from `PreparedExecution.winners` (`execution_preparation.rs:114-129`). This logical winner map is analytical Data projection, not observed runtime visibility (`execution_preparation.rs:47-51`).
- `execution_preparation.rs:92-219`: materializes base/mod/overwrite roots; extracts/sorts winning file references, resolves each provider identity with **linear `providers.iter().find`** and retrieves winning length/mtime; reopens canonical profile and reads present eight `PROFILE_FILES` plus modlist (up to nine files), decoding all but modlist; reads `mods.toml` **again** and carries its exact bytes and every provider meta byte into `consumed_bytes`. `PreparedExecution` equality covers binding, providers, winners, visible path/mtime, profile decoded text, directory paths, consumed snapshot state including absent/tombstone winners, raw config/metadata bytes, and winner lengths (`:53-65,202-219,240-259`). It does **not** hash arbitrary provider file contents; same-length/same-mtime in-place rewrites can escape this equality, although the live VFS is still the final runtime view.

Key excerpts (line numbers at `5eb0f19`):

```rust
// snapshot.rs:217-222, 270-275
let mut overwrite_inventory = collect_provider_inventory(
    &overwrite, ProviderKind::Overwrite, effective_binding.is_some(), cancellation,
)?;
let mut inventory = collect_provider_inventory(
    &directory, ProviderKind::DataMod, effective_binding.is_some(), cancellation,
)?;
// snapshot.rs:784-795: game Data is collected once, then apply_inventory(inventory, ...).
```

```rust
// snapshot.rs:594-607: per ordinary execution file
if file_details {
    let metadata = file.metadata().context(ErrorMarker::environment_invalid(None))?;
    inventory.files.insert(data_path.comparison_key().to_owned(),
        ProviderFileDetails { length: metadata.len(), modified: metadata.modified()?.into_std() });
}
inventory.entries.push((data_path, false));
// execution_preparation.rs:249-259: full fresh pass B
let current = self.prepare_execution_with_spool(root, &prepared.game_binding,
    inis.directory(), cancellation)?;
if current != *prepared { return Err(report!(ErrorMarker::environment_invalid(Some("execution_state_changed")))); }
```

## Inventory costs and safety constraints

Let `M` = number of installed mod roots (enabled and disabled), `E` = all entries under all provider roots including base/overwrite and directories, `D` = directory count, `F` = provider files, `W` = winner-file count, `T_i` = tombstones of provider i, `W_i` = winner-map entries just before applying provider i, `P=M+2` provider identities, and `h` = maximum component depth (bounded at 64 within a provider). Hard caps: `M<=4096` (`snapshot.rs:65,246-248`); each provider <=100,000 entries, each directory <=16,384, depth <=64 (`snapshot.rs:66,406-419`, `safe_fs.rs:411-435`); metadata <=1 MiB; profile read <=16 MiB (`snapshot.rs:67,625-631`, `profile.rs:38`). Aggregate entries across 4096 roots are **not** globally capped at 100,000.

- `validate_provider_directory` (`snapshot.rs:478-610`) walks each directory once with `entries()`, one no-follow `symlink_metadata` per entry, a no-follow `open_dir` plus directory validation for each subdir, and a no-follow `open_regular` with file-handle metadata for each regular file. Execution `file_details=true` adds one **additional file-handle metadata** lookup per non-`meta.toml` file to collect length/mtime. Thus an ordinary file incurs at least one entry metadata query + one open + one handle metadata query to reject links, plus one further handle metadata query for details; these are API operations, **not** measured kernel syscall counts. `meta.toml` is still opened/validated in the walk, then `exists` re-stats it and `read_metadata` reopens/reads/parses it (`snapshot.rs:420-427,619-687`): a true per-provider duplicate handle open, needed to validate metadata content separately from pathname traversal. Game base rejects root `meta.toml` (`snapshot.rs:580-593`). File contents otherwise are **not** read/hash-scanned by the inventory.
- Identity ancestry check occurs once per visited directory (`snapshot.rs:490-499`) and additionally for each mod root (`:264-269`); `safe_fs.rs:278-292` clones a handle then walks parents, opening parent handles and comparing `same_file::Handle` identities until the provider root. The cost can scale with provider-relative depth rather than constant per directory; roughly `O(sum over directories of depth)`, worst `O(D*h)` handle/identity operations. `SafeDir::open_absolute` (`safe_fs.rs:101-126,295-337`) walks every absolute path component with no-follow opens + metadata validation; repeated root/game/profile/temp openings repeat these prefixes. These guards block symlink/reparse/path-escape races better than string checks and are not safe to skip casually. On Windows `is_reparse` checks `FILE_ATTRIBUTE_REPARSE_POINT` (`safe_fs.rs:354-361`); on Unix it checks symlinks. Files also require `nlink()==1` (`snapshot.rs:572-579`, `safe_fs.rs:202-213`); no-follow checked single-component names (`safe_fs.rs:393-409`) and entry/depth budgets enforce boundaries. Cap-std, platform and filesystem can turn each API action into multiple kernel syscalls; exact syscall counts cannot be inferred here.
- The traversal constructs case-folded full `DataRelativePath`s, local and global duplicate-name sets (`snapshot.rs:511-546`) and retains path+directory flag for **all** entries; this costs `O(E)` memory and average `O(E * path-length)` path/casefold work, plus `O(F)` file details and `O(T)` tombstone structures. Optional metadata parsing validates canonical paths, duplicate/reserved-root tombstones, and disallows tombstones that overlap own physical paths or nested directory tombstones (`snapshot.rs:427-470,619-687`). It checks ancestors by slash boundaries; each check is proportional to path component count. Entries with the same case-folded spelling collide, preventing ambiguous winner lookup.
- `apply_inventory` builds global namespace and winner hash maps (`snapshot.rs:879-902`), `O(E_enabled)` average. For each provider with tombstones, it builds a `TombstoneIndex` and checks **every current winner** with `controlling_steps` (`:907-945`), then adds explicit absent keys (`:947-961`). Complexity is at least `O(sum_i W_i)` for tombstone-bearing providers, with extra path/index lookup work up to path depth (cannot claim exact complexity without the index implementation); with many tombstone-bearing mods this can dominate despite only one filesystem walk per provider. The broad winner map preserves exact absence and namespace collision semantics. Modlist set construction/matching is `O(M)` expected (`snapshot.rs:290-313`); `provider_metadata.sort_by` is `O(M log M)` (`:332`). Winner extraction/sorting is `O(W log W)` (`execution_preparation.rs:114-129`), and `.find` for each winner adds `O(W*P)` identity comparisons (`:133-151`): likely a CPU hotspot at large mod/winner counts, **hypothesis**, not observed timing.
- Config I/O duplicates within one pass: `mods.toml` validated/read then re-read for exact-byte equality (`snapshot.rs:194`; `execution_preparation.rs:188-195`); `modlist.txt` read by profile validation, snapshot parsing, and execution consumed-bytes collection (`profile.rs:224-228`; `snapshot.rs:283-290`; `execution_preparation.rs:161-185`); present `Fallout.ini`, optional `FalloutPrefs.ini` and `FalloutCustom.ini`, plugins/loadorder read in validation and again for `profile_files`; other optional GECK INIs/settings read once for projection. All profile config reads invoke `read_bounded`: no-follow open, metadata validation, then 64-KiB chunks until EOF (`safe_fs.rs:438-475`). Repeated reads are meaningful for independent schema checking, textual projection, and raw-byte change comparison, but may duplicate disk/cache work. They are also not a coherent atomic snapshot under concurrent external edits (`execution_preparation.rs:50-51`).

## Derived INI stage/preservation I/O

`ProfileIniInputs::read` (`derived_profile.rs:38-101`) performs `exists` (no-follow metadata) and `read_bounded` for each present one of five INIs, validates `profile_ini_valid`, extracts archive list from Fallout/Custom, and conditionally reads `game/Fallout_default.ini` if neither provides the archive list. `ExecutionInis::create` (`:125-175`) performs this read, opens temp safely, makes a `tempfile` temp dir, opens it safely, derives and writes each present INI with `SafeDir::write_new`; that does create_new no-follow + metadata validation + full write + `finish` handle metadata + fsync (`safe_fs.rs:182-200,84-92`). `revalidate` (`derived_profile.rs:181-186`) repeats INI input reads before snapshot pass B. After drain, `preserve_inner` (`:210-278`) checks existence and reads canonical and staged child for *each* of five INIs (up to ten reads); only edited children require decode/merge/encode and durable publication. It still creates a `preserved` staging directory even when `updates` is empty (`:261-264`), and closes/removes the derived temp dir on success. `rename_durable_to` (`safe_fs.rs:243-260`) syncs destination and source parent for cross-parent publication. The postrun snapshot pass C then reads retained canonical profile again; such repetition is correctness policy, but creates substantial fixed I/O independent of mod count.

## Evidence, uncertainty, next measurements

**Measured synthetic fixture:** Ran `cargo test -p infrastructure-environment execution_collects_each_provider_once -- --nocapture` at this checkout: passed; `provider inventory: (4, 2), elapsed 2.963041ms`. The fixture has game `Data` with one file, two mods (`Enabled`, `Disabled`), each with `meta.toml` and a file, and overwrite (`execution_preparation.rs:329-380`): `(4,2)` is instrumented `(directory inventory walks, metadata reads)` for the first pass and the test asserts `(8,4)` after a fresh `revalidate_execution`. These counters count only `validate_provider_directory` calls and `read_metadata` calls (`snapshot.rs:58-61,500-504,619-624`), **not** OS syscalls, all bounded profile reads, mount preparation, or per-file cost. The elapsed 2.96 ms is one local macOS tiny-fixture prepare invocation, not a representative game dataset, not Windows, not launch latency; initialization fixture setup is outside its timer (`execution_preparation.rs:371-380`). No production measurements supplied.

**Hypotheses requiring profiling:** On a large installation the three exhaustive snapshots, their per-file no-follow metadata/open/handle checks and repeated ancestor handle walks are likely the environment-layer I/O bound; many mods with tombstones can add winner-map CPU work, and winner->provider linear search can add `W*M` CPU. These are algorithmic predictions, not measured causal attribution. Separately instrument wall time, directory/file counts, `is_ancestor_of` handle counts, winner/tombstone counts, config read bytes, and `prepare A`/`derive`/`revalidate B`/`preserve`/`check C` durations on Windows with realistic enabled+disabled providers, cold/warm cache, and antivirus settings. Keep safety checks and before-launch freshness while exploring only semantics-preserving caching/indexing.


# Native execution downstream performance map (PR #141, `5eb0f19`)

Read-only static trace. No Windows launch, timing, tests, or benchmark was run. Paths below are repo-relative unless prefixed `usvfs-rs@c23705c:` (the exact `usvfs-sys` Git revision in `src/infrastructure/execution/Cargo.toml` and `Cargo.lock`). The native bundle claims fork revision `c23705c` and upstream source revision `57f1ea5` (`native/usvfs-release.json`, `src/infrastructure/execution/build.rs:26-40`). Its binary performance is **not measured** by this review. Upstream code describes possible work, not a measured hot spot.

## Critical path and time attribution

- **Before usable game process:** a `spawn_blocking` worker and its single-thread Tokio runtime are created (`src/infrastructure/dependencies/src/execution_adapter.rs:89-112`); caller launch lookup/handle opening; effective binding and environment preparation (covered by other reviews); profile projection; second binding validation; INI derivation; `ViewConfiguration` construction; four full artifact reads+SHA-256 checks; native DLL/session creation; synchronous links (including recursive walks of enabled provider roots); environment revalidation; optional capture spool setup; Job setup; suspended `CreateProcessW` and injection/proxy; Job assignment; optional awaited progress callback; `ResumeThread`. The caller's `ExecutionPrepared` event is **after** the hooked root is created and assigned but **before** it is resumed (`src/infrastructure/dependencies/src/execution_adapter/native.rs:214-260`, `src/infrastructure/execution/src/managed.rs:56-63`). It is not a precise timestamp for game startup.
- **Normal running:** `supervise` repeatedly queries active Job count every 10 ms until *all* Job processes drain, then obtains root exit status and closes the usvfs session. This is principally game/child lifetime wait, **not launch latency or evidence of wasted CPU** (`managed.rs:51-83`; `process.rs:247-266`). A root exit alone does not end supervision while its Job children are still active.
- **After Job drain:** an additional Job query/drop check, preserve derived INIs, join any output-drain threads, and fresh postrun state check. These are tail latency; the check failure yields a warning rather than refusing a successful exit (`native.rs:260-305`). An output writer retaining a pipe handle or an undrained child may prolong completion; no timing or event separation proves which occurs in a given run.
- **Failures/cancellation:** normal cancellation waits up to 5 seconds only if grace is permitted, then `TerminateJobObject` and *unbounded* async Job-drain polling; force token requests termination without grace (`managed.rs:69-109`). Emergency Drop is distinct: synchronous termination and Job polling for at most `MODS_CLEANUP_TIMEOUT_MS` (5000 ms per `native/README.md:117`) before retaining native state if drain cannot be confirmed (`process.rs:269-326`). Failed injection or Job assignment can incur separate bounded waits (`usvfs-rs@c23705c:rust/usvfs-sys/native/barrier.cpp:181-196`; `process.rs:120-143`). Do not add these failure-only timeouts to normal startup.

## Detailed trace: adapter into execution crate and native boundary

1. **Resolve target and streams** (`src/infrastructure/dependencies/src/execution_adapter/native.rs:45-74`). A cancelled request exits immediately; argument OsStrings are copied, `self.caller.resolve(...)` opens candidate executable(s) and child directory and retains handles. With a bare executable, it enumerates the captured PATH candidates and their `.exe` forms, opening candidates until the first valid one (`src/infrastructure/execution/src/launch_inputs/windows_inputs.rs:89-159`); handle metadata, `GetFileType`, and `GetFinalPathNameByHandleW` validate actual target and directory (`:193-222`). Command-line quoting is UTF-16 in memory (`launch_inputs.rs:25-54`). For non-capture, three standard handles are duplicated as inheritable (`windows_inputs.rs:234-265`). Potential cost: candidate filesystem opens/metadata/path resolution and PATH traversal, especially unavailable/network entries; small argument processing otherwise. `CallerSnapshot::capture` is prior adapter setup (`windows_inputs.rs:56-82`), not repeated per `execute` here.
2. **Effective profile composition handoff** (`native.rs:76-162`). `load_execution_binding`, awaited `validate_effective_port`, synchronous `prepare_execution`; then profile file/visible file vectors and `build_profile_configuration` (`native.rs:102-128`). Profile builder validates/decodes already prepared text, groups visible paths and parses plugin entries, sorts unlisted plugins, and creates fixed path mappings (`src/infrastructure/execution/src/profile.rs:118-323`). Projected plugin info is logged individually (`native.rs:130-132`), so trace subscribers may add cost per plugin. A **second** settings load and awaited game validation ensures unchanged binding (`native.rs:156-160`), followed by INI derivation (`:162-164`). Snapshot scans and INI details belong to other breakdowns; these calls happen before native setup and can be repeated later. `await` for validation does not imply the remainder of this async method is nonblocking.
3. **Mapping plan** (`native.rs:166-191`; `src/infrastructure/execution/src/configuration.rs:54-162`). Replace `.ini` source paths with retained derived INIs, add invalidation mapping; `ViewConfiguration::new` validates provider IDs/roots, finds output target, sorts enabled non-Steam providers by rank, validates every winner reference against the providers and mapping paths. Note `providers.iter().find(...)` inside the winner loop (`configuration.rs:109-125`) is CPU work scaling with winners × providers in the worst case, though not filesystem I/O. Winners **do not** become per-file runtime links: the runtime maps entire enabled roots regardless of winners (`:43-47,90-106`). `apply` clears bypass lists, then links each enabled Data root recursively (output target uses `LINKFLAG_CREATETARGET`), a few profile directories nonrecursively, each profile file, and a recursive save target (`:145-161`). This makes the native calls proportional to enabled roots, not winner count; their *recursive* work is a separate file-tree traversal.
4. **Bundle verification and session open, synchronous** (`src/infrastructure/execution/src/usvfs/mod.rs:60-124`). `current_exe` chooses the adjacent `usvfs/` folder; `load` calls `std::fs::read` and `Sha256::digest` on **all four** shipped DLL/proxy files each execution before the session gate/open (`:81-104`). Full bytes are allocated/read and hashed; page cache affects I/O but does not remove CPU hashing. It then opens the architecture-matched controller via C++ shim (`:106-123`). The shim `LoadLibraryExW(..., LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32)`, resolves eleven exports, creates parameters and VFS (`usvfs-rs@c23705c:rust/usvfs-sys/native/barrier.cpp:63-99`). Native VFS creation removes an old named shared-memory object then connects context (`usvfs-rs@c23705c:src/usvfs_dll/usvfs.cpp:448-469`), whose construction opens/creates shared memory and tree mappings (`src/usvfs_dll/hookcontext.cpp:57-85`). Potential startup costs: sequential bundle read/hash, DLL loader/dependency resolution, shared-memory setup, and CPU. `SESSION_ACTIVE` enforces one process-local controller session, not parallel native setup (`usvfs/mod.rs:47-57,99-104`). No existing runtime stage timer separates them.
5. **Native link building, synchronous** (`usvfs/mod.rs:202-245`; `configuration.rs:145-161`). Rust converts physical/virtual paths to checked wide strings, including verbatim/UNC normalization (`usvfs/native_path.rs:12-85`), then each call crosses FFI into shim (`usvfs-rs@c23705c:rust/usvfs-sys/native/barrier.cpp:103-126`). For recursive directory calls, upstream checks destination ancestors and mutates redirection trees, enumerates physical source directories with `quickFindFiles`, recurses into descendants, adds eligible enumerated files, updates shared parameters (`usvfs-rs@c23705c:src/usvfs_dll/usvfs.cpp:716-808`). `quickFindFiles` opens directory and loops `NtQueryDirectoryFile` into a vector (`src/shared/winapi.cpp:387-425`); path-ancestor checks may themselves query physical paths (`src/usvfs_dll/usvfs.cpp:583-622`). Each enabled provider root gets its **own** recursive traversal (`configuration.rs:145-153`), so overlapping logical Data paths can be enumerated across different provider trees even when final winners are already known. The save target is also recursive; profile-directory links are nonrecursive. Candidate cost scales with physical file/directory counts and storage latency, plus tree/string work. No evidence quantifies this against previous snapshot scans or proves it dominates. Files under an already scanned root are not necessarily read bytewise by linking; enumeration/metadata/tree construction is the likely work.
6. **Revalidation and optional capture before launch** (`native.rs:200-254`). The adapter revalidates prepared state *after* native mapping setup and explicitly closes the view if invalid (`:200-212`); other worker covers its content and repeat scans. Capture mode creates three pipes, duplicates their selected handles, makes a temp spool directory and two files, and starts **two OS threads** draining output/error in 8 KiB reads (`src/infrastructure/execution/src/child_output/windows.rs:16-38`; `child_output.rs:88-156`). Capture is lazy and only configured when `self.capture` exists. These allocations/opens/thread startups precede the launch; later data I/O occurs while the game runs. Without capture, inherited handles were duplicated earlier. A blocked stdout/stderr pipe is meant to drain concurrently; spool write failures trigger force cancellation while reads continue (`child_output.rs:114-150`). There is no await around these methods.
7. **Suspended hooked launch and injection, synchronous** (`src/infrastructure/execution/src/process.rs:67-153`, `usvfs/mod.rs:126-169`). Create a kill-on-close Job and set limit, fill startup stream handles, convert paths/command, call shim. The shim builds a restricted child-handle attribute list when streams are inherited, sets `CREATE_SUSPENDED` (plus optional private process group), then calls upstream `usvfsCreateProcessHooked` (`usvfs-rs@c23705c:rust/usvfs-sys/native/barrier.cpp:129-175`). Upstream calls `CreateProcessW` suspended and injects **before** returning the handles (`usvfs-rs@c23705c:src/usvfs_dll/usvfs.cpp:817-858`). Same-bitness injection finds the matching DLL and overwrites suspended thread context with an injection stub (`src/usvfs_helper/inject.cpp:91-119`; `src/tinjectlib/injectlib.cpp:342-393,475-481`). Cross-bitness launches matching `usvfs_proxy_*.exe` and **waits for proxy completion, max 15 seconds**, which is a possible prelaunch wait; proxy opens target process/thread and calls injection (`src/usvfs_helper/inject.cpp:120-192`; `src/usvfs_proxy/main.cpp:145-170`). Loader, child creation, stub operations and proxy launch can each cost OS/AV I/O and CPU; no data here picks one. Once shim returns, Rust assigns still-suspended root to Job (`process.rs:119-151`). Only `supervise` later resumes it. Note proxy completion does not itself imply the game has finished.
8. **Run, drain, close** (`native.rs:256-305`; `managed.rs:51-109`; `process.rs:156-326`). Awaited progress callback occurs before supervise. `ResumeThread` runs synchronously, Job active-process query checks immediately and every 10 ms with Tokio `sleep(...).await` between polls (`managed.rs:56-83`; `process.rs:176-193,247-262`). `root_status` performs a zero-timeout process wait plus `GetExitCodeProcess` only when Job empty (`process.rs:200-213`). `finish` confirms Job empty and calls native disconnect/free/unload (`process.rs:229-231,286-297`; `usvfs/mod.rs:181-190`; `usvfs-rs@c23705c:rust/usvfs-sys/native/barrier.cpp:199-213`). Then adapter repeats Job empty query, drops process, preserves INIs, joins two pipe-drain threads if capturing, and performs postrun environment check (`native.rs:260-305`). The capture response's later `ExecutionCapture::read` reads both complete spool files and hashes/UTF-8 decodes them (`child_output.rs:57-85`): this is **additional post-execution work**, where called outside this specific `native.rs` method.

## Async/blocking and evidence gaps

`ExecutionAdapter::run_port` moves the full native session into `spawn_blocking`, builds a **current-thread Tokio runtime on that blocking worker**, and `block_on`s `execute` there (`src/infrastructure/dependencies/src/execution_adapter.rs:89-112`). This protects the caller Tokio worker and preserves thread-affine usvfs state, but has per-execution worker/runtime setup cost and does not make inner work cooperative. `ExecutionAdapter::execute` is `async`, yet ordinary filesystem operations, SHA-256 hashing, FFI setup/recursive native directory enumeration, pipe preparation, process creation/injection, Win32 Job calls and postrun validation are direct synchronous calls on that dedicated blocking worker (`native.rs:53-81,119-128,162-254,260-305`). Explicit yields are awaited game validation (`:79,157`), progress callback (`:257`), and supervision's 10 ms Tokio sleeps (`managed.rs:82,99`). An async call site does not make the native prelaunch stages cooperative. Capture drain uses separate `std::thread` threads, while joining them in `PrivateStreams::finish` is blocking (`child_output.rs:164-188`). Emergency `Drop` itself blocks up to its bounded timeout, not a Tokio await (`process.rs:269-326`). No tracing spans/timers in these named functions isolate target lookup, full-bundle hashing, per-root native recursion, proxy wait/injection, post-setup revalidation, game/Job run time, stdout drain join, or postrun check. Windows ETW/stage-specific elapsed timers, counts for roots/files/bytes, and explicit first-resume/Job-empty timestamps would be needed before naming an actual bottleneck. The only loop whose normal elapsed duration naturally tracks *the game* is Job supervision; conflating it with setup latency is misleading.
