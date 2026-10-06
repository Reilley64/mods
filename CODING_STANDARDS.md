# Coding standards

This document is the repository authority for handwritten Rust. Apply it during implementation and review.

The rules live in one file per area under `docs/coding-standards/`. Before you write or review Rust, read every area file that the diff touches, using the list below.

Each standard is a rubric. `Rule` is normative. `Violation` and `Compliant` define the decision boundary. Good and bad examples are representative rather than exhaustive. An optional `Applies to` field lists repository-relative globs. A standard with the field covers only files that match one of its globs, and a standard without it covers every file. Formatting, compilation, lint, dependency, layout, and repository checks remain independent sources of deterministic evidence.

## Authority

Resolve conflicts in this order:

1. Rust correctness and soundness.
2. Repository architecture, domain rules, specifications, and accepted decisions.
3. `rustfmt`.
4. Workspace rustc and Clippy policy.
5. Rust API Guidelines.
6. The [Linux review principles retained](docs/coding-standards/linux-review-principles-retained.md).

## Areas

Read these files for every Rust diff:

- [comments-and-documentation.md](docs/coding-standards/comments-and-documentation.md) covers readability first, self-explanatory code, reason comments, and Rustdoc format.
- [formatting-and-imports.md](docs/coding-standards/formatting-and-imports.md) covers import placement and blank lines between the phases of a function.
- [control-flow.md](docs/coding-standards/control-flow.md) covers the conditional form, guard clauses, `match`, and `Option` and `Result` combinators.
- [functions-and-tests.md](docs/coding-standards/functions-and-tests.md) covers orchestration functions, helper extraction, test placement, and test scope.
- [linux-review-principles-retained.md](docs/coding-standards/linux-review-principles-retained.md) covers language-neutral review priorities and Rust conventions over Linux C idioms.

Read each of these files when the diff matches its trigger:

- [architecture-and-modules.md](docs/coding-standards/architecture-and-modules.md) applies when the diff adds a module or crate, changes a public API, or adds a dependency between layers.
- [dependencies.md](docs/coding-standards/dependencies.md) applies when the diff adds a crate or writes custom code for a need that a crate could meet.
- [application-use-cases-and-ports.md](docs/coding-standards/application-use-cases-and-ports.md) applies when the diff touches `src/application/` or invokes an application port.
- [cli-arguments.md](docs/coding-standards/cli-arguments.md) applies when the diff adds or changes a command-line argument or option.
- [errors.md](docs/coding-standards/errors.md) applies when the diff creates, maps, propagates, or reports errors.
- [runtime-primitives.md](docs/coding-standards/runtime-primitives.md) applies when the diff uses async, synchronization, or cancellation primitives, or reports progress.
- [unsafe-rust.md](docs/coding-standards/unsafe-rust.md) applies when the diff adds or changes `unsafe` code or FFI.
