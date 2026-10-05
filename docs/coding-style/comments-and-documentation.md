# Coding style: Comments and documentation

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

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

Add comments for external constraints, safety proofs, compatibility workarounds, and intentionally surprising decisions. Explain why the constraint exists rather than narrating the code. This applies to ordinary comments and Rustdoc: documentation syntax alone does not justify a comment. Keep API contracts and non-obvious semantics, but omit standalone summaries that only repeat item names, fields, signatures, or implementation steps.

#### Violation

An added ordinary comment or Rustdoc summary only restates the adjacent operation, item name, fields, signature, or obvious syntax. It adds no external constraint, safety proof, caller obligation, non-obvious semantics, or necessary rationale. A tautological Rustdoc summary is still a violation even when formatted correctly.

#### Compliant

The comment explains an external constraint, proof, workaround, surprising decision, or API contract that the code alone cannot communicate. Preserve meaningful error conditions, sentinel meanings, ownership and lifetime obligations, and non-obvious return semantics. A concise introductory summary accompanying such a contract is acceptable; do not flag it merely because the item name is descriptive.

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
