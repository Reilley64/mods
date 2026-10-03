# Issue #131 acceptance record

## Scope

Explicit global `--json` covers command results, help, version, startup and argument failures. File Conflict and installation success objects use typed, allowlisted fields. Failure objects use CLI Problem Details. Normal text output stays in place. `exec` forwards inherited child streams without a JSON success document. No ADR, glossary, MCP, prompt, JSONL, or choice-session change was made.

## Checks

- `cargo fmt --check`: passed.
- `cargo check -p mods`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed after six lint repairs.
- `cargo nextest run -p mods`: 49 passed.
- `git diff --check`: passed.
- Full `bun run check`: passed after repair, including 401 Rust tests, 119 tooling tests, release-version tests, formatting, Clippy, and dependency checks.
- CLI smoke checked JSON help, version, invalid arguments, root selection, and pre-launch `exec` errors; ordinary argument output remained text.
- JSON Schema validation passed using `jsonschema` in a user-approved temporary `uv` environment. `Draft202012Validator.check_schema` accepted the schema. All 12 published examples passed their command-specific definitions and the aggregate schema. Five captured CLI documents passed their definitions: help, version, invalid arguments, Environment Root selection failure, and pre-launch execution failure. Each capture also preserved its expected exit status and used only the documented output channel. No project dependency or lockfile changed.

## Platform blocks

- Native Windows byte-for-byte child stdout and stderr, inherited stdin, process status, and post-launch supervision checks cannot run on macOS. The native adapter uses inherited streams and an existing post-launch event; that is code inspection, not Windows runtime evidence.
- The macOS cross-target Windows MSVC check stops in `zstd-sys`: Windows SDK `stdio.h` and `stdlib.h` are unavailable. It is blocked, not passed.

## Coding-style gate disposition

The gate emits advisory findings that vary by run. The following dispositions cover every unique reported rule; accepted findings are not a clean gate pass.

- Fixed: `runner.rs`, `main.rs`, and `json_output.rs` phase spacing at publication and warning phases; `json_conflicts.rs`, `json_install.rs`, and `json_output.rs` import placement with module-level imports and only necessary qualified paths; `json_output.rs` Rustdoc-format finding by removing the misplaced comment; `runner.rs` readability by keeping JSON state typed until publication; six deterministic Clippy findings.
- Accepted, presentation-only false positive: `main.rs` and `runner.rs` use-case parameters and declaration order, and `runner.rs` focused use-case orchestration. Neither file defines an application use case. They own CLI composition and dispatch.
- Accepted, ticket-specific code: narrow custom implementation findings in `main.rs`, `runner.rs`, `json_output.rs`, `json_install.rs`, and `json_conflicts.rs`. These files map approved CLI results to a stable, allowlisted public JSON contract; they do not implement general-purpose serialization infrastructure.
- Accepted, test-only result shape: `commands.rs` and `json_output.rs` tests use standard `Result` to surface Clap errors without `unwrap` or `panic`. The rootcause lower-layer-results rubric applies to production lower-layer APIs, not presentation tests.
- Accepted, approved test seam: `runner.rs` test-public-behavior finding. The tests call `run` and assert CLI output, using a fake application port for pre-launch and post-launch scenarios.
- Accepted, safe publication: `main.rs` and `json_output.rs` preserve-causes and presentation-allowlist findings. Clap's own argument/help text is rendered only at the CLI boundary; `ErrorKind::Io` uses a fixed safe detail. Application report trees are never serialized. `runner.rs` Environment Root selection maps the known domain error to fixed safe wording and retains the typed cause in the local error. The gate also reported rootcause/context propagation while this mapping was under repair; that is resolved by retaining the typed domain cause in `RootSelectionError`.

## Final review

The standards re-review confirmed that its concrete blockers are resolved, including the final phase-spacing fix in `main.rs`. The spec re-review confirmed the completed nested schemas and additive-field compatibility. External schema validation then passed as recorded above.

The latest advisory gate still reports these file-level findings. They remain explicit acceptances, not a clean gate pass:

| File under `src/presentation/cli/src/` | Rule | Disposition and reason |
| --- | --- | --- |
| `json_conflicts.rs` | Import placement and use | Accepted as unsupported. Imports are module-level; the read-only review found no concrete violating location. |
| `json_install.rs` | Import placement and use | The identified qualified enum paths were fixed and rechecked. Accept the remaining file-level allegation because it supplies no further location. |
| `runner.rs` | Phase spacing | The identified classification and publication phases were separated and rechecked. Accept the remaining allegation because no further violating location was established. |
| `main.rs` | Phase spacing | Acquisition, transformation, and publication now have separate phases. The reviewer verified the final fix. Accept the residual file-level allegation as stale or unsupported. |
| `json_output.rs` | Phase spacing | Detail collection, title construction, and output are separated. The reviewer confirmed the repair. Accept the remaining allegation as unsupported. |
| `runner.rs` | Narrow custom implementations | Accepted. The code owns command output and the launch boundary; it uses serde_json rather than replacing a general-purpose serializer. |
| `json_conflicts.rs` | Narrow custom implementations | Accepted. Explicit File Conflict field mapping preserves the approved public contract. |
| `json_output.rs` | Narrow custom implementations | Accepted. Explicit warning, setting, and Problem Details mapping preserves the safe allowlist. |
| `runner.rs` | Use-case parameters | Accepted as inapplicable to presentation dispatch rather than application use cases. |
| `main.rs` | Use-case parameters | Accepted as inapplicable to the binary composition root. |
| `runner.rs` | Use-case declaration order | Accepted as inapplicable to presentation dispatch rather than application use cases. |

These acceptances do not bypass enforce mode. If the gate is enforcing, its explicit override mechanism is still required.

## Follow-ups outside this ticket

The marker has `setting_key` and `mod_name`, but the existing text error allowlist omits them. Extending the allowlist needs a separate decision. Serializer-switch deduplication and broader typed JSON representations can be considered after the public contract is reviewed. Neither follow-up changes this ticket's output.
