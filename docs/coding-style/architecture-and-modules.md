# Coding style: Architecture and modules

This file is one area of the [coding style](../../CODING_STYLE.md). Resolve conflicts between rules with its Authority order.

## Architecture and modules

### Dependency direction and composition roots


#### Rule

Preserve `presentation -> application -> domain`. Infrastructure implements application-owned ports. Framework, transport, filesystem, Windows API, and provider types stay outside domain and application. The root is a virtual workspace. Binary presentation packages own their executables and composition roots. In the CLI-only MVP, the CLI package owns `mods.exe`.

#### Violation

Dependency or workspace checks find a reversed layer dependency, a forbidden external type, an unexpected root package, or a misplaced binary composition root.

#### Compliant

The workspace dependency graph and package inventory match the approved direction and ownership.

#### Bad example

```rust
// src/domain/src/path.rs
use windows::Win32::Storage::FileSystem::WIN32_FILE_ATTRIBUTE_DATA;
```

#### Good example

```rust
// src/infrastructure/environment/src/path.rs
use windows::Win32::Storage::FileSystem::WIN32_FILE_ATTRIBUTE_DATA;
```

### Capability modules and public APIs


#### Rule

Organize each layer by capability. Keep leaf modules private by default and re-export a deliberate public API from the parent module.

#### Violation

The change adds a generic dumping-ground module, exposes a leaf module without need, or makes internal implementation types broadly public.

#### Compliant

The module is named for one capability, leaf modules remain private, and the parent re-exports only the intended API.

#### Bad example

```rust
pub mod utils;
pub mod internal_parser;
```

#### Good example

```rust
mod archive_path;

pub use archive_path::ArchivePath;
```
