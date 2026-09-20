# Coding style

This document is the repository authority for handwritten Rust. Apply it during implementation and review.

Each style item is a rubric. `Rule` is normative. `Violation` and `Compliant` define the decision boundary. Good and bad examples are representative rather than exhaustive. The coding-style gate submits every rubric item to Jev; formatting, compilation, lint, dependency, layout, and repository checks remain independent sources of deterministic evidence.

## Authority

Resolve conflicts in this order:

1. Rust correctness and soundness.
2. Repository architecture, domain rules, specifications, and accepted decisions.
3. `rustfmt`.
4. Workspace rustc and Clippy policy.
5. Rust API Guidelines.
6. The Linux review principles retained below.

## Comments and documentation

### Readability before secondary cleanup


#### Rule

Readability is the first implementation and review priority. Improve difficult-to-read code before secondary cleanup or optimization.

#### Violation

The change optimizes, deduplicates, or abstracts code while leaving its main control flow or intent harder to read.

#### Compliant

The change first makes the operation easy to follow through clear control flow, names, types, responsibilities, and spacing.

#### Bad example

```rust
let output = condition.then(|| transform(input)).transpose()?.unwrap_or_default();
```

#### Good example

```rust
if !condition {
	return Ok(Output::default());
}

let output = transform(input)?;
```

### Self-explanatory code


#### Rule

Code explains itself through precise names, expressive types, cohesive responsibilities, guard clauses, and deliberate spacing. Rewrite unclear code instead of using a comment to excuse it.

#### Violation

A new comment compensates for vague names, tangled control flow, or mixed responsibilities that can be made clear in code.

#### Compliant

The code is understandable without a narration comment; any comment records information the code cannot express.

#### Bad example

```rust
// Check whether x can be used.
if x != 0 && x < max {
	use_value(x);
}
```

#### Good example

```rust
let value_is_usable = value != 0 && value < maximum;
if value_is_usable {
	use_value(value);
}
```

### Reason comments


#### Rule

Add comments for external constraints, safety proofs, compatibility workarounds, and intentionally surprising decisions. Explain why the constraint exists rather than narrating the code.

#### Violation

An added comment merely restates the adjacent operation or describes obvious syntax without giving a necessary reason.

#### Compliant

The comment explains an external constraint, proof, workaround, or surprising decision that the code alone cannot communicate.

#### Bad example

```rust
// Add one to the retry count.
retry_count += 1;
```

#### Good example

```rust
// Steam reports one-based attempts, so preserve the offset in diagnostics.
retry_count += 1;
```


### Rustdoc format

#### Rule

Write comments that document Rust items with Rustdoc syntax and conventions. Use `///` for the following item and `//!` for the containing module or crate. Start with a concise summary paragraph, use Markdown for code and links, and add conventional sections such as `# Errors`, `# Panics`, or `# Safety` when they apply. Keep implementation rationale in ordinary `//` comments.

#### Violation

A comment intended to document a Rust item or module uses ordinary `//` syntax, uses the wrong inner or outer Rustdoc form, or presents applicable API contracts as unstructured prose instead of Rustdoc Markdown.

#### Compliant

Item and module documentation uses the correct `///` or `//!` form and Rustdoc Markdown, while comments about local implementation decisions remain ordinary `//` comments.

#### Bad example

```rust
// Installs an archive and returns an error when extraction fails.
pub async fn install_archive(input: InstallArchiveInput) -> Result<InstallArchiveOutput, InstallArchiveError> {
	// The provider requires paths to be normalized before extraction.
	normalize_and_install(input).await
}
```

#### Good example

```rust
/// Installs an archive.
///
/// # Errors
///
/// Returns [`InstallArchiveError`] when extraction fails.
pub async fn install_archive(input: InstallArchiveInput) -> Result<InstallArchiveOutput, InstallArchiveError> {
	// The provider requires paths to be normalized before extraction.
	normalize_and_install(input).await
}
```

## Formatting and imports

### Import placement and use


#### Rule

Keep imports at module scope. Import a normal item once and use its local name. Use a qualified call-site path only for disambiguation or macro syntax. Put conditional imports at module scope with `#[cfg(...)]`.

#### Violation

The change adds a block-local import, repeats a normal qualified path without need, or places a conditional import inside a function.

#### Compliant

Imports are declared once at module scope, with call-site qualification reserved for a concrete ambiguity or macro requirement.

#### Bad example

```rust
fn load() {
	use std::fs::read_to_string;
	let value = std::fs::read_to_string("settings.toml");
}
```

#### Good example

```rust
use std::fs::read_to_string;

fn load() {
	let value = read_to_string("settings.toml");
}
```

### Phase spacing


#### Rule

Partition a function body into semantic blocks. Use one blank line when responsibility changes between guards or validation, acquisition or recovery, transformation or staging, side effects or publication, and output construction. Keep adjacent statements together when they jointly perform one operation.

#### Violation

The change runs distinct semantic phases together with no blank line, or separates statements that jointly perform one operation as though they were different phases.

#### Compliant

One blank line marks each real responsibility change, while statements belonging to the same operation remain contiguous.

#### Bad example

```rust
validate(&input)?;
let staged = stage(input)?;
publish(staged)?;
let output = Output::new();
```

#### Good example

```rust
validate(&input)?;

let staged = stage(input)?;

publish(staged)?;

let output = Output::new();
```

## Architecture and modules

### Dependency direction and composition roots


#### Rule

Preserve `presentation -> application -> domain`. Infrastructure implements application-owned ports. Framework, transport, filesystem, Windows API, and provider types stay outside domain and application. The root is a virtual workspace. Binary presentation packages own `mods.exe` and `mods-mcp.exe` plus their composition roots.

#### Violation

Dependency or workspace checks find a reversed layer dependency, a forbidden external type, an unexpected root package, or a misplaced binary composition root.

#### Compliant

The workspace dependency graph and package inventory match the approved direction and ownership.

#### Bad example

```rust
// src/domain/src/path.rs
use windows::Win32::Storage::FileSystem::WIN32_FILE_ATTRIBUTE_DATA;
```

#### Good example

```rust
// src/infrastructure/environment/src/path.rs
use windows::Win32::Storage::FileSystem::WIN32_FILE_ATTRIBUTE_DATA;
```

### Capability modules and public APIs


#### Rule

Organize each layer by capability. Keep leaf modules private by default and re-export a deliberate public API from the parent module.

#### Violation

The change adds a generic dumping-ground module, exposes a leaf module without need, or makes internal implementation types broadly public.

#### Compliant

The module is named for one capability, leaf modules remain private, and the parent re-exports only the intended API.

#### Bad example

```rust
pub mod utils;
pub mod internal_parser;
```

#### Good example

```rust
mod archive_path;

pub use archive_path::ArchivePath;
```

## Dependencies

### Prefer established crates


#### Rule

Prefer popular, actively maintained crates with clear ownership and documentation when they satisfy the requirement. Reuse their established abstractions instead of building a project-owned equivalent.

#### Violation

The change implements a general-purpose runtime, protocol, parser, synchronization primitive, or utility already supplied by an approved dependency, without identifying a concrete gap.

#### Compliant

The change uses an established crate abstraction, or records a concrete reason no suitable maintained crate satisfies the requirement.

#### Bad example

```rust
struct AsyncChannel<T> {
	queue: Mutex<VecDeque<T>>,
	waiters: Vec<Waker>,
}
```

#### Good example

```rust
let (sender, receiver) = tokio::sync::mpsc::channel(capacity);
```

### Narrow custom implementations


#### Rule

Write custom code only for a concrete domain, safety, compatibility, or platform gap that available crates do not close cleanly. Keep it narrow and document why the established solution is insufficient.

#### Violation

Custom infrastructure is broad, lacks a stated concrete gap, or reimplements unrelated parts of an existing abstraction.

#### Compliant

The custom code is limited to the uncovered capability and documents the exact gap and maintained invariants.

#### Bad example

```rust
struct ProjectRuntime {
	threads: Vec<Thread>,
	timers: TimerWheel,
	io: IoDriver,
}
```

#### Good example

```rust
// notify does not report rename pairs on this Windows API path, so this adapter
// correlates only the two records required by profile recovery.
struct RenamePairCorrelator { /* narrow state */ }
```

## Application use cases and ports

### Use-case declaration order


#### Rule

Declare `<UseCase>Dependencies`, `<UseCase>Output`, `<UseCase>Error`, then the `#[tracing::instrument(skip_all)]` async use-case function.

#### Violation

A syntax-aware declaration-order check reports that a primary item is absent or out of order.

#### Compliant

All four primary items exist in the required order.

#### Bad example

```rust
pub async fn install_archive() { }
pub struct InstallArchiveDependencies;
```

#### Good example

```rust
pub struct InstallArchiveDependencies;
pub struct InstallArchiveOutput;
pub struct InstallArchiveError;

#[tracing::instrument(skip_all)]
pub async fn install_archive() { }
```

### Use-case parameters


#### Rule

Pass the narrow dependency bundle by value first. Keep business arguments as separate typed values. Pass `tokio_util::sync::CancellationToken` last when cancellation is supported.

#### Violation

Dependencies are not the first value, business inputs are hidden in an unrelated bag, or `CancellationToken` is not the final parameter.

#### Compliant

The dependency bundle comes first, business values remain explicit and typed, and cancellation is last.

#### Bad example

```rust
async fn install(path: PathBuf, cancellation: CancellationToken, dependencies: Dependencies) { }
```

#### Good example

```rust
async fn install(dependencies: Dependencies, path: PathBuf, cancellation: CancellationToken) { }
```

### Callable port invocation


#### Rule

Application-owned callable ports use the approved nightly `fn_traits` feature and are invoked explicitly with `.call(())`, `.call((argument,))`, or `.call((argument_one, argument_two))`.

#### Violation

Compilation or a syntax-aware check finds direct function-call syntax for an application-owned callable port.

#### Compliant

The port uses the approved callable trait and explicit tuple-shaped `.call(...)` invocation.

#### Bad example

```rust
let archive = dependencies.open_archive(path).await?;
```

#### Good example

```rust
let archive = dependencies.open_archive.call((path,)).await?;
```

### Focused use-case orchestration


#### Rule

Keep a use-case file focused on its types and top-to-bottom orchestration. Move production functions for repeated callers to private sibling modules named for their responsibility. Use generic utilities only for genuinely cross-capability primitives with no clearer home.

#### Violation

The use-case file accumulates a reusable algorithm, or reusable code moves into a vague `utils` module instead of a precise capability module.

#### Compliant

The use case reads as orchestration; repeated production behavior lives in a private, responsibility-named sibling.

#### Bad example

```rust
mod utils;

fn normalize_every_archive_path(path: &Path) -> PathBuf { /* reusable algorithm */ }
```

#### Good example

```rust
mod archive_path;

use archive_path::normalize_archive_path;
```


### Use-case-local implementation modules


#### Rule

When nontrivial production logic serves exactly one use case and earns extraction, place it in a private child module under the use case's module directory. Name the child for its responsibility, not `lib`, `utils`, or `common`. Keep shared public types in the capability interface. Promote implementation logic to a private capability sibling only when repeated callers use it.

#### Violation

`state.change_kind` is `added` or `modified`, `state.file` is under `src/application/src/`, the file contains nontrivial production logic, `state.declares_file_named_entry_point` is false, the file is a helper or implementation module rather than the owning use-case entry point or a capability interface module such as `mod.rs`, `types.rs`, or `ports.rs`, the file sits directly in a capability directory, and `state.module_referencing_files` shows only the capability's module declaration plus one owning use-case file. A generic `lib`, `utils`, or `common` path or exposure outside the use-case parent is also a violation.

#### Compliant

The owning use-case entry-point file itself remains directly in the capability directory. It normally declares a public function matching the file name, which makes `state.declares_file_named_entry_point` true. Its single-use helpers are nested under its same-named directory. A precise private capability sibling is also compliant when `state.module_referencing_files` shows repeated use-case callers. Deleting an old capability-level helper while moving it under its owning use case is compliant. Empty placeholder files contain no production logic and are outside this rule. The rule does not apply outside `src/application/src/`. Capability interface modules such as `mod.rs`, `types.rs`, and `ports.rs` remain at capability level; shared public types remain there.

#### Bad example

```text
file: src/application/src/installation/fomod.rs
module_referencing_files:
  - src/application/src/installation/mod.rs
  - src/application/src/installation/install_archive.rs
```

#### Good example

```text
file: src/application/src/installation/install_archive/fomod.rs
module_referencing_files:
  - src/application/src/installation/install_archive.rs

or the owning use-case entry point:

file: src/application/src/installation/install_archive.rs
patch: pub async fn install_archive(dependencies: InstallArchiveDependencies, input: InstallArchiveInput) -> Result<InstallArchiveOutput, InstallArchiveError>

or a capability interface module:

file: src/application/src/installation/types.rs
patch: pub struct InstallArchiveInput; pub struct InstallArchiveOutput;
```

## Errors

### Rootcause lower-layer results


#### Rule

Rootcause 0.13 is the only lower-layer error system. Prefer `rootcause::Result<Value, Context>` for fallible domain, application, and infrastructure APIs.

#### Violation

Dependency or syntax checks find a second lower-layer error framework or an unapproved fallible API shape.

#### Compliant

Lower layers use Rootcause results and contexts consistently.

#### Bad example

```rust
fn load() -> anyhow::Result<Value> { /* ... */ }
```

#### Good example

```rust
fn load() -> rootcause::Result<Value, LoadValueError> { /* ... */ }
```

### Preserve causes at owned boundaries


#### Rule

Create a report when an error first enters an owned boundary. Preserve external and lower-layer causes, and add a new ownership context with `ResultExt::context(...)`.

#### Violation

The change discards an underlying error, replaces it with a marker alone, or formats it into a string instead of preserving the cause tree.

#### Compliant

The original cause remains in the report and the current boundary adds its own typed context.

#### Bad example

```rust
read_value(key).await.map_err(|_| ReadSettingError)?;
```

#### Good example

```rust
let value = dependencies
	.read_value
	.call((key,))
	.await
	.context(ReadSettingError)?;
```

### Context propagation


#### Rule

Give each use case one fixed outer context. Keep semantic markers in the child report tree and inspect them through Rootcause traversal at presentation boundaries. Use `.into_report()` when no new context is needed. Do not use `context_transform` for boundary propagation.

#### Violation

Boundary propagation replaces an existing context, adds multiple competing outer contexts, or loses semantic child markers.

#### Compliant

The fixed outer context remains stable and propagation preserves the existing report tree.

#### Bad example

```rust
operation().context_transform(InstallArchiveError)?;
```

#### Good example

```rust
operation().into_report()?;
```

### Presentation error allowlists


#### Rule

Presentation maps reports to allowlisted output. Raw report formatting never crosses a presentation boundary.

#### Violation

A presentation response, terminal message, or transport payload includes raw `Debug`, `Display`, or report-tree formatting.

#### Compliant

Presentation traverses known markers and emits only explicit, allowlisted fields and messages.

#### Bad example

```rust
response.error = format!("{report:?}");
```

#### Good example

```rust
response.error = map_report_to_public_error(&report);
```

## Runtime primitives

### Primitive selection order


#### Rule

Choose runtime, asynchronous, synchronization, signaling, and cancellation primitives in this order: Tokio or Tokio Util, the Rust standard library, then a project-owned implementation.

#### Violation

The change adds a project-owned runtime primitive while a Tokio or standard-library primitive satisfies the stated behavior.

#### Compliant

The highest-priority suitable primitive is used, or a concrete missing capability is documented.

#### Bad example

```rust
struct ProjectCancellation {
	cancelled: AtomicBool,
	waiters: Mutex<Vec<Waker>>,
}
```

#### Good example

```rust
use tokio_util::sync::CancellationToken;
```

### Custom primitive justification


#### Rule

Build a project-owned primitive only for a concrete capability or correctness gap. Document the gap and maintained invariants. Continue using ordinary standard-library data types when no Tokio runtime behavior is involved.

#### Violation

A custom primitive lacks a concrete gap or invariant description, or replaces an ordinary data type without runtime behavior.

#### Compliant

The code identifies the exact missing behavior and states the invariants its narrow implementation preserves.

#### Bad example

```rust
struct ProjectCounter(AtomicUsize);
```

#### Good example

```rust
// Tokio has no primitive that atomically publishes this two-file recovery point.
// `committed` becomes true only after both durable renames complete.
struct RecoveryPublication { /* narrow state */ }
```

### Cancellation propagation and checkpoints


#### Rule

Pass `CancellationToken` directly through application-owned ports to infrastructure operations that support cancellation. Infrastructure owns cooperative checks. Write each checkpoint inline with `cancellation.is_cancelled()` and return the typed cancellation error immediately; do not introduce a cancellation-check helper.

#### Violation

The token is wrapped or replaced, application code owns infrastructure checkpoints, or a helper hides a one-line cancellation check.

#### Compliant

The original token reaches infrastructure and each checkpoint is an inline early return with the typed error.

#### Bad example

```rust
fn check_cancelled(cancellation: &CancellationToken) -> Result<()> { /* one check */ }
```

#### Good example

```rust
if cancellation.is_cancelled() {
	return Err(CopyArchiveCancelled.into_report());
}
```

### Cancellation state preservation


#### Rule

On cancellation, preserve partial filesystem state without cleanup, settlement, or rollback. If the final irreversible mutation already completed, return success.

#### Violation

A cancellation path removes partial output, rolls back completed work, settles state, or reports cancellation after the final irreversible mutation succeeded.

#### Compliant

Cancellation returns immediately with partial state intact, while a completed final mutation returns success.

#### Bad example

```rust
if cancellation.is_cancelled() {
	remove_dir_all(staging).await?;
	return Err(Cancelled.into_report());
}
```

#### Good example

```rust
if published {
	return Ok(output);
}
if cancellation.is_cancelled() {
	return Err(Cancelled.into_report());
}
```

### Separate progress reporting


#### Rule

Add a separate narrow progress-reporting port when presentation needs progress. Do not bundle progress reporting with cancellation.

#### Violation

One port or callback combines cancellation control with progress events.

#### Compliant

Cancellation remains a `CancellationToken` and progress uses an independent narrow port.

#### Bad example

```rust
trait OperationControl {
	fn is_cancelled(&self) -> bool;
	fn report_progress(&self, completed: u64);
}
```

#### Good example

```rust
async fn copy(progress: ReportProgress, cancellation: CancellationToken) { /* ... */ }
```

## Control flow

### Choose the narrow conditional form


#### Rule

Use `if` for boolean conditions, `let ... else` for one required pattern with an exiting failure path, and `if let` when behavior depends on one relevant pattern.

#### Violation

The change uses a broader or more nested construct where one of the narrow conditional forms expresses the same logic clearly.

#### Compliant

The conditional form matches the shape of the decision without unnecessary arms or nesting.

#### Bad example

```rust
match enabled {
	true => run(),
	false => {},
}
```

#### Good example

```rust
if enabled {
	run();
}
```

### Guard clauses


#### Rule

Prefer guard clauses that return, continue, or break early. Keep the successful path unnested. Use `else` only when both branches have distinct continuing behavior.

#### Violation

The successful path is nested under a condition whose opposite branch exits, or an `else` follows a branch that already returns, continues, or breaks.

#### Compliant

Exiting cases appear first as guards; `else` remains only when both branches continue differently.

#### Bad example

```rust
if ready {
	publish();
} else {
	return Err(NotReady);
}
```

#### Good example

```rust
if !ready {
	return Err(NotReady);
}

publish();
```

### Match only for multi-way logic


#### Rule

Reserve `match` for genuinely multi-way logic, meaningful exhaustive handling, or value mapping clearer than a conditional. Replace one-pattern or simple two-arm matches with `if`, `if let`, or `let ... else` when possible.

#### Violation

A new `match` has one relevant pattern or a simple boolean/two-arm shape that a narrow conditional expresses more clearly.

#### Compliant

The match performs meaningful multi-way exhaustive logic or clear value mapping.

#### Bad example

```rust
match value {
	Some(value) => use_value(value),
	None => {},
}
```

#### Good example

```rust
if let Some(value) = value {
	use_value(value);
}
```

## Functions and tests

### Cohesive orchestration


#### Rule

Prefer cohesive orchestration functions that read from start to finish, with blank lines between phases. Use `initialize_environment` as the reference shape. Function length alone does not justify extraction. Keep one-use validation, transformation, and control flow inline when that preserves the narrative.

#### Violation

The change fragments a readable one-use operation solely to shorten a function, obscuring its top-to-bottom flow.

#### Compliant

The orchestration remains cohesive and phase-oriented; extraction occurs only for a meaningful seam or abstraction.

#### Bad example

```rust
fn install() {
	validate_for_install();
	stage_for_install();
	publish_for_install();
}
```

#### Good example

```rust
fn install(dependencies: Dependencies, request: InstallRequest) {
	validate_archive_path(&request.archive_path)?;

	let staged = dependencies.stage.call((request,))?;
	dependencies.publish.call((staged,))?;
}
```

### Helpers earn an interface


#### Rule

Extract a helper only for reused logic, a separately meaningful algorithm, a safety or FFI boundary, or a real adapter seam. Each helper hides nontrivial complexity; a helper that only renames one expression, condition, or call does not earn an interface.

#### Violation

A new one-use helper merely forwards a call, renames a condition, or wraps one expression without hiding complexity.

#### Compliant

The helper is reused or owns a meaningful algorithm, safety boundary, or adapter seam.

#### Bad example

```rust
fn is_cancelled(cancellation: &CancellationToken) -> bool {
	cancellation.is_cancelled()
}
```

#### Good example

```rust
fn validate_archive_paths(entries: &[ArchiveEntry]) -> Result<ValidatedPaths> {
	// Owns traversal, collision, and platform-reserved-name checks.
}
```

### Pre-MVP test placement


#### Rule

Before MVP, write only unit tests in a `#[cfg(test)]` module in the same source file as the behavior. Do not add separate `tests/` targets, end-to-end tests, or cross-file test modules. Keep fixture helpers in the colocated test module.

#### Violation

A repository layout check finds a new Rust test target or cross-file test module outside the allowed colocated module.

#### Compliant

New tests and their fixtures are colocated with the owned behavior.

#### Bad example

```text
src/application/tests/install_archive.rs
```

#### Good example

```rust
#[cfg(test)]
mod tests {
	use super::*;
}
```

### Test public behavior


#### Rule

Test behavior through public seams where practical. Focus on project-owned business behavior and boundaries. Trust dependency contracts; cover dependency details only for policy, an unstable adapter edge, or a focused regression. Prefer small wiring tests over recreated dependency suites.

#### Violation

A test couples to private implementation details or recreates a dependency's suite without a project-owned policy or regression reason.

#### Compliant

The test observes owned behavior through a public seam and uses only the wiring needed to protect a project boundary.

#### Bad example

```rust
#[test]
fn dependency_parser_handles_every_internal_token_state() { /* copied suite */ }
```

#### Good example

```rust
#[test]
fn install_rejects_archive_paths_that_escape_the_profile() { /* public seam */ }
```

## Unsafe Rust

### Unsafe layer confinement


#### Rule

Forbid unsafe code in domain, application, and presentation crates. Keep unsafe code in dedicated infrastructure or FFI modules behind safe interfaces.

#### Violation

A syntax or lint check finds `unsafe` in a forbidden layer or outside a dedicated infrastructure/FFI boundary.

#### Compliant

Unsafe operations are confined to a dedicated allowed module with a safe outward interface.

#### Bad example

```rust
// src/domain/src/path.rs
let value = unsafe { external_value() };
```

#### Good example

```rust
// src/infrastructure/windows/src/file_version.rs
pub fn file_version(path: &Path) -> Result<FileVersion> {
	let value = unsafe { read_version_resource(path)? };
	Ok(value)
}
```

### Safety proofs


#### Rule

Put a `// SAFETY:` proof immediately before every unsafe block. State API requirements, caller obligations, ownership and lifetime invariants, and how the safe wrapper maintains them. Public unsafe functions also require a rustdoc `# Safety` section.

#### Violation

An unsafe block lacks an immediate proof, the proof merely restates the operation, or a public unsafe function lacks caller obligations.

#### Compliant

The immediate proof addresses the relevant requirements and invariants; public unsafe APIs document their safety contract.

#### Bad example

```rust
// SAFETY: This call is safe.
let handle = unsafe { OpenFile(path) };
```

#### Good example

```rust
// SAFETY: `wide_path` is NUL-terminated and lives through the call. The returned
// handle is checked for INVALID_HANDLE_VALUE and is closed by `OwnedHandle`.
let handle = unsafe { OpenFile(wide_path.as_ptr()) };
```

## Linux review principles retained

### Language-neutral review priorities


#### Rule

Retain readable control flow, cohesive responsibilities, precise names, reason comments, explicit failure handling, established abstractions, restrained low-level optimization, and recovery from expected failures.

#### Violation

The change knowingly trades one of these review priorities for terseness, novelty, or premature optimization without a higher-priority requirement.

#### Compliant

The change favors readable, cohesive, explicit, established, and recoverable code unless a higher authority requires otherwise.

#### Bad example

```rust
let _ = operation(); // Ignore an expected failure to keep the fast path short.
```

#### Good example

```rust
if let Err(error) = operation() {
	recover_expected_failure(error)?;
}
```

### Rust conventions over Linux C idioms


#### Rule

Rust conventions replace remaining Linux C implementation conventions. Do not transfer cleanup `goto`, integer error codes, kernel-doc, preprocessor patterns, allocator spelling, or GNU C assembly syntax.

#### Violation

Review finds a Linux C implementation convention copied into handwritten Rust.

#### Compliant

The code uses Rust control flow, results, documentation, configuration, allocation, and assembly conventions.

#### Bad example

```rust
fn load() -> i32 {
	// Returns -EINVAL on failure.
	-22
}
```

#### Good example

```rust
fn load() -> Result<Value, LoadError> {
	Err(LoadError.into_report())
}
```
