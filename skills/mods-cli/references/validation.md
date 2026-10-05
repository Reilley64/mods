# Packaging and validation — issue #114

## Install this agent skill

The skill is a Markdown Agent Skill at `skills/mods-cli/SKILL.md`; its references travel with it. This installs **agent instructions**, not `mods.exe`.

Once this change is available on the repository's default branch:

```sh
npx skills add Reilley64/mods --skill mods-cli
```

Use a Node.js/npm environment with `npx` and GitHub access. The skills CLI asks which agent and scope to install into; project scope is the default, and `--global` selects user scope. Review the skill before approving installation. To list without installing:

```sh
npx skills add Reilley64/mods --skill mods-cli --list
```

For a local checkout containing this change, from that checkout:

```sh
npx skills add . --skill mods-cli --list
```

To install from a checkout into a separate consumer project, run `npx skills add /absolute/path/to/mods --skill mods-cli` from that consumer project. Local-path installation was verified; the GitHub source cannot supply unpushed work. Remote installation and skills.sh listing are not claimed by a local commit.

## Current packaging requirements

Checked against the [skills CLI README](https://github.com/vercel-labs/skills#readme), [skills.sh documentation](https://skills.sh/docs), and [Agent Skills specification](https://agentskills.io/specification). Context7 also confirmed the upstream `add`, `--skill`, and `--list` interfaces. CLI version tested: **1.7.0** (`npx skills@latest --version`).

- A discoverable directory contains `SKILL.md` with YAML `name` and `description`. `skills/<name>/SKILL.md` is a supported discovery layout.
- Name matches the directory; lowercase letters/digits/hyphens, at most 64 characters, no leading/trailing/consecutive hyphens. Description is nonempty and at most 1024 characters. Compatibility is at most 500 characters; metadata values are strings.
- Bundled relative links resolve within the installed skill. The entry point routes to references by task rather than embedding the full command manual.
- A public GitHub repository is an install source; there is no npm package to publish for this Markdown skill. skills.sh ranking uses installation telemetry. A local install with telemetry disabled does not establish a public listing.

Authoring followed **writing-for-agents** (including Skill Mechanics) and **skill-creator**: model-invoked routing, Markdown-only packaging, short ordered entry point, explicit approval/outcome boundaries, branch-specific references, metadata and installed-copy validation. No Python extension or contributor skill was added.

## Released evidence and coverage

Reference release: [v0.1.0](https://github.com/Reilley64/mods/releases/tag/v0.1.0), latest published stable release at validation. Published source revision: `1635e408c913181c15b518bc95339ebdd6926169`. The runtime ZIP's `BUILD-AND-SOURCE.md` states its executables are unchanged from the original release. Release-note history mentions MCP work, but the released CLI command definitions and runtime inventory contain no MCP executable/command; those claims are not copied into this skill.

All source paths below are under the fixed [v0.1.0 tree](https://github.com/Reilley64/mods/tree/v0.1.0), not working-tree HEAD.

| Reference coverage | Released evidence |
| --- | --- |
| Every command, positional argument, flag, level and setting key; example syntax | `src/presentation/cli/src/commands.rs`, including parser tests |
| Environment Root default and relative path semantics; install outcomes and child status forwarding | `src/presentation/cli/src/runner.rs`, `path_resolution.rs` |
| Settings and FOMOD/preview text, warnings, quiet mutations | `src/presentation/cli/src/output.rs`, `install_warning.rs` |
| Init precedence, target constraints, overrides | `src/application/src/environment/initialize_environment.rs`; `src/infrastructure/game_platform/src/resolution.rs`; `src/infrastructure/environment/src/lib.rs`; `src/infrastructure/settings/src/config_source.rs` |
| Archive formats, name derivation, replacement and disabled new installs | `src/infrastructure/archive/src/index.rs`; `src/domain/src/installation.rs`; `src/application/src/installation/install_archive.rs` |
| Complete ordered choices, cardinality and synthetic none | `CONTEXT.md`; `src/application/src/installation/install_archive/fomod.rs` |
| Conflict reports, invalid resolutions, content states and analytical scope | `src/presentation/cli/src/conflict_output.rs`; `src/application/src/conflicts/`; `CONTEXT.md` |
| Output Target, mapping limits, executable resolution and cwd | `src/infrastructure/execution/src/configuration.rs`, `launch_inputs/windows_inputs.rs`; `CONTEXT.md` |
| Errors, cancellation, child statuses, diagnostics | `src/presentation/cli/src/error.rs`, `main.rs`, `publication.rs`, `diagnostics.rs`, `operation.rs`; `src/infrastructure/execution/src/managed.rs` |
| Runtime prerequisites and files | Published runtime ZIP, `BUILD-AND-SOURCE.md`, runtime checksum asset; tagged `docs/distribution.md` |

v0.1.0 has no `--json` flag, so the table above is not evidence for JSON output. JSON output comes from issue #131 and is unreleased at this reference version. Its source is `src/presentation/cli/src/json_output.rs`, `json_install.rs`, and `json_conflicts.rs` on the default branch, and its contract is [JSON output](https://github.com/Reilley64/mods/blob/main/docs/cli/json.md).

Examples were checked against these released definitions and tests. The `--json` flag in the examples was checked against the default-branch source above, not against v0.1.0. Paths, tool names/arguments, and FOMOD IDs are illustrative, not claims of executed game workflows. Windows commands must still be checked against the user's installed help.

## Validation results

- Release API: `gh release view` identified `v0.1.0` and its published runtime assets.
- WinGet follow-up: at inspection, the public manifest path was absent and [package PR #442597](https://github.com/microsoft/winget-pkgs/pull/442597) was open. Per owner direction, setup assumes package availability and instructs `winget install Reilley64.Mods` directly, without a preflight availability check. This is an authoring assumption, not verified publication evidence. Command syntax was checked against [Microsoft documentation](https://learn.microsoft.com/en-us/windows/package-manager/winget/install); no Windows installation was run.
- Downloaded runtime ZIP SHA-256: `3a71d12b77cbfd8e2371258075ae0e5148db219865006cb3e0f952b9f31f44f9`; matches downloaded `mods-v0.1.0-runtime-SHA256SUMS`. Inspected ZIP inventory: `mods.exe`, both DLLs, both proxies, notices, and source instructions.
- `npx skills@1.7.0 add . --skill mods-cli --list`: exit 0, exactly one skill discovered, correct name and description.
- `npx skills@1.7.0 add <checkout> --skill mods-cli --agent codex --yes --copy`: exit 0 in a disposable consumer project; installed `.agents/skills/mods-cli`. Telemetry disabled for validation.
- Metadata validation passed: matching valid name, bounded nonempty description/compatibility, and string metadata. All 16 bundled Markdown links resolve inside the skill; seven Markdown files are packaged.
- Final disposable-project install: all seven installed files checked byte-for-byte against the source, with no missing bundled references.
- Public-source discovery command `npx skills@1.7.0 add Reilley64/mods --skill mods-cli --list` cloned successfully but listed only the existing `subagent-router`, not this unpushed skill. Exit zero alone is not a remote publication pass. Rerun discovery and installation after merge/publication.
- `bun run check`: passed (format, Clippy/typechecking, dependency checks, release-version tests, full host Rust and repository-tool suites).
- `cargo check --workspace --target x86_64-pc-windows-msvc`: blocked, exit 101. The macOS C toolchain cannot find Windows target C headers (`stdlib.h`, `string.h`) while building `zstd-sys`. No toolchain workaround or new platform harness was added.
- `git diff --check`: passed. No Rust behavior changed, so no new Rust tests or TDD seam were introduced.

**Unavailable on this macOS host:** Windows runtime installation/help execution, PowerShell examples, Steam discovery, real FOMOD archive installation/replacement, native conflict/execution behavior, child-status and cancellation smoke tests, and an interactive agent reload. Source inspection and local skill installation are not substitutes for those checks. No Windows runtime pass is claimed.
