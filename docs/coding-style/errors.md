# Coding style: Errors

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

## Errors

### Rootcause lower-layer results

#### Applies to

- `src/domain/**`
- `src/application/**`
- `src/infrastructure/**`

#### Rule

Rootcause 0.13 is the only lower-layer error system. Prefer `rootcause::Result<Value, Context>` for fallible domain, application, and infrastructure APIs.

#### Violation

Dependency or syntax checks find a second lower-layer error framework or an unapproved fallible API shape.

#### Compliant

Lower layers use Rootcause results and contexts consistently.

#### Bad example

```rust
fn load() -> anyhow::Result<Value> { /* ... */ }
```

#### Good example

```rust
fn load() -> rootcause::Result<Value, LoadValueError> { /* ... */ }
```

### Preserve causes at owned boundaries


#### Rule

Create a report when an error first enters an owned boundary. Preserve external and lower-layer causes, and add a new ownership context with `ResultExt::context(...)`.

#### Violation

The change discards an underlying error, replaces it with a marker alone, or formats it into a string instead of preserving the cause tree.

#### Compliant

The original cause remains in the report and the current boundary adds its own typed context.

#### Bad example

```rust
read_value(key).await.map_err(|_| ReadSettingError)?;
```

#### Good example

```rust
let value = dependencies
	.read_value
	.call((key,))
	.await
	.context(ReadSettingError)?;
```

### Context propagation


#### Rule

Give each use case one fixed outer context. Keep semantic markers in the child report tree and inspect them through Rootcause traversal at presentation boundaries. Use `.into_report()` when no new context is needed. Do not use `context_transform` for boundary propagation.

#### Violation

Boundary propagation replaces an existing context, adds multiple competing outer contexts, or loses semantic child markers.

#### Compliant

The fixed outer context remains stable and propagation preserves the existing report tree.

#### Bad example

```rust
operation().context_transform(InstallArchiveError)?;
```

#### Good example

```rust
operation().into_report()?;
```

### Presentation error allowlists

#### Applies to

- `src/presentation/**`

#### Rule

Presentation maps reports to allowlisted output. Raw report formatting never crosses a presentation boundary.

#### Violation

A presentation response, terminal message, or transport payload includes raw `Debug`, `Display`, or report-tree formatting.

#### Compliant

Presentation traverses known markers and emits only explicit, allowlisted fields and messages.

#### Bad example

```rust
response.error = format!("{report:?}");
```

#### Good example

```rust
response.error = map_report_to_public_error(&report);
```
