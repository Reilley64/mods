# Contributing

Use the pinned Rust toolchain. Follow `CODING_STYLE.md` for Rust implementation and review. Install the pinned Rust test runner and repository tools once:

```text
cargo install cargo-nextest --locked --version 0.9.145
bun install --frozen-lockfile
```

Use focused red-green cycles during implementation. Run Rust tests through `cargo nextest run` so `.config/nextest.toml` enforces the repository-wide per-test timeout. Run focused checks for the affected crates, then run the complete checks once before opening a pull request:

```text
bun run check
cargo check --workspace --target x86_64-pc-windows-msvc
```

Pull request titles must follow Conventional Commits. This repository validates squash pull request titles in GitHub Actions. It does not install Husky or enforce individual local commit messages.

## Release Please credentials

The release workflow creates a short-lived token from a repository-scoped GitHub App. Configure the App Client ID as the `RELEASE_PLEASE_APP_CLIENT_ID` repository variable and its private key as the `RELEASE_PLEASE_APP_PRIVATE_KEY` repository secret. Grant the App only repository `contents`, `pull requests`, and `issues` write access. The workflow requests only those permissions.
