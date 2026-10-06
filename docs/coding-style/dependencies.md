# Coding style: Dependencies

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

## Dependencies

### Prefer established crates


#### Rule

Prefer popular, actively maintained crates with clear ownership and documentation when they satisfy the requirement. Reuse their established abstractions instead of building a project-owned equivalent.

#### Violation

The change implements a general-purpose runtime, protocol, parser, synchronization primitive, or utility already supplied by an approved dependency, without identifying a concrete gap.

#### Compliant

The change uses an established crate abstraction, or records a concrete reason no suitable maintained crate satisfies the requirement.

#### Bad example

```rust
struct AsyncChannel<T> {
	queue: Mutex<VecDeque<T>>,
	waiters: Vec<Waker>,
}
```

#### Good example

```rust
let (sender, receiver) = tokio::sync::mpsc::channel(capacity);
```

### Narrow custom implementations


#### Rule

Write custom code only for a concrete domain, safety, compatibility, or platform gap that available crates do not close cleanly. Keep it narrow and document why the established solution is insufficient.

#### Violation

Custom infrastructure is broad, lacks a stated concrete gap, or reimplements unrelated parts of an existing abstraction.

#### Compliant

The custom code is limited to the uncovered capability and documents the exact gap and maintained invariants.

#### Bad example

```rust
struct ProjectRuntime {
	threads: Vec<Thread>,
	timers: TimerWheel,
	io: IoDriver,
}
```

#### Good example

```rust
// notify does not report rename pairs on this Windows API path, so this adapter
// correlates only the two records required by profile recovery.
struct RenamePairCorrelator { /* narrow state */ }
```
