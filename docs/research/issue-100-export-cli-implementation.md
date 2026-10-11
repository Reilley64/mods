# Issue #100 CLI export implementation evidence

Status: local implementation on stacked `feat/export-cli-100`, independent re-review complete; Windows acceptance pending. This does not close #100 or assert Windows release readiness. Based on #99 implementation evidence in [issue-99-export-implementation.md](issue-99-export-implementation.md) and the approved #97 contract at `1feddf41da1b2eeff9d7f5c4eb7ea1ace0767f09:docs/research/issue-97-export-contract.md`.

## Scope and implementation

The CLI accepts exactly `mods [--environment PATH] export OUTPUT [--include-saves] [--dry-run]`. OUTPUT resolves from startup cwd. Resources composes the existing application use case with an effective Game Binding read and validation before inventory; it checks the current binding again before publishing. Dry-run prints output paths, provider identities, file counts and bytes; successful publication is quiet. Errors render allowlisted marker fields, destination or retained staging paths, and targeted next steps without raw report trees. The standalone [export guide](../../skills/mods-cli/references/export.md) explains manual Data versus Documents/LocalAppData placement, saves, invalidation, provenance, modification times, load-order and upstream limitations. README and skill command/troubleshooting references link this guide. No export algorithm, domain/application interface, dependency, build target, MCP, destination apply, migration or game launch changed.

The CI-only Windows test-import fix belongs to base commit `082edbe` and PR #137, not this branch's #100 diff.

## Checks and limits

- `cargo test -p mods export`: 5 passed (CLI syntax, dispatch preview/publish, allowlisted errors, preview text).
- Bounded repair: `cargo test -p mods retained_ini`: 2 passed (allowlisted retained execution INI path and CLI dispatch status/path/no-raw-report).
- `cargo fmt --all && bun run check && git diff --check`: 413 Rust tests, 2 release-version tests, 119 tooling tests, workspace Clippy, dependency graph, formatting and whitespace passed on final Rust files.
- macOS cannot run native Windows final publication/usvfs. The existing #99 adapter tests exercise dry-run/staging but not Windows publication. Parent owns Windows build/install evidence. No environment was exported to a game or applied.

## New coding-style gate dispositions

The gate reported likely findings during incremental edits. These are manual dispositions, **not a clean automated gate or override**. Existing #99 findings retain their dispositions in [the #99 report](issue-99-export-implementation.md).

| File under `src/` | Rule | Disposition |
| --- | --- | --- |
| presentation/cli/src/runner.rs | Callable port invocation | Accepted false positive: `export_environment(...)` is the application use-case function, not a callable port; application itself calls ports with `.call((...))`. |
| presentation/cli/src/runner.rs | Import placement and use | Fixed accidental duplicate/unindented test imports. Current imports are at module scope. |
| presentation/cli/src/runner.rs | Use-case declaration order | Accepted false positive: binary dispatch module, not an application use-case definition. The existing application export use case retains its required primary declaration order. |
| presentation/cli/src/runner.rs | Phase spacing | Fixed the output-path resolution/dispatch boundary. The command arm otherwise handles one dispatch phase and its result mapping. |
| presentation/cli/src/runner.rs | Use-case parameters | Accepted false positive: `run`/`dispatch` are existing presentation functions, not application use-case entry points. The application export call passes dependencies first and cancellation last. |
| infrastructure/dependencies/src/export_environment.rs | Phase spacing | Fixed separation between resource acquisition, validation, inventory, publication revalidation and output construction. Adjacent cloned captures belong to one acquisition phase. |
| infrastructure/dependencies/src/export_environment.rs | Use-case parameters | Accepted false positive: Resources method and infrastructure port closures are not application use-case entry points; the port signature is frozen and cancellation stays last. |
| infrastructure/dependencies/src/export_environment.rs | Narrow custom implementations | Accepted: this is narrow composition for effective-binding validation at the CLI boundary and revalidation before publish; it reuses settings, game-platform and environment adapters without implementing new general infrastructure. |
| infrastructure/dependencies/src/export_environment.rs | Use-case declaration order | Accepted false positive: infrastructure composition method, not an application use case. |
| presentation/cli/src/main.rs | Use-case parameters; Use-case declaration order | Accepted false positives: binary composition root constructs the dependency bundle; it does not define or invoke a new application use-case signature here. |
| presentation/cli/src/export_output.rs | Language-neutral review priorities | Fixed initial dense fixture formatting via rustfmt and clear field-by-field output; preview maps explicit typed provider variants to approved output. No optimization or ignored failure. |
| presentation/cli/src/export_output.rs | Import placement and use | Fixed test return type to imported `rootcause::Result`; other imports remain module scoped. |
| presentation/cli/src/export_output.rs | Phase spacing | Fixed header/entry iteration/output boundaries and fixture/output assertion boundary. |
| presentation/cli/src/export_output.rs | Use-case parameters | Accepted false positive: `preview` is a presentation formatter, not an application use-case function; no dependency bundle or cancellation applies. |
| presentation/cli/src/export_output.rs | Narrow custom implementations | Accepted: presentation text mapping owns a specific CLI output format and is not generic infrastructure. Existing quote function is reused. |
| presentation/cli/src/error.rs | Phase spacing | Fixed marker formatting, retained-path selection and advice/output boundaries. |
| presentation/cli/src/error.rs | Use-case parameters | Accepted false positive: presentation-only error formatter, not application use case; no dependency bundle or cancellation applicable. |

## Bounded independent review and repair

Standards review reported no blockers. Spec review identified one in-scope blocker: the CLI execution error path dropped the typed `RetainedExecutionInis.path` supplied by #99. The bounded repair adds an allowlisted quoted `retained_execution_inis` path and advice to inspect edits only after all managed processes stop. It preserves existing error marker/status behavior and never formats the raw report. A focused formatter test and an execution-dispatch regression cover the path, status and absence of internal report text. No runtime/profile API or policy changed. The one permitted spec and standards re-review passed with no remaining #100 blockers. Final local checks passed with 413 Rust tests, 2 release-version tests and 119 tooling tests; Windows checks remain separate.

New incremental gate findings after this repair: `presentation/cli/src/error.rs` / Import placement and use was fixed by importing `Path` and `PathBuf` in the colocated test module instead of qualifying each call; `presentation/cli/src/runner.rs` / Test public behavior was fixed by moving the regression through `run(...)` instead of directly testing the private outcome function. Further runner Use-case parameters and Use-case declaration order notices are covered above: this is binary presentation dispatch, not an application use-case definition. The automated gate remains non-clean; no override was used.
