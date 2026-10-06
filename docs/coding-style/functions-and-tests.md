# Coding style: Functions and tests

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

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
