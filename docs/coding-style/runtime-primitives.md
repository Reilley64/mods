# Coding style: Runtime primitives

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

## Runtime primitives

### Primitive selection order


#### Rule

Choose runtime, asynchronous, synchronization, signaling, and cancellation primitives in this order: Tokio or Tokio Util, the Rust standard library, then a project-owned implementation.

#### Violation

The change adds a project-owned runtime primitive while a Tokio or standard-library primitive satisfies the stated behavior.

#### Compliant

The highest-priority suitable primitive is used, or a concrete missing capability is documented.

#### Bad example

```rust
struct ProjectCancellation {
	cancelled: AtomicBool,
	waiters: Mutex<Vec<Waker>>,
}
```

#### Good example

```rust
use tokio_util::sync::CancellationToken;
```

### Custom primitive justification


#### Rule

Build a project-owned primitive only for a concrete capability or correctness gap. Document the gap and maintained invariants. Continue using ordinary standard-library data types when no Tokio runtime behavior is involved.

#### Violation

A custom primitive lacks a concrete gap or invariant description, or replaces an ordinary data type without runtime behavior.

#### Compliant

The code identifies the exact missing behavior and states the invariants its narrow implementation preserves.

#### Bad example

```rust
struct ProjectCounter(AtomicUsize);
```

#### Good example

```rust
// Tokio has no primitive that atomically publishes this two-file recovery point.
// `committed` becomes true only after both durable renames complete.
struct RecoveryPublication { /* narrow state */ }
```

### Cancellation propagation and checkpoints


#### Rule

Pass `CancellationToken` directly through application-owned ports to infrastructure operations that support cancellation. Infrastructure owns cooperative checks. Write each checkpoint inline with `cancellation.is_cancelled()` and return the typed cancellation error immediately; do not introduce a cancellation-check helper.

#### Violation

The token is wrapped or replaced, application code owns infrastructure checkpoints, or a helper hides a one-line cancellation check.

#### Compliant

The original token reaches infrastructure and each checkpoint is an inline early return with the typed error.

#### Bad example

```rust
fn check_cancelled(cancellation: &CancellationToken) -> Result<()> { /* one check */ }
```

#### Good example

```rust
if cancellation.is_cancelled() {
	return Err(CopyArchiveCancelled.into_report());
}
```

### Cancellation state preservation


#### Rule

On cancellation, preserve partial filesystem state without cleanup, settlement, or rollback. If the final irreversible mutation already completed, return success.

#### Violation

A cancellation path removes partial output, rolls back completed work, settles state, or reports cancellation after the final irreversible mutation succeeded.

#### Compliant

Cancellation returns immediately with partial state intact, while a completed final mutation returns success.

#### Bad example

```rust
if cancellation.is_cancelled() {
	remove_dir_all(staging).await?;
	return Err(Cancelled.into_report());
}
```

#### Good example

```rust
if published {
	return Ok(output);
}
if cancellation.is_cancelled() {
	return Err(Cancelled.into_report());
}
```

### Separate progress reporting


#### Rule

Add a separate narrow progress-reporting port when presentation needs progress. Do not bundle progress reporting with cancellation.

#### Violation

One port or callback combines cancellation control with progress events.

#### Compliant

Cancellation remains a `CancellationToken` and progress uses an independent narrow port.

#### Bad example

```rust
trait OperationControl {
	fn is_cancelled(&self) -> bool;
	fn report_progress(&self, completed: u64);
}
```

#### Good example

```rust
async fn copy(progress: ReportProgress, cancellation: CancellationToken) { /* ... */ }
```
