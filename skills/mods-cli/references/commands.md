# Commands and configuration — v0.1.0

Examples use PowerShell. Paths and Mod Names are examples to replace with the user's actual values. Mutations require authorization as described in the skill entry point.

## Complete command surface

```text
mods [--environment PATH] [--log-level LEVEL] [--json] COMMAND
mods --json init [--game-install PATH]
mods --json config list
mods --json config get KEY
mods --json config set game-dir VALUE
mods --json install ARCHIVE_OR_NEXUS_URL [--file ID] [--name NAME] [--replace] [--choice GROUP=OPTION]... [--dry-run]
mods --json conflicts list [--compare-content]
mods --json conflicts inspect MOD_NAME [--compare-content]
mods --json conflicts explain PATH [--compare-content]
mods --json export OUTPUT [--include-saves] [--include-game-data] [--dry-run]
mods --json exec [--output-target NAME] [--cwd PATH] -- PROGRAM [ARGS]...
mods exec --hidden [--output-target NAME] [--cwd PATH] -- PROGRAM [ARGS]...
mods --json shortcut [--name NAME] [--destination PATH] [--output-target NAME] [--cwd PATH] -- PROGRAM [ARGS]...
```

Global `--environment` and `--log-level` also work after subcommands. Log levels are `trace`, `debug`, `info` (default), `warn`, `error`, `off`. `-h`/`--help` show help; root `-V`/`--version` shows version. Clap's `help` subcommand also provides command help, for example `mods help install`.

```powershell
mods --version
mods --help
mods config set --help
mods install --help
mods --json --environment 'D:\Mod Environments\Mojave' --log-level off config list
```

`exec --hidden` does not accept `--json`. `--json` needs a release that includes JSON output; released v0.1.0 rejects it with exit status 2 and `unexpected argument '--json'`. In that case, omit `--json` from the examples in these references and read the text output. The CLI has no MCP command, conflict-resolution command, uninstall command, or mod enable/reorder command. Use supported commands and installed help rather than inventing flags.

## Initialization and settings

`init` optionally accepts `--game-install PATH`; see [setup](setup.md) for selection and prerequisites. `config list` reads non-secret settings. `config get KEY` accepts exactly:

| Key | Meaning |
| --- | --- |
| `schema-version` | Environment Manifest schema |
| `name` | Optional Mod Environment display name |
| `game-dir` | Effective Game Installation path |

The optional `nexus_api_key` in `mods.toml` is secret and is not a queryable key. `MODS_NEXUS_API_KEY` overrides it. See the [Nexus installation requirements](installation.md#nexus-url-input).

Only `game-dir` has a CLI setter. Do not infer setters for the other keys.

```powershell
mods --json --environment 'D:\Mod Environments\Mojave' config get game-dir
# After approval:
mods --json --environment 'D:\Mod Environments\Mojave' config set game-dir 'E:\SteamLibrary\steamapps\common\Fallout New Vegas'
```

Inspect output fields `source`, `manifest_value`, `manifest_path`, `shadowed`, and `writable` as well as the effective value. `MODS_GAME_DIR` can shadow the stored path; changing the manifest does not clear the inherited override. Ask before changing either the binding or the environment variable.

## Output contract

Without `--json`, CLI query and preview output is line-oriented text (`key = value`, indexed fields and counts). Strings use JSON quoting, but the output is **not a JSON document**. Settings can show `unset`. Without `--json`, successful `init` and `config set game-dir` have empty stdout; warnings can still appear on stderr. With `--json`, each returns an object with a `warnings` array. See [JSON output](https://github.com/Reilley64/mods/blob/main/docs/cli/json.md).

Installation has three distinct successful outcomes; read [installation](installation.md). Export preview lists paths, providers and byte totals; see [export and manual placement](export.md). Conflict reports can contain invalid resolution and scoped problems even on exit zero; read [conflicts and execution](execution.md). Capture stdout, stderr, and status together. [Troubleshooting](troubleshooting.md) defines statuses and diagnostics.
