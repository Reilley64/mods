# Coding style: Formatting and imports

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

## Formatting and imports

### Import placement and use


#### Rule

Keep imports at module scope. Import a normal item once and use its local name. Use a qualified call-site path only for disambiguation or macro syntax. Put conditional imports at module scope with `#[cfg(...)]`.

#### Violation

The change adds a block-local import, repeats a normal qualified path without need, or places a conditional import inside a function.

#### Compliant

Imports are declared once at module scope, with call-site qualification reserved for a concrete ambiguity or macro requirement.

#### Bad example

```rust
fn load() {
	use std::fs::read_to_string;
	let value = std::fs::read_to_string("settings.toml");
}
```

#### Good example

```rust
use std::fs::read_to_string;

fn load() {
	let value = read_to_string("settings.toml");
}
```

### Phase spacing


#### Rule

Partition a function body into semantic blocks. Use one blank line when responsibility changes between guards or validation, acquisition or recovery, transformation or staging, side effects or publication, and output construction. Keep adjacent statements together when they jointly perform one operation.

#### Violation

The change runs distinct semantic phases together with no blank line, or separates statements that jointly perform one operation as though they were different phases.

#### Compliant

One blank line marks each real responsibility change, while statements belonging to the same operation remain contiguous.

#### Bad example

```rust
validate(&input)?;
let staged = stage(input)?;
publish(staged)?;
let output = Output::new();
```

#### Good example

```rust
validate(&input)?;

let staged = stage(input)?;

publish(staged)?;

let output = Output::new();
```
