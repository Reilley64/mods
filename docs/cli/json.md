# JSON output

Use `mods --json COMMAND` to request one final JSON document. The flag is global and explicit. Redirection and piping do not turn it on. Pass child arguments after `exec --`; a child `--json` is not a CLI flag. Success goes to stdout. An ordinary failure goes to stderr. JSON mode does not ask for input or grant approval.

The [schema](schema.json) and [examples](examples) define the public shape. Validate a document against the schema definition for its command. The aggregate schema uses `anyOf` because successful mutations permit additive fields. Each command has its own top-level result. Every success has `warnings`, an ordered array of `{code, message, details}` objects. Quiet mutations return `{"warnings":[]}`. `config list` returns `settings`, while `config get` returns `setting`. Settings expose `key`, `value`, `source`, `manifest_value`, `manifest_path`, `shadowed`, and `writable`. Unset values are `null`. A source has a `kind` and, when relevant, `variable` or `argument`.

`conflicts list`, `inspect`, and `explain` return File Conflict results, including `resolution_status`, ordered providers and rows, comparison states, and `problems`. Exit zero can still include problems. `install` returns `outcome` with one of `additional_selections_required`, `preview`, or `installed`. A required-choice response includes accepted choices and unresolved groups. Resubmit the full choice set on the next invocation. A preview includes the Install Plan and hypothetical enabled File Conflicts. An installed response acknowledges completion. Exit zero alone does not establish completion.

`--json --help` returns `{ "help": "...", "warnings": [] }`. `--json --version` returns `{ "version": "...", "warnings": [] }`. Help text is human-readable, not a command catalogue.

Errors use [CLI Problem Details](problems.md): `type` is a permanent repository Markdown link and the primary machine identifier; `title` is a stable label; `detail` describes this occurrence. `code` repeats the type identifier, and `exit_code` gives the process result. `details` contains allowlisted fields such as `phase`, `field`, `group_id`, `option_id`, `sequence`, and build IDs. `instance` is present only when a Diagnostic Session identifier exists. Warnings may appear on a failure. There is no HTTP `status` field. Raw internal error chains are not public output.

`exec` is the exception. The launched child writes live, binary-safe stdout and stderr to the same respective streams in both modes. It inherits stdin. The CLI adds no success document. A nonzero child exit remains the child's exit code and does not become Problem Details. Before launch, CLI failures are JSON Problem Details. Once launched, warnings and supervision or cleanup failures are plain stderr; a supervision or cleanup failure exits nonzero. No post-launch JSON document is mixed with child output.

The JSON contract follows the product version. Breaking changes need a major version bump. Additive fields, warning codes, and problem types may appear without one. Consumers must ignore unknown additions. Removed fields, changed types or meanings, and new values for closed enums such as `outcome` are breaking changes. Human-readable wording is not stable API. There is no separate schema-version field.

## Validation limits

The CLI unit tests cover JSON publication and pre-launch versus post-launch error routing on macOS. Native Windows child stdout/stderr byte forwarding, inherited stdin, and actual child exit status need a Windows runtime run. Those checks are blocked on macOS, not passed.
