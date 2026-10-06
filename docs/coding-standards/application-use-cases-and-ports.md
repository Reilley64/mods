# Coding standards: Application use cases and ports

This file is one area of the [coding standards](../../CODING_STANDARDS.md). Resolve conflicts between rules with its Authority order.

## Application use cases and ports

### Use-case declaration order

#### Applies to

- `src/application/**`

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

#### Applies to

- `src/application/**`

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

### Reusable capability ports

#### Applies to

- `src/application/**`

#### Rule

Name a port and its input and output types for the capability they provide, not for the use case that first consumed them. A port does one step, so use cases compose ports instead of receiving a combined workflow. When a second use case needs the same step, it reuses the existing port. It does not get a use-case-specific copy, and it does not get a port that runs the whole workflow inside infrastructure. Keep use-case-specific steps in their own ports next to the shared ones.

#### Violation

A port or its types are named after one use case even though the step is general. A port bundles several steps that another use case also needs. Or a second use case adds a near-duplicate port instead of reusing the existing one.

#### Compliant

Shared steps are neutral, single-step ports that more than one use case can compose. Only steps that one use case alone needs are specific to that use case.

#### Bad example

```rust
pub struct ExportEnvironmentDependencies {
	// Prepares, lists, and returns a publisher in one infrastructure call.
	pub prepare_export: PrepareExport,
}

pub struct ExecuteProgramDependencies {
	pub prepare_launch_plan: PrepareLaunchPlan,
}
```

#### Good example

```rust
pub struct ExportEnvironmentDependencies {
	pub prepare_environment_plan: PrepareEnvironmentPlan,
	pub project_profile: ProjectProfile,
	pub list_export_files: ListExportFiles,
	pub write_export: WriteExport,
}

pub struct ExecuteProgramDependencies {
	pub prepare_environment_plan: PrepareEnvironmentPlan,
	pub project_profile: ProjectProfile,
	pub create_virtual_file_system: CreateVirtualFileSystem,
}
```


### Focused use-case orchestration

#### Applies to

- `src/application/**`

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

#### Applies to

- `src/application/**`

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
