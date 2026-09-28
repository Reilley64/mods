# Issue #33 coding-style-gate disposition

The enforce-mode scan reported 111 likely violations. This audit fixes nine concrete findings and explicitly accepts the other 102. Accepted findings are not described as a clean review. Acceptance does not clear enforce mode; final handoff still requires `/coding-style-gate override <reason>`.

Audit baseline: merge base `86e9563bc5422f9afce47046fdc9df17c51a08d1`; issue #33 repair head before this disposition was `436bb8b8a72f4ec9ad864281b270e5eb8ffc23c8`.

## Dispositions

1. `src/infrastructure/dependencies/src/execution_adapter.rs` — **Runtime primitives / Separate progress reporting**

   **ACCEPT** — `src/infrastructure/dependencies/src/execution_adapter.rs` — **Runtime primitives / Separate progress reporting**. `run_port` takes `progress` and `cancellation` as distinct closure arguments (70) and forwards them separately (106-107); native `execute` also has separate `Option<ReportProgress>` and `CancellationToken` parameters. Nothing bundles progress with cancellation.

2. `src/infrastructure/dependencies/src/execution_adapter/native.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/dependencies/src/execution_adapter/native.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `ExecutionAdapter::execute` is an infrastructure adapter method, not an application use-case entry point; even so, its inputs are separate and cancellation is last. Commit `b3fe423` changed only its advisory `tracing::info!`, not the signature.

3. `src/application/src/ports/archive.rs` — **Runtime primitives / Separate progress reporting**

   **ACCEPT — `src/application/src/ports/archive.rs` — Runtime primitives / Separate progress reporting.** `IndexArchive` and `ExtractApprovedFiles` take `Option<ReportProgress>` and `CancellationToken` as separate positional values (lines 11–22). `ReportProgress` is an independent application-owned callable and has no cancellation operation. This is the rubric’s compliant split, not one combined control/progress port.

4. `src/infrastructure/archive/src/index.rs` — **Application use cases and ports / Callable port invocation**

   **ACCEPT — `src/infrastructure/archive/src/index.rs` — Application use cases and ports / Callable port invocation.** The apparent direct call is `checkpoint()` at line 235, whose type is the private `Option<&dyn Fn()>` parameter at lines 201–205. It is a synchronous infrastructure-local progress checkpoint, not an application-owned callable port. The real application progress port in `archive/src/adapter.rs:85-89` is invoked with `.call((ProgressEvent::ArchiveScanCheckpoint,))`, as required.

5. `src/presentation/mcp/src/execution_output.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/presentation/mcp/src/execution_output.rs` — **Application use cases and ports / Use-case parameters**. `executed(output, captured)` is a synchronous MCP serializer from typed output/streams to `CallToolResult`, not an application use case or application-owned port; it has no dependency bundle or cancellation support. The branch changes warning text/assertions, not its signature.

6. `src/presentation/mcp/src/diagnostics.rs` — **Errors / Preserve causes at owned boundaries**

   `src/presentation/mcp/src/diagnostics.rs` — **Errors / Preserve causes at owned boundaries** — **ACCEPT**. `RollingFileAppender::build` failure is intentionally downgraded to the capability state `SessionStart::SetupFailed`, after which `server.rs` emits the fixed allowlisted `SINK_WARNING` and runs the command unchanged. No error `Result` is propagated with a replaced marker, and retaining/exposing sink diagnostics would conflict with the frozen best-effort presentation policy. The file is unchanged from the merge base.

7. `src/infrastructure/dependencies/src/execute_program.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/dependencies/src/execute_program.rs` — Application use cases and ports / Use-case parameters.** The only functions are `Resources` factory methods that construct and return `ExecuteProgramDependencies`; they are not application use cases. Their actual inputs are explicit (`startup_directory`) and `force_cancellation` is final (lines 10–14 and 28–32). The real use case in `src/application/src/execution/execute_program.rs` has the dependency bundle first and `CancellationToken` last.

8. `src/infrastructure/environment/src/lib.rs` — **Errors / Context propagation**

   **ACCEPT — `src/infrastructure/environment/src/lib.rs` — Errors / Context propagation.** The issue-33 delta in this file is only tests at lines 653–715; those use `expect`/`expect_err` and add no report-boundary propagation. Existing production propagation uses `.context(...)`, which retains the cause tree, or `.into_report().context(...)`; there is no `context_transform`. The conditional mapping at lines 146–152 adds `game_install_invalid` while returning the original report otherwise, so it does not discard child markers.

9. `src/infrastructure/archive/src/adapter.rs` — **Runtime primitives / Cancellation propagation and checkpoints**

   **ACCEPT** — `src/infrastructure/archive/src/adapter.rs` — **Runtime primitives / Cancellation propagation and checkpoints**. The application ports receive the caller token directly and checkpoints are inline early returns (e.g. 1108-1110, 1115-1119, 1125-1129). The child token at 1091 is only an internal producer-stop handle, so consumer failure can stop the blocking producer at 1139-1143; it does not replace the application-port token.

10. `src/presentation/mcp/src/server.rs` — **Application use cases and ports / Callable port invocation**

   `src/presentation/mcp/src/server.rs` — **Application use cases and ports / Callable port invocation** — **ACCEPT**. Calls such as `execute_program(...)`, `list_settings(...)`, and `install_archive(...)` are named use-case functions, not application-owned callable-port values. The `ReportProgress` code constructs a port; it does not directly invoke one. The file is unchanged.

11. `src/presentation/mcp/src/cancellation.rs` — **Runtime primitives / Separate progress reporting**

   **ACCEPT — `src/presentation/mcp/src/cancellation.rs` — Runtime primitives / Separate progress reporting.** `CancellationState` stores cancelled request IDs and a `ProgressToken -> RequestId` correlation map (lines 153–157), but this presentation transport neither reports progress nor exposes a combined callback. The map only lets send/commit logic suppress queued MCP progress frames associated with a cancelled request (lines 68–74 and 176–182). Actual progress remains a separate `ReportProgress` port in `server.rs`.

12. `src/application/src/conflicts/projection.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/application/src/conflicts/projection.rs` — Application use cases and ports / Use-case parameters.** `project_installation`, `project_list`, `project_inspection`, and `project_path` are private projection helpers used by the actual use-case entry files (`list_effective_conflicts.rs`, `inspect_mod_conflicts.rs`, `explain_path.rs`, and `install_archive.rs`), not primary use cases with `<UseCase>Dependencies`. Their business inputs are explicit, and cancellation is already final. The rule therefore does not require a dependency bundle here.

13. `src/infrastructure/game_platform/src/steam/discovery.rs` — **Control flow / Choose the narrow conditional form**

   **ACCEPT** — `src/infrastructure/game_platform/src/steam/discovery.rs` — **Control flow / Choose the narrow conditional form**. `match validate(&candidate)` at 82-86 has three meaningful outcomes: return a successful binding, retain the first validation error, or discard a later error. A one-pattern conditional would obscure or lose one of those payload-dependent paths.

14. `src/presentation/mcp/src/contract.rs` — **Errors / Preserve causes at owned boundaries**

   `src/presentation/mcp/src/contract.rs` — **Errors / Preserve causes at owned boundaries** — **ACCEPT**. External failures from JSON parsing and schema compilation use `.into_report()?`. `Error::other(...)` converts an absent required JSON shape or unknown tool from `Option`; there is no underlying cause to discard. Validation errors are deliberately converted to allowlisted field/kind records to honor the no-value-echo presentation contract. The file is unchanged.

15. `src/application/src/ports/execution.rs` — **Runtime primitives / Separate progress reporting**

   **ACCEPT — `src/application/src/ports/execution.rs` — Runtime primitives / Separate progress reporting.** `RunManagedProgram` has distinct `Option<ReportProgress>` and `CancellationToken` arguments (lines 11–22). Neither type combines the other capability.

16. `src/presentation/mcp/src/server.rs` — **Functions and tests / Test public behavior**

   **FIXED** — The removed test called the private, single-use `invalid_arguments` helper with already-sanitized synthetic problems and therefore did not prove the public boundary policy. Its structured-error assertions now run through `Server::run_tool` in `invalid_arguments_do_not_touch_the_environment`, which also verifies that rejected values are not echoed and the environment is untouched.

17. `src/infrastructure/archive/src/limits.rs` — **Comments and documentation / Rustdoc format**

   **ACCEPT** — `src/infrastructure/archive/src/limits.rs` — **Comments and documentation / Rustdoc format**. Lines 12, 14, and 16 use `///` for item contracts. Line 18 intentionally uses `//` because it explains implementation rationale—why staged output is capped independently of archive expansion—rather than documenting the constant’s API. That is the rubric’s required ordinary-comment case.

18. `src/infrastructure/game_platform/src/version.rs` — **Control flow / Choose the narrow conditional form**

   `src/infrastructure/game_platform/src/version.rs` — **Control flow / Choose the narrow conditional form** — **ACCEPT**. The `match read_file_version(file)` needs both payloads: use the `Ok(version)` value, or retain the first `Err(error)` report and continue to the fallback xNVSE file. A one-pattern conditional is not narrower for this two-payload recovery. The file is unchanged.

19. `src/presentation/mcp/src/install_output.rs` — **Errors / Rootcause lower-layer results**

   **ACCEPT — `src/presentation/mcp/src/install_output.rs` — Errors / Rootcause lower-layer results.** The flagged `Result<Value, rmcp::ErrorData>` signatures are presentation-layer wire mappers, while the rule expressly governs fallible domain, application, and infrastructure APIs. `ErrorData` is RMCP’s required transport-boundary error, not a second lower-layer error framework. The issue-#33 diff did not introduce these signatures; it only adds `"kind": "archive_candidate"` and a contract test.

20. `src/application/src/installation/install_archive/fomod.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/application/src/installation/install_archive/fomod.rs` — Application use cases and ports / Use-case parameters.** This is a private child implementation module of the `install_archive` use case. `condition_tree_matches` and `evaluate` are FOMOD algorithms, not use-case entry points; `DependencyFacts` is typed FOMOD input, not an unrelated dependency bag, and cancellation is final in both signatures.

21. `src/presentation/cli/src/runner.rs` — **Control flow / Guard clauses**

   **ACCEPT** — `src/presentation/cli/src/runner.rs` — **Control flow / Guard clauses**. Exec validation already uses unnested exiting `let ... else` guards at 394-414. The issue-33 hunk at 428-437 is an exhaustive warning-to-message value mapping; it does not nest a success path behind an exiting alternative.

22. `src/infrastructure/archive/src/index.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/archive/src/index.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `index_archive_with_progress`, `verify_identity`, and `hash_and_prefix` are synchronous infrastructure algorithms, not application use cases. Their values are explicit; the public indexing seam already places cancellation last. The file is unchanged.

23. `src/infrastructure/archive/src/rar.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/archive/src/rar.rs` — Application use cases and ports / Use-case parameters.** `index`, `open`, and the preflight/parser functions are infrastructure adapter helpers, not async application use cases, so no use-case dependency bundle belongs in their signatures. Borrowed cancellation followed by `Instant started` is an internal archive work-budget signature, not a violation of the top-level use-case parameter contract.

24. `src/presentation/mcp/src/cancellation.rs` — **Runtime primitives / Cancellation propagation and checkpoints**

   **ACCEPT — `src/presentation/mcp/src/cancellation.rs` — Runtime primitives / Cancellation propagation and checkpoints.** This file does not import or wrap `tokio_util::sync::CancellationToken`. `CancellationState` tracks JSON-RPC request IDs/progress tokens so cancelled responses are suppressed at transport commitment, a concrete rmcp 3.3.0 gap documented at lines 1–6. It is presentation transport policy, not application-to-infrastructure cancellation propagation, and it defines no hidden one-line cancellation-check helper.

25. `src/presentation/cli/src/install_warning.rs` — **Formatting and imports / Import placement and use**

   **ACCEPT** — `src/presentation/cli/src/install_warning.rs` — **Formatting and imports / Import placement and use**. Production imports are module-scoped at 1-9, and test imports are scoped to the nested test module at 282-287. The qualified names in the body are enum/type associated variants, not repeated module paths or block-local imports.

26. `src/presentation/mcp/src/install_output.rs` — **Formatting and imports / Phase spacing**

   `src/presentation/mcp/src/install_output.rs` — **Formatting and imports / Phase spacing** — **ACCEPT**. The added regression test separates fixture construction (current lines 410–457), preview/installed mapping (459–467), and contract validation (469–477). Statements inside each block jointly perform one operation; another blank line is not required. Production candidate staging and JSON output construction are also separated. This is an issue-33 change, but the suspected spacing defect is not present.

27. `src/application/src/ports/execution.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/application/src/ports/execution.rs` — Application use cases and ports / Use-case parameters.** `RunManagedProgram` is a callable port alias, not a use-case function. Its business inputs are separate typed values and `CancellationToken` is final (lines 12–18). Adding a `<UseCase>Dependencies` bundle at this port boundary would be the wrong abstraction.

28. `src/infrastructure/execution/src/child_output/windows.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/execution/src/child_output/windows.rs` — Application use cases and ports / Use-case parameters.** `ExecutionCapture::prepare` and `PrivateStreams` methods are Windows infrastructure helpers, not application use cases. `prepare(&self, failure: CancellationToken)` has no business-input/use-case dependency contract, and its only token is necessarily final.

29. `src/infrastructure/execution/src/child_output.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/infrastructure/execution/src/child_output.rs` — **Application use cases and ports / Use-case parameters**. `ExecutionCapture::drain(&self, stdout, stderr, failure)` is an infrastructure spool operation, not an application use case. Its concrete pipe arguments are adapter resources, and its token is already the final explicit parameter.

30. `src/infrastructure/execution/src/child_output.rs` — **Formatting and imports / Phase spacing**

   `src/infrastructure/execution/src/child_output.rs` — **Formatting and imports / Phase spacing** — **ACCEPT** for issue #33. Any arguable blank-line cleanup around spool ownership or test cleanup is pre-existing: this file is byte-identical to the merge base. `docs/acceptance/issue-33.md:29` records the owner exclusion of the unrelated coding-style report; changing this capture implementation would add a new execution-lifecycle patch with no #33 acceptance behavior.

31. `src/presentation/mcp/src/contract.rs` — **Formatting and imports / Import placement and use**

   **ACCEPT — `src/presentation/mcp/src/contract.rs` — Formatting and imports / Import placement and use.** Imports are at production module scope (lines 1–14) or nested `mod tests` scope, never inside a function/block. `draft202012::new`, `Value::Object`, and `Error::other` name associated items through already imported modules/types; they are not repeated crate-qualified normal free-function paths.

32. `src/presentation/mcp/src/lifecycle.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/presentation/mcp/src/lifecycle.rs` — Application use cases and ports / Use-case parameters.** `Lifecycle::{retain_capture,release_response,execution_finished,drain}` are presentation supervision methods, not application use cases. Where cancellation is supported, `execution_finished(..., cancellation)` places the token last; there is no hidden use-case dependency bundle.

33. `src/infrastructure/archive/src/adapter.rs` — **Formatting and imports / Phase spacing**

   **FIXED** — `src/infrastructure/archive/src/adapter.rs` — **Formatting and imports / Phase spacing**. `index_for_application` runs index acquisition at 123-124 directly into cancellation/time validation at 125-136, then directly into digest/output staging at 137-139. Add one blank line after 124 and one after 136 to mark the acquisition, validation, and construction phases. The blank-line-only repair has now been applied.

34. `src/infrastructure/environment/src/snapshot.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/environment/src/snapshot.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `load`, `load_inner`, and assessment helpers are infrastructure functions, not application use cases, so no dependency bundle is required. Their cancellation arguments are explicit and terminal on the public load seams. The file is unchanged.

35. `src/presentation/mcp/src/server.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/presentation/mcp/src/server.rs` — Formatting and imports / Phase spacing.** `run_tool` separates shutdown guard, admission, contract validation, cancellation, dispatch/staging, diagnostic capture, contract enforcement, and output with blank lines (notably lines 118–155 and 437–476). Statements kept together within each tool arm jointly validate one input or prepare/invoke one use case, so splitting those would create false phases.

36. `src/application/src/conflicts/projection.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/application/src/conflicts/projection.rs` — Formatting and imports / Phase spacing.** Existing functions already separate real phases. For example, `project_installation` separates candidate projection (lines 69–110), scan replacement (112–123), resolution (125–129), and output construction (131–135). `Projection::rows` similarly separates key acquisition, ordinary-row projection, tombstone-row projection, and publication. Adjacent statements within each block jointly perform one operation. The file has no issue-33 diff.

37. `src/infrastructure/dependencies/src/execution_adapter/native.rs` — **Formatting and imports / Phase spacing**

   **FIXED** — `src/infrastructure/dependencies/src/execution_adapter/native.rs` — **Formatting and imports / Phase spacing**. Virtual-view acquisition completes at 183-184, but distinct environment revalidation/recovery starts at 185 with no blank line. Add a blank line between 184 and 185. Although the issue-33 hunk changes line 129, this is a direct, blank-line-only repository-standard repair with no semantic or scope expansion. The blank-line-only repair has now been applied.

38. `src/infrastructure/execution/src/child_output/windows.rs` — **Formatting and imports / Phase spacing**

   `src/infrastructure/execution/src/child_output/windows.rs` — **Formatting and imports / Phase spacing** — **ACCEPT** for issue #33. Pipe acquisition, inherited-handle duplication, and drain construction form the single private-stream setup operation; the output construction is already separated. Any additional separator would be baseline whitespace churn in an unchanged lifecycle file, which the accepted ledger says remains unchanged (`docs/acceptance/issue-33.md:33`).

39. `src/presentation/mcp/src/conflict_output.rs` — **Formatting and imports / Import placement and use**

   **ACCEPT — `src/presentation/mcp/src/conflict_output.rs` — Formatting and imports / Import placement and use.** Production imports are module-scope (lines 1–22), and test imports are nested-test-module scope. Qualified names are enum variants such as `TombstoneEffect::Controlling`, `ResolutionReason::...`, and `ConflictRow::...`, where qualification identifies the variant; there is no block-local import or needless repeated fully qualified free-function call.

40. `src/presentation/mcp/src/error_output.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/presentation/mcp/src/error_output.rs` — Formatting and imports / Phase spacing.** `application_error` separates report classification (lines 9–12), code/detail mapping (14–98), output construction/mutation (100–104), and return (106). Blank lines inside the match group related error categories; they do not split a joint operation. The file has no issue-33 diff.

41. `src/presentation/mcp/src/cancellation.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/presentation/mcp/src/cancellation.rs` — **Application use cases and ports / Use-case parameters**. `io_transport(reader, writer, lifecycle)` and the `Transport`/`Sink` methods are presentation-framework adapter code, not application use cases/ports. They have no `<UseCase>Dependencies` or business/cancellation ordering contract.

42. `src/presentation/mcp/src/diagnostics.rs` — **Dependencies / Narrow custom implementations**

   `src/presentation/mcp/src/diagnostics.rs` — **Dependencies / Narrow custom implementations** — **ACCEPT**. The module delegates logging/runtime work to `tracing`, `tracing_appender`, and `tracing_subscriber`. Its custom surface only implements product policy: one UUID session, boundary events, and project-target filtering. It does not reimplement a logger or runtime.

43. `src/presentation/mcp/src/error_output.rs` — **Formatting and imports / Import placement and use**

   **ACCEPT — `src/presentation/mcp/src/error_output.rs` — Formatting and imports / Import placement and use.** Imports are module-scope (lines 1–6) or nested-test-module scope. `ErrorCode::...`, `ErrorData::internal_error`, and `CallToolResult::structured_error` are associated-item/variant qualification through locally imported types, not prohibited qualified normal-item calls.

44. `src/infrastructure/archive/src/rar.rs` — **Functions and tests / Test public behavior**

   **ACCEPT — `src/infrastructure/archive/src/rar.rs` — Functions and tests / Test public behavior.** The private `preflight_bytes` seam supports focused project-owned safety regressions: rejecting compressed/encrypted forms before parser construction, enforcing metadata/member bounds, retaining source identity across path replacement, and cancellation between copy chunks (lines 1230–1533). Those tests protect mods’ preflight/resource policy and unstable archive-adapter boundary; they do not recreate the `rars` dependency suite. The file has no issue-33 diff.

45. `src/presentation/mcp/src/cancellation.rs` — **Formatting and imports / Phase spacing**

   **FIXED** — `src/presentation/mcp/src/cancellation.rs` — **Formatting and imports / Phase spacing**. In `queued_progress_is_dropped_but_committed_response_finishes`, fixture/transport acquisition ends at 345 and the request/write action begins at 346 without a phase break. Add a blank line between them; the spacing rule applies to test bodies too. The blank-line-only repair has now been applied.

46. `src/presentation/mcp/src/contract.rs` — **Formatting and imports / Phase spacing**

   `src/presentation/mcp/src/contract.rs` — **Formatting and imports / Phase spacing** — **ACCEPT** for issue #33. Production code already separates document/tool acquisition, map construction, schema transformation, and output construction. Any concern in old tests is in a file unchanged from the merge base; modifying it would add contract-module churn despite the frozen no-wire-format-change scope.

47. `src/presentation/mcp/src/settings_output.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/presentation/mcp/src/settings_output.rs` — Formatting and imports / Phase spacing.** `mutation` keeps the related response-fact transformations (`source`, `binding`, `warnings`, lines 31–53) together, then separates output construction and return. Constructing `result` and extending that same result’s warning content are one output-construction operation, so no additional phase break is required.

48. `src/application/src/installation/install_archive/fomod.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/application/src/installation/install_archive/fomod.rs` — Formatting and imports / Phase spacing.** `evaluate` separates budget setup (160), cancellation guard (162–166), unsupported-dependency validation (168–193), selection staging (195–218), and choice evaluation (220 onward). Statements left adjacent within each block are one operation. The file has no issue-33 diff.

49. `src/presentation/mcp/src/inputs.rs` — **Functions and tests / Test public behavior**

   **ACCEPT** — `src/presentation/mcp/src/inputs.rs` — **Functions and tests / Test public behavior**. The tests call the crate seam `tools()` and validate client-visible generated/published input schemas and annotations (189-307). They protect the project-owned MCP wire contract rather than recreating `jsonschema` or `schemars` internals.

50. `src/presentation/mcp/src/server.rs` — **Application use cases and ports / Use-case parameters**

   `src/presentation/mcp/src/server.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `Server::run_tool` is a presentation dispatcher, not an application use case. Its application calls pass the dependency bundle first and cancellation last (`execute_program`, `set_game_directory`, `install_archive`, and conflict calls). The file is unchanged.

51. `src/application/src/installation/install_archive.rs` — **Functions and tests / Test public behavior**

   **ACCEPT — `src/application/src/installation/install_archive.rs` — Functions and tests / Test public behavior.** The test module invokes the public `install_archive` use case with callable-port fakes; it does not call private `evaluate`, `plan_candidates`, `InstallerEvaluation`, or `condition_tree_matches`. Assertions cover owned boundary behavior such as preview/mutation access, progress order, cancellation, FOMOD policy, conflict projection, and preserved error markers. It does not recreate a dependency’s test suite.

52. `src/infrastructure/archive/src/adapter.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/archive/src/adapter.rs` — Application use cases and ports / Use-case parameters.** `ArchiveAdapter::{index_port,extract_port}` construct infrastructure implementations of application-owned ports, and `index_for_application` is a private adapter helper. None is an application use-case entry point. The port closures retain their declared argument order and put cancellation last.

53. `src/presentation/mcp/src/diagnostics.rs` — **Functions and tests / Test public behavior**

   **ACCEPT** — `src/presentation/mcp/src/diagnostics.rs` — **Functions and tests / Test public behavior**. Tests exercise the crate-visible `DiagnosticSession` boundary and observe owned effects: no log for Off, UUID log/boundary events, and configured filtering (168-225). They do not inspect dependency-private state.

54. `src/presentation/mcp/src/execution_output.rs` — **Formatting and imports / Phase spacing**

   `src/presentation/mcp/src/execution_output.rs` — **Formatting and imports / Phase spacing** — **FIXED**. This file changed in `b3fe423`. In `executed`, initial structured-result construction ends at line 14 and warning publication begins at line 15 with no phase break. In the changed test, warning-text checks end at line 107, then structured-content acquisition is at line 108 while the blank line is incorrectly after that acquisition; move the break before `let value` and keep `let value` with its schema/value assertions. This is behavior-neutral, in-scope whitespace remediation. (A final blank before `Ok(())` is optional only if the local convention treats test return as a separate output phase; the two identified boundaries are the concrete violation.) The blank-line-only repair has now been applied.

55. `src/presentation/mcp/src/main.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/presentation/mcp/src/main.rs` — Formatting and imports / Phase spacing.** `run` separates startup acquisition, environment-path resolution, typed-root validation, service/session/transport composition, and execution under the chosen diagnostic session (lines 48–94). The statements that stage session, server, lifecycle, stdio transport, and the supervised future are intentionally one composition phase.

56. `src/infrastructure/execution/src/child_output.rs` — **Dependencies / Narrow custom implementations**

   **ACCEPT — `src/infrastructure/execution/src/child_output.rs` — Dependencies / Narrow custom implementations.** The exact gap is documented at lines 1–3: `os_pipe` and `std` provide pipes/threads, while this adapter owns only the rule that spool failure cancels execution while both pipes continue draining. The implementation is limited to `ExecutionCapture::drain` and `CaptureDrains::{finish,drop}`; it does not reimplement a runtime, pipe package, or unrelated abstraction.

57. `src/presentation/mcp/src/cancellation.rs` — **Functions and tests / Test public behavior**

   **ACCEPT** — `src/presentation/mcp/src/cancellation.rs` — **Functions and tests / Test public behavior**. Tests exercise the implemented `Transport<RoleServer>` boundary and observe delivered/suppressed wire frames (338-559). Module docs 1-6 identify the concrete rmcp EOF/cancellation gap, making these focused unstable-adapter regression tests explicitly allowed.

58. `src/presentation/mcp/src/lifecycle.rs` — **Functions and tests / Test public behavior**

   `src/presentation/mcp/src/lifecycle.rs` — **Functions and tests / Test public behavior** — **ACCEPT**. Tests exercise crate-visible lifecycle operations and observe project behavior through weak ownership, semaphore admission, shutdown, and task completion. They do not inspect the private `captures` map or recreate Tokio/rmcp tests. The file is unchanged.

59. `src/infrastructure/dependencies/src/install_archive.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/infrastructure/dependencies/src/install_archive.rs` — Formatting and imports / Phase spacing.** The function body performs one operation: construct and return one `InstallArchiveDependencies` struct literal (lines 5–18). Its fields are not separate semantic phases; blank lines between them would falsely split one construction.

60. `src/infrastructure/environment/src/execution_preparation.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/environment/src/execution_preparation.rs` — Application use cases and ports / Use-case parameters.** `EnvironmentAdapter::{prepare_execution,...}` are infrastructure adapter methods, not application use cases; `&self` is the adapter receiver, typed root/binding inputs are explicit, and cancellation is last. The only issue-33 delta is the approved analytical-projection Rustdoc on `PreparedExecution`, not a parameter change.

61. `src/application/src/installation/install_archive.rs` — **Formatting and imports / Phase spacing**

   **FIXED** — `src/application/src/installation/install_archive.rs` — **Formatting and imports / Phase spacing**. Installer evaluation ends at 288, and the distinct unresolved-selection output/early-return phase starts at 289 without a blank line. Add a blank line between 288 and 289. The blank-line-only repair has now been applied.

62. `src/infrastructure/archive/src/extract.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/archive/src/extract.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `extract_selected` and its helpers are synchronous infrastructure algorithms, not application use cases; the generic output callback after cancellation does not violate an application-use-case signature rule. The file is unchanged.

63. `src/infrastructure/archive/src/seven_zip.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/archive/src/seven_zip.rs` — Application use cases and ports / Use-case parameters.** `index`, `open_controlled`, and private preflight/validation functions are infrastructure parser helpers, not application use cases. Their cancellation/work-budget parameters therefore do not require a use-case dependency bundle; applying the rubric here is a category error.

64. `src/infrastructure/dependencies/src/execution_adapter.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/dependencies/src/execution_adapter.rs` — Application use cases and ports / Use-case parameters.** `ExecutionAdapter::new`, builder methods, and `run_port` are infrastructure composition. `run_port` constructs the existing application-owned `RunManagedProgram` callable; its closure’s argument order is that port contract, not a use-case dependency bundle, and cancellation is last.

65. `src/presentation/mcp/src/cancellation.rs` — **Dependencies / Narrow custom implementations**

   **ACCEPT** — `src/presentation/mcp/src/cancellation.rs` — **Dependencies / Narrow custom implementations**. Module docs 1-6 name the exact rmcp 3.3.0 gap: EOF draining bypasses its cancelled-request pool and rmcp lacks a writer-commitment hook/split reader. The code retains `AsyncRwTransport`/`JsonRpcMessageCodec` and adds only request state plus the missing `start_send` commitment check, so it is narrow compatibility infrastructure.

66. `src/presentation/mcp/src/conflict_output.rs` — **Formatting and imports / Phase spacing**

   `src/presentation/mcp/src/conflict_output.rs` — **Formatting and imports / Phase spacing** — **ACCEPT** for issue #33. Any arguable spacing between enum-to-wire transformation and JSON construction is pre-existing in a file with no branch or working-tree diff. Adding presentation-mapping cleanup would exceed the owner-frozen bounded repair and touch no #33 distribution/runtime acceptance criterion.

67. `src/application/src/ports/archive.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/application/src/ports/archive.rs` — Application use cases and ports / Use-case parameters.** `IndexArchive` and `ExtractApprovedFiles` are callable port type aliases, not use-case functions. Their business inputs are separate typed arguments and `CancellationToken` is final in both aliases (lines 11–22). A use-case dependency bundle does not belong inside these port signatures.

68. `src/domain/src/provider_resolution.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/domain/src/provider_resolution.rs` — Formatting and imports / Phase spacing.** `TombstoneIndex::insert` keeps key/entry/order construction together, then has a blank before publication into the two indexes (lines 24–45). `controlling_steps` separates iterator construction from its scan, and `resolve_effective_file` separates the winner guard from absent-output construction. No semantic phases are run together or falsely split; the file has no issue-33 diff.

69. `src/presentation/cli/src/runner.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/presentation/cli/src/runner.rs` — **Application use cases and ports / Use-case parameters**. `execute`/`dispatch` are CLI routing/composition helpers, not application use cases. Their calls to real use cases follow the rule—for example `execute_program(dependencies.execute_program, ..., signals.cancellation.clone())`, with the narrow bundle first and cancellation last. The branch only changes warning rendering/tests.

70. `src/application/src/ports/progress.rs` — **Dependencies / Narrow custom implementations**

   `src/application/src/ports/progress.rs` — **Dependencies / Narrow custom implementations** — **ACCEPT**. `ReportProgress` is a narrow application-owned callable-port type over `std` traits, not custom infrastructure or a replacement runtime/protocol. Its invariant—observational only, never controlling cancellation or success—is documented immediately above it.

71. `src/infrastructure/environment/src/profile.rs` — **Formatting and imports / Phase spacing**

   **FIXED — `src/infrastructure/environment/src/profile.rs` — Formatting and imports / Phase spacing.** Concrete guard-to-work transitions lack the required blank line: `validate_profile_mode` ends its initial cancellation guard at line 148 and starts allowed-set transformation at 149; `validate_saves_inner` ends the guard at 266 and acquires entries at 267; `stage_plugin_maintenance` ends the guard at 336 and begins snapshot acquisition at 337. These are exactly the rubric’s guard/validation-to-acquisition/transformation boundaries. The minimal repair is blank-line-only, changes no interface or issue behavior, and therefore is a documented-standard blocker rather than a justified frozen-scope exception. The blank-line-only repair has now been applied.

72. `src/infrastructure/environment/src/snapshot.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/infrastructure/environment/src/snapshot.rs` — Formatting and imports / Phase spacing.** `load_inner` separates the initial cancellation guard, root/temp acquisition and recovery, canonical validation, and final construction. Sequences such as `opened_temp` → cancellation checkpoint → `.context(...)` at lines 131–135 are intentionally contiguous because they jointly perform one checked filesystem operation. The file has no issue-33 diff.

73. `src/infrastructure/environment/src/transactions.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/infrastructure/environment/src/transactions.rs` — **Application use cases and ports / Use-case parameters**. `InstallationTransaction::begin`, `begin_file`, and `publish_installation` are filesystem-transaction implementation functions, not application use cases; where supported, cancellation is already final.

74. `src/presentation/mcp/src/lifecycle.rs` — **Formatting and imports / Phase spacing**

   `src/presentation/mcp/src/lifecycle.rs` — **Formatting and imports / Phase spacing** — **ACCEPT**. `execution_finished` separates lookup/guard, lifecycle capture, and spawned cleanup; `drain` separates task shutdown/wait from capture release. Missing spacing between test items is not a function-body phase violation. The file is unchanged.

75. `src/application/src/installation/types.rs` — **Dependencies / Narrow custom implementations**

   **ACCEPT — `src/application/src/installation/types.rs` — Dependencies / Narrow custom implementations.** This file contains project-owned install/FOMOD contract data types (`FomodFileEffect`, `InstallationState`, `InstallWarning`, `InstallPlan`, etc.). It implements no custom runtime, parser, synchronization primitive, or general-purpose utility that duplicates a crate. The rubric’s suspected custom infrastructure does not exist here.

76. `src/domain/src/profile_state.rs` — **Formatting and imports / Phase spacing**

   **ACCEPT — `src/domain/src/profile_state.rs` — Formatting and imports / Phase spacing.** `profile_test_file_slots` keeps each line’s parse/filter guards together (lines 9–25), then uses the blank at 26 to separate the accepted assignment and the blank at 29 to separate final output. Extra spacing inside the guard chain would incorrectly split one parse operation. The file has no issue-33 diff.

77. `src/infrastructure/environment/src/profile_activation.rs` — **Formatting and imports / Phase spacing**

   **FIXED** — `src/infrastructure/environment/src/profile_activation.rs` — **Formatting and imports / Phase spacing**. `ProfileActivation::load` completes the INI acquisition/transformation loop at 65 and immediately constructs the output at 66. Add a blank line before `Ok(Self { active_plugins })` to separate transformation from output construction. The blank-line-only repair has now been applied.

78. `src/infrastructure/execution/src/launch_inputs/windows_inputs.rs` — **Formatting and imports / Phase spacing**

   `src/infrastructure/execution/src/launch_inputs/windows_inputs.rs` — **Formatting and imports / Phase spacing** — **ACCEPT** for issue #33. Additional blank lines in the dense baseline `new`, `resolve`, or `validated_path` could be follow-up cleanup, but the file is byte-identical to the merge base. Changing Windows launch-input code would broaden the frozen repair beyond the sole native-path boundary change and requires Windows evidence not called for by a whitespace-only issue-33 repair.

79. `src/presentation/mcp/src/contract.rs` — **Dependencies / Narrow custom implementations**

   **ACCEPT — `src/presentation/mcp/src/contract.rs` — Dependencies / Narrow custom implementations.** The code uses the established `jsonschema` Draft 2020-12 validator. Its custom layer only binds the fixed tool schemas and converts validator diagnostics into sorted, sanitized field/kind records because native diagnostics can contain supplied values (comment at line 65). It does not reimplement JSON Schema; it implements a narrow product security/output policy.

80. `src/presentation/mcp/src/main.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/presentation/mcp/src/main.rs` — Application use cases and ports / Use-case parameters.** `main` and `run(cli)` are presentation startup/composition functions; `run` creates `Resources::system`, `Server`, stdio transport, and lifecycle supervision. They are not application use cases and therefore do not take a `<UseCase>Dependencies` bundle.

81. `src/application/src/conflicts/projection.rs` — **Dependencies / Narrow custom implementations**

   **ACCEPT** — `src/application/src/conflicts/projection.rs` — **Dependencies / Narrow custom implementations**. This private application module implements project-specific provider ranking, hypothetical participation, tombstone suppression, Steam Data exclusion, UTF-16 ordering, and file/directory collision policy. It is concrete conflict-domain logic, not a general-purpose abstraction duplicated from a crate.

82. `src/application/src/execution/execute_program.rs` — **Application use cases and ports / Use-case parameters**

   `src/application/src/execution/execute_program.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. It is explicitly compliant: `ExecuteProgramDependencies` is by value first; `OutputTarget`, `WorkingDirectory`, `Program`, and `ProgramArgument` values are separate; `CancellationToken` is last.

83. `src/domain/src/profile_state.rs` — **Dependencies / Narrow custom implementations**

   **ACCEPT — `src/domain/src/profile_state.rs` — Dependencies / Narrow custom implementations.** `profile_test_file_slots` is a narrow Fallout-domain projection, not a general INI parser: it recognizes only `[General]` and case-insensitive `sTestFile1` through `sTestFile10`, keeps the last assignment, trims values, and returns a fixed ten-slot array. Rustdoc states the maintained per-INI and caller-policy invariants. This is the concrete project gap, not broad replacement infrastructure.

84. `src/infrastructure/dependencies/src/install_archive.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/dependencies/src/install_archive.rs` — Application use cases and ports / Use-case parameters.** `Resources::install_archive_dependencies(&self)` is the composition-root factory that constructs `InstallArchiveDependencies`. It is not the `install_archive` use case and has no business or cancellation arguments, so the rubric does not apply.

85. `src/infrastructure/execution/src/managed.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/infrastructure/execution/src/managed.rs` — **Application use cases and ports / Use-case parameters**. `supervise(process, cancellation, force)` is an infrastructure process-supervision primitive, not an application use case. Its two tokens deliberately represent distinct cooperative and forced signals, so the single application-use-case cancellation-order rule does not apply.

86. `src/presentation/mcp/src/server.rs` — **Dependencies / Narrow custom implementations**

   `src/presentation/mcp/src/server.rs` — **Dependencies / Narrow custom implementations** — **ACCEPT**. `Server` implements established `rmcp::ServerHandler` and delegates MCP protocol/runtime work to `rmcp`. Its custom code is only product-specific routing, admission, diagnostics, contract checking, and use-case composition—not a replacement protocol/runtime.

87. `src/presentation/mcp/src/main.rs` — **Dependencies / Narrow custom implementations**

   **ACCEPT — `src/presentation/mcp/src/main.rs` — Dependencies / Narrow custom implementations.** This is the required `mods-mcp` composition root. It delegates to Clap, Tokio, RMCP, Rootcause, `Resources`, and `DiagnosticSession`; its local future only sequences server wait, lifecycle drain, and diagnostic capture. It creates no replacement runtime, protocol, or generic infrastructure.

88. `src/presentation/mcp/src/error_output.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/presentation/mcp/src/error_output.rs` — Application use cases and ports / Use-case parameters.** `application_error(tool, report)` is a presentation error allowlist/mapper, not an application use case. Its inputs are mapper context and a report, not hidden use-case business inputs; it supports no cancellation.

89. `src/application/src/conflicts/projection.rs` — **Application use cases and ports / Use-case declaration order**

   **ACCEPT** — `src/application/src/conflicts/projection.rs` — **Application use cases and ports / Use-case declaration order**. `projection.rs` is a private shared implementation module, not a primary use-case file. Actual entry points `list_effective_conflicts.rs`, `inspect_mod_conflicts.rs`, and `explain_path.rs` each contain Dependencies -> Output -> Error -> instrumented async function in the required order; fabricating a Projection quartet would misclassify the module.

90. `src/infrastructure/execution/src/process.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/execution/src/process.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `VirtualGameView::launch` and cleanup helpers are Windows infrastructure adapter APIs, not application use cases. `LaunchRequest` is a native launch contract, not a bag hiding application business inputs. The file is unchanged.

91. `src/application/src/installation/install_archive/planning.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/application/src/installation/install_archive/planning.rs` — Application use cases and ports / Use-case parameters.** `plan_candidates` is a private use-case-local planning helper, not the `install_archive` use-case entry point. Its two planning inputs are explicit and borrowed `CancellationToken` is last (lines 27–31). It correctly has no dependency bundle.

92. `src/infrastructure/environment/src/conflict_scan.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/environment/src/conflict_scan.rs` — Application use cases and ports / Use-case parameters.** `scan(root_path, cancellation)` and `read_content(root_path, id, cancellation)` are infrastructure operations behind application-owned ports, not application use-case entry points. Their typed inputs are explicit and cancellation is already last.

93. `src/infrastructure/environment/src/lib.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/infrastructure/environment/src/lib.rs` — **Application use cases and ports / Use-case parameters**. This file implements `EnvironmentAdapter` and port factories, not application use cases. Operations such as `publish(&self, root, plan, cancellation)` and returned closures put cancellation last. The issue-33 diff adds tests only, not production signatures.

94. `src/infrastructure/environment/src/profile_activation.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/environment/src/profile_activation.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `ProfileActivation::load` is a private synchronous infrastructure helper, not an application use case; its explicit cancellation reference is outside this rule. The file is unchanged.

95. `src/infrastructure/game_platform/src/version.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/game_platform/src/version.rs` — Application use cases and ports / Use-case parameters.** `read_game_version` and `read_xnvse_version` are infrastructure adapter methods, not application use cases. Both take the receiver, explicit `GameBinding`, and final borrowed `CancellationToken` (lines 17–36).

96. `src/presentation/mcp/src/diagnostics.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/presentation/mcp/src/diagnostics.rs` — Application use cases and ports / Use-case parameters.** `DiagnosticSession::{start,capture,finish}` and `filter` are presentation logging helpers. None is an instrumented application use case or owns a use-case dependency bundle, and no cancellation token is supported.

97. `src/presentation/mcp/src/server.rs` — **Application use cases and ports / Use-case declaration order**

   **ACCEPT** — `src/presentation/mcp/src/server.rs` — **Application use cases and ports / Use-case declaration order**. This is presentation routing: `Server` owns rmcp/lifecycle/resources state and `run_tool` invokes imported application use cases. It declares no application use case, so no `<UseCase>Dependencies/Output/Error/function` quartet belongs here.

98. `src/infrastructure/environment/src/profile.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/environment/src/profile.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `stage_profile` and validation/maintenance functions are private infrastructure operations, not application use cases. This is also not the changed `infrastructure/execution/src/profile.rs`. The cited file is unchanged.

99. `src/presentation/mcp/src/main.rs` — **Application use cases and ports / Use-case declaration order**

   **ACCEPT — `src/presentation/mcp/src/main.rs` — Application use cases and ports / Use-case declaration order.** The file defines the binary entry point `main` and private bootstrap `run`, not an application `<UseCase>`. Requiring `<UseCase>Dependencies`, `<UseCase>Output`, and `<UseCase>Error` here would misclassify a presentation composition root and duplicate application ownership.

100. `src/application/src/installation/install_archive.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/application/src/installation/install_archive.rs` — Application use cases and ports / Use-case parameters.** The actual use-case signature is exactly compliant: `InstallArchiveDependencies` is passed by value first (line 84), separate typed business values follow (85–89), and `CancellationToken` is final (90). The surrounding declarations also appear in the required Dependencies/Output/Error/function order.

101. `src/application/src/settings/list_settings.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/application/src/settings/list_settings.rs` — **Application use cases and ports / Use-case parameters**. This actual use case complies exactly: `list_settings(dependencies: ListSettingsDependencies)` has the narrow dependency bundle first and sole; it has no business arguments or cancellation support.

102. `src/infrastructure/archive/src/adapter.rs` — **Application use cases and ports / Use-case declaration order**

   `src/infrastructure/archive/src/adapter.rs` — **Application use cases and ports / Use-case declaration order** — **ACCEPT**. `ArchiveAdapter` supplies implementations of application-owned ports; it declares no `<UseCase>Dependencies`/`Output`/`Error`/function quartet, so the declaration-order rule cannot apply. The file is unchanged.

103. `src/infrastructure/dependencies/src/execute_program.rs` — **Application use cases and ports / Use-case declaration order**

   **ACCEPT — `src/infrastructure/dependencies/src/execute_program.rs` — Application use cases and ports / Use-case declaration order.** This infrastructure factory only builds application-owned `ExecuteProgramDependencies`. The actual `src/application/src/execution/execute_program.rs` already declares `ExecuteProgramDependencies`, `ExecuteProgramOutput`, `ExecuteProgramError`, then instrumented `execute_program` in the required order. Duplicating those primary items in the factory would reverse ownership.

104. `src/infrastructure/dependencies/src/inspect_mod_conflicts.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/dependencies/src/inspect_mod_conflicts.rs` — Application use cases and ports / Use-case parameters.** `Resources::inspect_mod_conflicts_dependencies(&self)` is a composition-root factory that constructs `InspectModConflictsDependencies`; it is not the use case and has no business or cancellation inputs.

105. `src/infrastructure/dependencies/src/list_effective_conflicts.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/infrastructure/dependencies/src/list_effective_conflicts.rs` — **Application use cases and ports / Use-case parameters**. `Resources::list_effective_conflicts_dependencies(&self)` is a composition-root factory that assembles a dependency bundle, not the application use case. Its only argument is the receiver; the real signature is in application/conflicts.

106. `src/infrastructure/dependencies/src/set_game_directory.rs` — **Application use cases and ports / Use-case parameters**

   `src/infrastructure/dependencies/src/set_game_directory.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT**. `Resources::set_game_directory_dependencies` is a composition-root factory that constructs the dependency bundle. It is not the application `set_game_directory` function and has no business or cancellation parameters to order. The file is unchanged.

107. `src/infrastructure/settings/src/lib.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/infrastructure/settings/src/lib.rs` — Application use cases and ports / Use-case parameters.** This is an infrastructure adapter/port-factory module, not an application use-case module. Relevant adapter methods keep typed business inputs explicit and cancellation final where present—for example `store_game_binding(&self, binding, cancellation)` (lines 181–185). Requiring a use-case dependency bundle here would invert the application/infrastructure seam.

108. `src/presentation/mcp/src/contract.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT — `src/presentation/mcp/src/contract.rs` — Application use cases and ports / Use-case parameters.** `Contracts::{new,output_matches,tools,validate}` are presentation schema-registry methods, not application use cases. They have no `<UseCase>Dependencies` bundle or cancellation contract, so the rubric does not apply.

109. `src/presentation/mcp/src/install_output.rs` — **Application use cases and ports / Use-case parameters**

   **ACCEPT** — `src/presentation/mcp/src/install_output.rs` — **Application use cases and ports / Use-case parameters**. `additional_selections`, `preview`, and `installed` are MCP JSON serializers over typed application results. They are synchronous presentation helpers with no dependency bundle/cancellation. The issue-33 diff changes the candidate discriminator and contract coverage, not parameter ordering.

110. `src/application/src/installation/mod.rs` — **Application use cases and ports / Use-case declaration order**

   `src/application/src/installation/mod.rs` — **Application use cases and ports / Use-case declaration order** — **ACCEPT**. The entries here are re-exports, not primary declarations. The actual primary items in `installation/install_archive.rs` are in the mandated order: `InstallArchiveDependencies` line 53, `InstallArchiveOutput` line 67, `InstallArchiveError` line 74, then instrumented `install_archive` line 83. The façade file is unchanged.

111. `src/infrastructure/execution/src/usvfs/native_path.rs` — **Formatting and imports / Phase spacing**

   **FIXED; residual signal ACCEPTED as a false positive** — Spacing now separates UTF-16 acquisition from validation, path acquisition from namespace validation, component transformation from device-name validation, and test arrangement from assertions. The drive predicate remains adjacent to the `if` that consumes it because those statements jointly perform one validation. `cargo test -p infrastructure-execution native_path --all-features` and `cargo fmt --all -- --check` pass.

## Repeated worktree-path findings

A later enforce-mode scan repeated four already-disposed findings under absolute worktree paths. They are not new rules or code sites:

- `src/presentation/mcp/src/execution_output.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT** under disposition 5: `executed` is a presentation serializer, not an application use case.
- `src/presentation/mcp/src/server.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT** under disposition 50: `Server::run_tool` is a presentation dispatcher; its real use-case calls pass dependency bundles first and cancellation last.
- `src/application/src/installation/install_archive.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT** under disposition 103: the actual use case passes `InstallArchiveDependencies` first, explicit business inputs next, and `CancellationToken` last.
- `src/infrastructure/archive/src/adapter.rs` — **Application use cases and ports / Use-case parameters** — **ACCEPT** under disposition 52: this is an infrastructure port implementation/factory, not an application use-case declaration.
