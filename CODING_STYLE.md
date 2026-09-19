# Coding style

This document is the repository authority for handwritten Rust. Apply it during implementation and review.

## Comments and documentation

Readability is the project's first implementation and review priority. When code is difficult to read, improve its readability before secondary cleanup or optimization.

Readable code explains itself through clear top-to-bottom flow, precise names, expressive types, cohesive responsibilities, guard clauses, and deliberate spacing. Comments do not make unclear code acceptable. Rewrite unclear code first.

Add comments for external constraints, safety proofs, compatibility workarounds, and intentionally surprising decisions. Explain why the constraint exists rather than narrating the code.

## Authority

Resolve conflicts in this order:

1. Rust correctness and soundness.
2. Repository architecture, domain rules, specifications, and accepted decisions.
3. `rustfmt`.
4. Workspace rustc and Clippy policy.
5. Rust API Guidelines.
6. The Linux review principles retained below.

## Formatting and imports

Format handwritten Rust with the workspace `rustfmt.toml`: hard tabs at eight-column stops, a 120-column maximum width, one imported item per `use` line, and one contiguous import group.

Keep imports at module scope. Import a normal item once and use its local name in the module. Use a qualified path at the call site only when it is required for disambiguation or macro syntax. Put conditional imports at module scope with `#[cfg(...)]`.

Use blank lines to separate logical phases, such as validation, recovery, staging, publication, and output construction. Keep one thought together. Do not add decorative spacing inside a fluent expression or tightly coupled operation.

## Architecture and modules

Preserve the dependency direction `presentation -> application -> domain`. Infrastructure implements application-owned ports. Framework, transport, filesystem, Windows API, and provider types stay outside domain and application code. The repository root is a virtual workspace. Each binary-only presentation package owns its executable and composition root: `mods.exe` for the CLI and `mods-mcp.exe` for MCP.

Organize each layer by capability. Keep leaf modules private by default and re-export a deliberate public API from the parent module.

## Dependencies

Prefer popular, actively maintained crates with clear ownership and documentation when they satisfy the requirement. Reuse their established abstractions instead of building a project-owned equivalent.

Write custom code only for a concrete domain, safety, compatibility, or platform gap that available crates do not close cleanly. Keep that code narrow and document why the established crate solution is insufficient.

## Application use cases and ports

Each use-case file declares these primary items in order:

1. `<UseCase>Dependencies`;
2. `<UseCase>Output`;
3. `<UseCase>Error`;
4. the `#[tracing::instrument(skip_all)]` async use-case function.

Pass the narrow dependency bundle by value first. Keep business arguments as separate typed values. Pass `tokio_util::sync::CancellationToken` last when the use case supports cancellation.

Application-owned callable ports use the approved nightly `fn_traits` feature. Invoke them explicitly with `.call(())`, `.call((argument,))`, or `.call((argument_one, argument_two))`.

Keep a use-case file focused on its types and top-to-bottom orchestration. When a production function exists to serve repeated callers, move it to a private sibling module named for the responsibility it owns. Prefer a precise capability name over a generic `utils` module; use a generic utility module only for a genuinely cross-capability primitive with no clearer home.

## Errors

Rootcause 0.13 is the only lower-layer error system. Prefer `rootcause::Result<Value, Context>` for fallible domain, application, and infrastructure APIs.

Create a report when the error first enters an owned boundary. Preserve external and lower-layer causes. Add a new ownership context with `ResultExt::context(...)`:

```rust
let value = dependencies
    .read_value
    .call((key,))
    .await
    .context(ReadSettingError)?;
```

Each use case has one fixed outer context. Keep semantic error markers in the child report tree. Inspect them through Rootcause traversal at presentation boundaries. Use `.into_report()` when no new context is needed. Do not replace a context with `context_transform` during boundary propagation.

Presentation maps reports to allowlisted output. Raw report formatting never crosses a presentation boundary.

## Runtime primitives

Choose runtime, asynchronous, synchronization, signaling, and cancellation primitives in this order:

1. Tokio or Tokio Util;
2. the Rust standard library;
3. a project-owned implementation.

Build a project-owned primitive only when the first two options have a concrete capability or correctness gap. Document that gap and the invariants the custom implementation maintains. Continue to use ordinary standard-library data types when no Tokio runtime behavior is involved.

Pass `CancellationToken` directly through application-owned ports to infrastructure operations that support cancellation. Infrastructure owns cooperative cancellation checks. Write each cancellation checkpoint inline with `cancellation.is_cancelled()` and return the typed cancellation error immediately. A cancellation-check helper does not earn a separate interface. Preserve partial filesystem state without cleanup, settlement, or rollback. If the final irreversible mutation already completed, return success. Add a separate narrow progress-reporting port if a presentation needs progress; do not bundle progress with cancellation.

## Control flow

Prefer `if`, `if let`, or `let ... else` whenever they express the logic clearly. Use an ordinary `if` for boolean conditions, `let ... else` when one required pattern has an exiting failure path, and `if let` when behavior depends on one relevant pattern.

Prefer guard clauses that return, continue, or break early. Keep the successful path unnested. Use an `else` branch only when both branches have distinct continuing behavior and a guard clause would not make the function clearer.

Reserve `match` for genuinely multi-way logic, meaningful exhaustive handling, or value mapping that is clearer than the equivalent conditional form. Replace a one-pattern or simple two-arm `match` with `if`, `if let`, or `let ... else` when possible.

## Functions and tests

Prefer cohesive orchestration functions that read from start to finish, with blank lines separating their phases. Use `initialize_environment` as the reference shape. Function length alone is not a reason to extract code. Keep one-use validation, transformation, and control flow inline when that preserves the operation's narrative.

Extract a helper when it represents reused logic, a separately meaningful algorithm, a safety or FFI boundary, or a real adapter seam. Each helper should hide nontrivial complexity. A helper that only renames one expression, one condition, or one call does not earn a separate interface.

Before MVP, write only unit tests. Put each test in a `#[cfg(test)]` module in the same source file as the behavior it tests. Do not add separate `tests/` targets, end-to-end tests, or cross-file test modules. Keep fixture helpers inside the colocated test module; this is the test-specific exception to reusable-helper placement.

Test behavior through public seams where practical. Unit tests focus on project-owned business behavior and owned boundaries. Trust dependency contracts. Add dependency-detail coverage only for project policy, an unstable adapter edge, or a focused regression for a real interaction failure. Prefer small wiring tests over recreating dependency suites.

## Unsafe Rust

Forbid unsafe code in domain, application, and presentation crates. Keep unsafe code in dedicated infrastructure or FFI modules behind safe interfaces.

Put a `// SAFETY:` proof immediately before every unsafe block. State the API requirements, caller obligations, ownership and lifetime invariants, and how the safe wrapper maintains them. Public unsafe functions also require a rustdoc `# Safety` section.

## Linux review principles retained

Retain these language-neutral principles:

- readable control flow;
- cohesive responsibilities;
- precise names;
- comments that explain necessary reasons and constraints;
- explicit failure handling;
- reuse of established abstractions;
- restrained low-level optimization;
- recovery from expected failures.

Rustfmt applies the retained eight-column tab stops and 120-column width. Rust conventions replace the remaining Linux C implementation conventions. Do not transfer K&R braces, cleanup `goto`, integer error codes, kernel-doc, preprocessor patterns, allocator spelling, or GNU C assembly syntax.
