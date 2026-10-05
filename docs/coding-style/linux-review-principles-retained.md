# Coding style: Linux review principles retained

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

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
