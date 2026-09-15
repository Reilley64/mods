# Coding style

This document is the repository authority for handwritten Rust. Apply it during implementation and review.

## Authority

Resolve conflicts in this order:

1. Rust correctness and soundness.
2. Repository architecture, domain rules, specifications, and accepted decisions.
3. `rustfmt`.
4. Workspace rustc and Clippy policy.
5. Rust API Guidelines.
6. The Linux review principles retained below.

## Formatting and imports

Use the pinned nightly toolchain and run `cargo fmt --all`. The workspace `rustfmt.toml` uses hard tabs at eight-column stops and a 120-column maximum width. It also enforces one imported item per `use` line and one contiguous import group.

Keep imports at module scope. Import a normal item once and use its local name in the module. Use a qualified path at the call site only when it is required for disambiguation or macro syntax. Put conditional imports at module scope with `#[cfg(...)]`.

Use blank lines to separate logical phases, such as validation, recovery, staging, publication, and output construction. Keep one thought together. Do not add decorative spacing inside a fluent expression or tightly coupled operation.

## Architecture and modules

Preserve the dependency direction `presentation -> application -> domain`. Infrastructure implements application-owned ports. Framework, transport, filesystem, Windows API, and provider types stay outside domain and application code. The repository root is a virtual workspace. Each binary-only presentation package owns its executable and composition root: `mods.exe` for the CLI and `mods-mcp.exe` for MCP.

Organize each layer by capability. Keep leaf modules private by default and re-export a deliberate public API from the parent module.

## Application use cases and ports

Each use-case file declares these primary items in order:

1. `<UseCase>Dependencies`;
2. `<UseCase>Output`;
3. `<UseCase>Error`;
4. the `#[tracing::instrument(skip_all)]` async use-case function.

Pass the narrow dependency bundle by value first. Keep business arguments as separate typed values. Pass `tokio_util::sync::CancellationToken` last when the use case supports cancellation.

Application-owned callable ports use the approved nightly `fn_traits` feature. Invoke them explicitly with `.call(())`, `.call((argument,))`, or `.call((argument_one, argument_two))`.

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

Pass `CancellationToken` directly through application-owned ports to infrastructure operations that support cancellation. Infrastructure owns cooperative cancellation checks. When a check observes cancellation, return the typed cancellation error immediately and preserve the partial filesystem state without cleanup, settlement, or rollback. If the final irreversible mutation already completed, return success. Add a separate narrow progress-reporting port if a presentation needs progress; do not bundle progress with cancellation.

## Control flow

Prefer `let ... else` when one required pattern has a failure path that returns, continues, or otherwise exits the current flow. Prefer `if let` when behavior depends on one relevant pattern and the remaining cases need no distinct handling.

Use `match` when multiple arms have distinct meaning, exhaustive handling protects correctness, the expression maps values, or `match` is clearer than nested conditions. Do not replace a meaningful exhaustive match only to satisfy this preference.

## Functions and tests

Prefer readable control flow and precise names. Keep single-use logic inline when practical. Extract code for reuse or a real boundary, not only to make a unit test easier.

Test behavior through public seams. Unit tests focus on project-owned business behavior and integration seams. Trust dependency contracts. Add dependency-detail tests only for project policy, an unstable adapter edge, or a focused regression for a real interaction failure. Prefer small wiring tests over recreating dependency suites.

Use focused red-green cycles, then run the complete repository checks once before completion.

## Comments and documentation

Express behavior through names, types, and control flow first. Add comments for external constraints, safety proofs, compatibility workarounds, and intentionally surprising decisions. Explain why the constraint exists rather than narrating the code.

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

## Verification

During implementation, run focused tests and checks for the affected crates. Before completion, run:

```text
bun run check
cargo check --workspace --target x86_64-pc-windows-msvc
```
