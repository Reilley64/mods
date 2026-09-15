# Contributing

Use Bun for repository checks:

```text
bun install --frozen-lockfile
bun run check
```

Pull request titles must follow Conventional Commits. This repository validates squash pull request titles in GitHub Actions. It does not install Husky or enforce individual local commit messages.

Use the pinned Rust toolchain. Follow `CODING_STYLE.md` for Rust implementation and review. Run `bun run check` before opening a pull request.

## Release Please credentials

The release workflow creates a short-lived token from a repository-scoped GitHub App. Configure the App Client ID as the `RELEASE_PLEASE_APP_CLIENT_ID` repository variable and its private key as the `RELEASE_PLEASE_APP_PRIVATE_KEY` repository secret. Grant the App only repository `contents`, `pull requests`, and `issues` write access. The workflow requests only those permissions.
