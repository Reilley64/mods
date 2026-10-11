# Coding standards: Unsafe Rust

This file is one area of the [coding standards](../../CODING_STANDARDS.md). Resolve conflicts between rules with its Authority order.

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
