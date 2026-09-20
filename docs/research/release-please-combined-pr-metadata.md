# Combined release-PR metadata in release-please 17.6.0

## Question and conclusion

This note evaluates whether `googleapis/release-please` 17.6.0 can produce one Cargo-workspace manifest PR while all of these remain true:

1. one combined manifest release PR;
2. the PR title contains the version of the public `mods` crate;
3. the PR body labels the `src/presentation/cli` entry as `mods`;
4. the public tag is `vX.Y.Z`, not `mods-vX.Y.Z`;
5. internal crates retain independent versions and component-prefixed bookkeeping tags; and
6. only `mods` creates a public GitHub Release and the root `CHANGELOG.md`.

**Conclusion:** the stock manifest configuration cannot express all six constraints in 17.6.0. Two implementation details prevent it:

- the final merge takes `${component}` and `${version}` only from a package configured at the special root path `.`; and
- `include-component-in-tag: false` makes `BaseStrategy.getComponent()` return the empty string. That same value controls PR title/body metadata as well as tag lookup, so it removes the `mods` body label in addition to removing `mods-` from the tag.

Under those original six constraints, the smallest robust solution is a repository-owned runner pinned to `release-please@17.6.0`. It would register a small custom Rust release type and a small manifest plugin before calling `Manifest.fromManifest()`. Do not post-edit the generated PR with an unstructured regex and then let the stock action parse it at release time.

The project subsequently accepted a different release model: a separately versioned aggregate product package at `.` owns the public changelog, `vX.Y.Z` tag, and GitHub Release, while CLI and MCP are equal internal presentation packages with independent component-prefixed tags. That model removes the original constraints that the public release version must be the CLI crate version and that the CLI crate itself must own the public release. It provides the desired metadata without an extension and is now the recommended design for this repository.

The relevant v17.6.0 source commit is [`712fcf01effd08d7b0e7b1fd3861f2cb388bc8d1`](https://github.com/googleapis/release-please/tree/712fcf01effd08d7b0e7b1fd3861f2cb388bc8d1). The current workflow pins `release-please-action` 5.0.0 at [`45996ed1f6d02564a971a2fa1b5860e934307cf7`](https://github.com/googleapis/release-please-action/tree/45996ed1f6d02564a971a2fa1b5860e934307cf7). Its declared dependency is `release-please: ^17.6.0` ([`package.json`](https://github.com/googleapis/release-please-action/blob/45996ed1f6d02564a971a2fa1b5860e934307cf7/package.json)), and its lockfile bundles 17.6.0 ([lockfile](https://github.com/googleapis/release-please-action/blob/v5.0.0/package-lock.json#L5130-L5134)). A custom runner should use an exact `17.6.0` dependency rather than the range.

## Adopted no-extension model: aggregate product release

The accepted repository model changes the release ownership rather than extending release-please:

- `.` is a `simple` aggregate product package with its own version in `version.txt`. It owns the root `CHANGELOG.md`, public `vX.Y.Z` tag, and GitHub Release.
- `src/presentation/cli` and `src/presentation/mcp` are equal internal presentation packages. Their components are `mods-cli` and `mods-mcp`; both skip changelogs and GitHub Releases and receive component-prefixed bookkeeping tags.
- The aggregate product version is intentionally independent from both presentation-package versions and is calculated from repository-wide release-worthy commits.
- `cargo-workspace` uses `merge: false`. It still computes Cargo dependency updates, while the final manifest merge produces the one combined release PR. This avoids a Cargo-generated root candidate shadowing the real aggregate root candidate.

A local `release-please@17.6.0 release-pr --dry-run` against this configuration produced exactly one PR titled `chore: prepare v0.1.0 release`, with component summaries for `mods-cli: 0.1.0` and `mods-mcp: 0.1.0`, plus an unprefixed `0.1.0` summary for the aggregate product. It updated the root `CHANGELOG.md`, aggregate `version.txt`, Cargo manifests, root lockfile, and manifest versions without custom code. A separate release-time round-trip passed the generated title and body back through the 17.6.0 strategies: it produced exactly the public `v0.1.0` aggregate release and skipped GitHub Releases for both presentation packages.

The root remains a virtual Cargo workspace: the aggregate product package exists only in the release-please manifest and does not add a Cargo `[package]`.

## What the pre-change configuration got right

The pre-change repository config used:

- `separate-pull-requests: false`, which requests one manifest PR;
- `cargo-workspace` with `updateAllPackages: false`, which preserves independent package versions while still patch-bumping affected dependents and updating the lockfile. This is the officially recommended manifest setup for a Cargo monorepo ([Cargo workspace plugin docs](https://github.com/googleapis/release-please/blob/v17.6.0/docs/manifest-releaser.md#cargo-workspace));
- `skip-github-release: true` and `skip-changelog: true` for internal crates;
- `src/presentation/cli` as package/component `mods`, with the root changelog and `include-component-in-tag: false`; and
- repository workflow code that creates the component-prefixed internal bookkeeping tags which release-please still expects when GitHub Releases are skipped. The schema explicitly warns that `skip-github-release` still requires another system to create tags ([schema lines 57-63](https://github.com/googleapis/release-please/blob/v17.6.0/schemas/config.json#L57-L63); [manifest docs lines 223-231](https://github.com/googleapis/release-please/blob/v17.6.0/docs/manifest-releaser.md#L223-L231)).

Those settings were sufficient for constraints 1, 5, and 6, and for the public tag shape in constraint 4. They were not sufficient for constraints 2 and 3.

## Why configuration alone fails

### Combined title metadata comes only from `.`

When `separate-pull-requests` is false, `Manifest.buildPullRequests()` appends its internal `Merge` plugin after all configured plugins ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/manifest.ts#L798-L840)). `Merge` unions each candidate's updates, release data, and labels, but records a candidate as the metadata source only when `candidate.path === '.'`. It then renders the group title from that root candidate's component and version; without one, both values are `undefined` ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/plugins/merge.ts#L90-L143)).

The official docs say the same thing: `${scope}`, `${component}`, and `${version}` in `group-pull-request-title-pattern` are inherited from the `.` package, if present ([docs lines 253-259](https://github.com/googleapis/release-please/blob/v17.6.0/docs/manifest-releaser.md#L253-L259)). `src/presentation/cli` is not `.`. Therefore changing only the group title template can change the surrounding text, but it cannot supply the public version. `${version}` renders as an empty string because missing title fields are converted to empty strings during rendering ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/util/pull-request-title.ts#L194-L220)).

Adding a synthetic `.` package is not a good configuration-only escape hatch. The special root path is a real release package, not an alias for another package ([docs lines 455-466](https://github.com/googleapis/release-please/blob/v17.6.0/docs/manifest-releaser.md#L455-L466)). It would introduce its own version/release semantics, and this workspace root has no Rust `[package]` version to act as the `mods` crate version. Linking a synthetic root version to `mods` adds bookkeeping and still does not solve the component/tag coupling below.

### Body component and tag component are coupled

`BaseStrategy.getComponent()` returns `''` whenever `includeComponentInTag` is false; only otherwise does it return the configured or discovered component ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/strategies/base.ts#L174-L193)). Candidate PR construction uses that result for the PR title and body release datum ([source lines 277-346](https://github.com/googleapis/release-please/blob/v17.6.0/src/strategies/base.ts#L277-L346), including [release data construction](https://github.com/googleapis/release-please/blob/v17.6.0/src/strategies/base.ts#L242-L263)). Consequently:

| Public package configuration | Body summary | Public tag |
| --- | --- | --- |
| `component: mods`, `include-component-in-tag: false` | componentless | `vX.Y.Z` |
| `component: mods`, `include-component-in-tag: true` | `mods: X.Y.Z` | `mods-vX.Y.Z` |

There is no schema field for a separate display/body component or tag component. The available fields merely expose the coupled behavior ([schema for tag and PR fields](https://github.com/googleapis/release-please/blob/v17.6.0/schemas/config.json#L81-L123)). `tag-separator` cannot remove the component; `TagName.toString()` always emits a non-empty component before its separator ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/util/tag-name.ts#L27-L59)).

### Skip flags do not fix metadata

`skip-changelog` only controls changelog updates. `skip-github-release` makes `buildRelease()` return early ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/strategies/base.ts#L594-L601)). These correctly restrict public artifacts to `mods`, but neither changes the merged PR title/body metadata.

## Release-time parsing makes post-editing risky

On a later run, release-please finds merged release PRs and runs every configured package strategy against the same merged PR ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/manifest.ts#L1178-L1208)). `BaseStrategy.buildRelease()`:

1. parses the title with the package title pattern, then the group title pattern;
2. parses the body;
3. for a multi-component PR, finds the release datum whose normalized component equals `getComponent()`; and
4. prefers that body datum's version over the title version before constructing the tag ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/strategies/base.ts#L594-L718)).

This has an important consequence. If a workflow simply changes the public summary from componentless to `mods: X.Y.Z` while retaining `include-component-in-tag: false`, the public strategy still searches for component `''`. It does not find `mods`, logs that the PR contains no release for its component, and skips the public GitHub Release. Putting the version in the title does not rescue it because the body-component lookup returns first.

The parser format is also intentionally narrow:

- title matching is anchored; `${version}` must begin with a digit (an optional literal `v` is accepted), and component/branch characters are limited ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/util/pull-request-title.ts#L22-L61));
- the body must preserve the release-please `---` delimiters ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/util/pull-request-body.ts#L82-L115)); and
- multi-release summaries must match either `component: semver` or bare `semver` inside `<details><summary>…</summary>` ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/util/pull-request-body.ts#L117-L153)).

A human edit, a regex that changes the wrong same-version entry, altered HTML, or a future renderer change can silently prevent release creation. A duplicate hidden/componentless `mods` entry would make parsing work, but it produces misleading duplicate release notes and relies on the first matching datum. It is not robust.

## Extension and loading options

### Configured plugins are a closed runtime registry

The schema accepts a generic plugin object, but the runtime factory only recognizes registered names. Its built-ins are `linked-versions`, `cargo-workspace`, `node-workspace`, `maven-workspace`, `sentence-case`, and `group-priority`; an unknown name throws `ConfigurationError` ([factory](https://github.com/googleapis/release-please/blob/v17.6.0/src/factories/plugin-factory.ts#L54-L165)). `merge` itself is internal rather than a config-loadable factory plugin.

The package publicly exports both `registerPlugin` and `registerReleaseType` ([exports](https://github.com/googleapis/release-please/blob/v17.6.0/src/index.ts#L29-L54); [release-type registry](https://github.com/googleapis/release-please/blob/v17.6.0/src/factory.ts#L143-L166)). Registration must happen in the same JavaScript process and module instance before `Manifest.fromManifest()` parses/builds the configured strategies and plugins.

### Stock `release-please-action` has no extension input

Action 5.0.0 imports its bundled `GitHub` and `Manifest`, loads the manifest, then calls `createReleases()` and `createPullRequests()` ([action source](https://github.com/googleapis/release-please-action/blob/v5.0.0/src/index.ts#L88-L151)). Its inputs have no custom module/plugin hook ([`action.yml`](https://github.com/googleapis/release-please-action/blob/v5.0.0/action.yml)). Preloading a repository dependency with `NODE_OPTIONS` would not reliably register against the action's bundled copy of release-please. Therefore the normal `uses: googleapis/release-please-action@…` step cannot activate a repository-defined registration.

The action reports `releases_created`, `paths_released`, `prs_created`, `pr`, and `prs`; release fields are root outputs for path `.` and `<path>--…` outputs otherwise ([official output docs](https://github.com/googleapis/release-please-action/blob/v5.0.0/README.md#outputs); [implementation](https://github.com/googleapis/release-please-action/blob/v5.0.0/src/index.ts#L178-L225)). A replacement runner must reproduce any outputs consumed by downstream jobs. In this repository's current workflow no downstream step consumes those outputs, so they need not be recreated until such a consumer is added.

### CLI `--plugin` is usable only with a side-effect module in 17.6.0

The 17.6.0 CLI offers `--plugin`, but its loader performs `require(pluginName)` and only checks whether `plugin.init` exists. It never invokes `init()` ([source](https://github.com/googleapis/release-please/blob/v17.6.0/src/bin/release-please.ts#L889-L909)). A CLI extension must therefore register its release type and plugin as a module-load side effect. This is surprising and easy to break. A direct programmatic runner is clearer and testable.

## Options considered

| Option | Result |
| --- | --- |
| Only set `group-pull-request-title-pattern` | Fails: no `.` candidate supplies `${version}`. |
| Add a synthetic `.` package | Does not preserve the original CLI-owned public release, but becomes the clean no-extension solution when a separately versioned aggregate product release is accepted. |
| Set public `include-component-in-tag: true` | Gets `mods` in the body, but incorrectly creates `mods-vX.Y.Z`. |
| Post-edit title and body after the stock action | Looks correct before merge, but the stock release parser cannot match `mods` to its configured empty component and skips the release. |
| Post-edit PR, set all packages to `skip-github-release`, and manually create the public tag/Release when the manifest changes | Can work, but duplicates release selection, notes, idempotency, and action-output logic outside release-please. It is larger than the targeted extension. |
| Fork the full action and patch bundled release-please | Can preserve action inputs/outputs, but carries a bundled-action maintenance and rebuild burden. |
| Programmatic pinned runner plus two small registered extensions | Meets all constraints while retaining release-please's manifest, Cargo, changelog, tag discovery, and GitHub Release behavior. Recommended. |

## Extension design under the original constraints (not adopted)

If the CLI crate itself must again own the public release while retaining the original six constraints, use an exact `release-please@17.6.0` repository dependency and a small Node runner. The runner should register both extensions before it calls `Manifest.fromManifest()` and should then call `createReleases()` followed by `createPullRequests()`, matching the action's order.

### 1. A `public-rust` release type

Derive from the v17.6.0 Rust strategy (the published package includes `build/src`, so a version-pinned internal subpath import is possible). Keep the inherited `getComponent()` behavior unchanged so public tag/release discovery continues to map componentless `vX.Y.Z` tags to `src/presentation/cli`.

Override only the PR/release boundary:

- after `super.buildReleasePullRequest(...)`, set that candidate's title component and sole `PullRequestBody.releaseData` component to `await getBranchComponent()` (`mods`); and
- before delegating to `super.buildRelease(...)`, parse a cloned merged body and make only the `mods` datum componentless in that clone. Do not mutate the original PR passed to the internal strategies.

The first override provides the desired human metadata. The second presents the stock base release parser with the componentless datum it expects, after which the stock code still constructs `vX.Y.Z` because `include-component-in-tag` remains false. Keep this adapter narrow and cover both methods with fixtures using a realistic combined body.

Register it with `registerReleaseType('public-rust', ...)`, and use that release type only for `src/presentation/cli`. Internal packages remain ordinary `rust` strategies.

### 2. A `public-manifest-metadata` plugin

Keep `cargo-workspace` first and keep its merge enabled. Its workspace merge produces a root-path candidate containing all release data. Run the custom plugin after it. The plugin should:

- require exactly one root candidate;
- find exactly one body datum with component `mods`;
- require that datum to have a version; and
- set the root candidate title's component/version to that datum's values.

Fail closed on zero or multiple matches rather than generating an empty or wrong title. The automatic final `Merge` plugin then inherits those fields from the root candidate as designed. Set a group pattern containing all parseable placeholders, for example:

```json
"group-pull-request-title-pattern": "chore${scope}: release${component} ${version}"
```

Register the plugin with `registerPlugin('public-manifest-metadata', ...)` and list it after `cargo-workspace` in `plugins`. Official plugin lifecycle docs confirm configured plugins run after individual releasers and before the final PR ([docs lines 468-493](https://github.com/googleapis/release-please/blob/v17.6.0/docs/manifest-releaser.md#L468-L493)).

### 3. Preserve current artifact policy

Retain:

- internal `skip-changelog: true` and `skip-github-release: true`;
- the external component-prefixed internal tag step;
- public `changelog-path: /CHANGELOG.md`, `skip-github-release: false`, and `include-component-in-tag: false`;
- `updateAllPackages: false`; and
- the individual manifest entries, so versions remain independent.

The resulting combined PR should have a title such as `chore(main): release mods 1.2.3`; a body entry `<summary>mods: 1.2.3</summary>`; a public GitHub tag and Release named `v1.2.3`; internal component-prefixed tags without public GitHub Releases; and only the root changelog updated.

## Validation required before adoption

Pin fixture tests to 17.6.0 and assert all of the following:

1. a multi-crate change builds exactly one PR;
2. its rendered title contains the `src/presentation/cli` manifest version;
3. its body contains exactly one `mods: VERSION` summary;
4. every internal body summary still maps to its own version;
5. feeding the rendered PR back through `Manifest.buildReleases()` yields exactly one candidate release, at path `src/presentation/cli`, with tag `vVERSION` and the `mods` notes;
6. no internal candidate release is created because its strategy is skipped;
7. the root changelog is the only changelog update;
8. previous public `vVERSION` and internal component tags are found on the next run; and
9. malformed or ambiguous public metadata causes the custom plugin/adapter to fail loudly.

These round-trip tests are more important than snapshotting the initial PR. The main failure mode occurs on the later post-merge run, when title/body metadata is parsed back into release candidates.
