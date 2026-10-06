# Contributing

Use the pinned Rust toolchain. Follow `CODING_STANDARDS.md` and the area files under `docs/coding-standards/` that it names for your change. Install the pinned Rust test runner and repository tools once:

```text
cargo install cargo-nextest --locked --version 0.9.145
bun install --frozen-lockfile
```

Use focused red-green cycles during implementation. Run Rust tests through `cargo nextest run` so `.config/nextest.toml` enforces the repository-wide per-test timeout. Run focused checks for the affected crates, then run the complete checks once before opening a pull request:

```text
bun run check
cargo check --workspace --target x86_64-pc-windows-msvc
```

On macOS or Linux, run the Windows check through [cargo-xwin](https://github.com/rust-cross/cargo-xwin) instead. It needs LLVM's `clang-cl` and `llvm-lib` on `PATH`, and the pinned usvfs bundle in `MODS_USVFS_ARTIFACTS`:

```text
cargo install cargo-xwin --locked
brew install llvm
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
export MODS_USVFS_ARTIFACTS="$PWD/native/artifacts/bin"
cargo xwin clippy --workspace --all-targets --all-features --target x86_64-pc-windows-msvc -- -D warnings
```

Get the bundle with `pwsh ./native/fetch.ps1`, as `native/README.md` describes. The first run downloads the MSVC CRT and Windows SDK headers. This check compiles and lints `cfg(windows)` code; Windows CI still runs the Windows tests.

Pull request titles must follow Conventional Commits. This repository validates squash pull request titles in GitHub Actions. It does not install Husky or enforce individual local commit messages.

## Installation seams

The CLI parses local or remote input and calls the provider-neutral `installation::install_mod` use case. Its `DownloadMod` port returns a completed local archive with a suggested Mod Name and optional explicit source metadata, or files that need explicit selection. Nexus authentication, file resolution, and cache handling stay inside `infrastructure-nexus::NexusAdapter` behind that port. `install_archive` keeps the existing archive safety, FOMOD, preview, and publication flow.

Keep provider policy tests in the Nexus adapter. Test installation orchestration through `install_mod` and command wiring through the CLI. Credential loading is infrastructure wiring, not an application setting that callers can inspect. The Nexus adapter supplies explicit Nexus provenance; downloads without it remain valid. Nexus provenance still uses the required `[nexus]` table; the provider-neutral orchestration does not choose a provider or derive names from that table.

## Release Please credentials

The release workflow creates a short-lived token from a repository-scoped GitHub App. Configure the App Client ID as the `RELEASE_PLEASE_APP_CLIENT_ID` repository variable and its private key as the `RELEASE_PLEASE_APP_PRIVATE_KEY` repository secret. Grant the App only repository `contents`, `pull requests`, and `issues` write access. The workflow requests only those permissions.
