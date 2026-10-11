# Coding standards: Control flow

This file is one area of the [coding standards](../../CODING_STANDARDS.md). Resolve conflicts between rules with its Authority order.

## Control flow

### Choose the narrow conditional form


#### Rule

Use `if` for boolean conditions, `let ... else` for one required pattern with an exiting failure path, and `if let` when behavior depends on one relevant pattern. When the failure path only converts to an error or a default, use a combinator instead; see "Prefer Option and Result combinators".

#### Violation

The change uses a broader or more nested construct where one of the narrow conditional forms expresses the same logic clearly.

#### Compliant

The conditional form matches the shape of the decision without unnecessary arms or nesting.

#### Bad example

```rust
match enabled {
	true => run(),
	false => {},
}
```

#### Good example

```rust
if enabled {
	run();
}
```

### Guard clauses


#### Rule

Prefer guard clauses that return, continue, or break early. Keep the successful path unnested. Use `else` only when both branches have distinct continuing behavior.

#### Violation

The successful path is nested under a condition whose opposite branch exits, or an `else` follows a branch that already returns, continues, or breaks.

#### Compliant

Exiting cases appear first as guards; `else` remains only when both branches continue differently.

#### Bad example

```rust
if ready {
	publish();
} else {
	return Err(NotReady);
}
```

#### Good example

```rust
if !ready {
	return Err(NotReady);
}

publish();
```

### Match only for multi-way logic


#### Rule

Reserve `match` for genuinely multi-way logic, meaningful exhaustive handling, or value mapping clearer than a conditional. Replace one-pattern or simple two-arm matches with `if`, `if let`, or `let ... else` when possible.

#### Violation

A new `match` has one relevant pattern or a simple boolean/two-arm shape that a narrow conditional expresses more clearly.

#### Compliant

The match performs meaningful multi-way exhaustive logic or clear value mapping.

#### Bad example

```rust
match value {
	Some(value) => use_value(value),
	None => {},
}
```

#### Good example

```rust
if let Some(value) = value {
	use_value(value);
}
```

### Prefer Option and Result combinators


#### Rule

When a branch only converts, forwards, or defaults an `Option` or `Result`, write it as a combinator chain with `?`, such as `ok_or`, `ok_or_else`, `map`, `map_err`, `and_then`, `unwrap_or`, or `.context(...)`, instead of `match`, `if let`, or `let ... else`. Keep a conditional when a branch has side effects or does more than one conversion, or when the chain would need nested closures that are harder to read. Error conversions still follow "Preserve causes at owned boundaries": attach context and never discard the source error.

#### Violation

A `match`, `if let`, or `let ... else` only turns `None` or an error into another error or a default value, where a combinator chain expresses the same logic.

#### Compliant

Pure conversions use combinators and `?`. Conditionals remain for branches with real work in them.

#### Bad example

```rust
let Some(native) = self.native else {
	return Err(report!(ExecutionError));
};
```

#### Good example

```rust
let native = self.native.ok_or_else(|| report!(ExecutionError))?;
```
