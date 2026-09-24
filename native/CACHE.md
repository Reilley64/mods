# CI native caches

CI keeps two GitHub Actions build caches:

- A complete x86+x64 Release upstream bundle, restored only by an exact key.
- vcpkg binary archives, reused by vcpkg's package ABI checks on bundle misses.

Keys include the pinned upstream revision (and therefore presets and dependency
baselines), runner image, VS2022/toolset/compiler identities, installed Windows
SDK inventory, CMake version, and vcpkg revision. Bundle keys additionally include
the source manifest, build/cache helpers, and CI workflow. Rust source and Cargo
changes do not invalidate the native caches. Runner image upgrades do.

Every restored or built bundle must pass `native/cache.ps1 -Mode Validate`:
revision marker, manifest source/configuration, all four required entries, and
SHA-256 content checks. A damaged bundle fails the job; it is not silently used.
Delete the affected Actions cache to recover. The key verifies build inputs;
the hashes check integrity, not independent binary provenance. Caches remain
within GitHub's branch/PR access scope and are not a binary release channel.

The job saves successful native work before Rust checks, so a later Rust failure
does not discard the expensive native build. Cache hits still run x86 adapter
checks and the normal Rust checks. Preview publication remains disabled.

The Monday schedule skips both cache restores and performs a clean native build.
The manual CI dispatch also offers `clean_native`. Clean runs may seed a new
cache key but do not replace an immutable existing cache. The first run for a
new key is still slow; subsequent runs can reuse the bundle. PR-specific caches
are not shared with other PRs; the main-branch run seeds caches for later PRs.

Run the offline artifact-validation tests on a PowerShell 7 host:

```text
pwsh -NoProfile -File native/cache.tests.ps1
```

`bun run check:tools` checks the workflow's cache safety conditions. Cache-key
inspection requires the actual Windows build toolchain and is exercised in CI.
The unchanged upstream presets already enable MSVC `/MP`; no upstream sources
or build presets are patched for caching.
