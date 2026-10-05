# Commands and configuration — v0.1.0

Examples use PowerShell. Paths and Mod Names are examples to replace with the user's actual values. Mutations require authorization as described in the skill entry point.

## Complete command surface

```text
mods [--environment PATH] [--log-level LEVEL] COMMAND
mods init [--game-install PATH]
mods config list
mods config get KEY
mods config set game-dir VALUE
mods install ARCHIVE_OR_NEXUS_URL [--file ID] [--name NAME] [--replace] [--choice GROUP=OPTION]... [--dry-run]
mods conflicts list [--compare-content]
mods conflicts inspect MOD_NAME [--compare-content]
mods conflicts explain PATH [--compare-content]
mods exec [--output-target NAME] [--cwd PATH] -- PROGRAM [ARGS]...
```

Global `--environment` and `--log-level` also work after subcommands. Log levels are `trace`, `debug`, `info` (default), `warn`, `error`, `off`. `-h`/`--help` show help; root `-V`/`--version` shows version. Clap's `help` subcommand also provides command help, for example `mods help install`.

```powershell
mods --version
mods --help
mods config set --help
mods install --help
mods --environment 'D:\Mod Environments\Mojave' --log-level off config list
```

The released surface has no `--json`, MCP command, conflict-resolution command, uninstall command, or mod enable/reorder command. Use supported commands and installed help rather than inventing flags.

## Initialization and settings

`init` optionally accepts `--game-install PATH`; see [setup](setup.md) for selection and prerequisites. `config list` reads non-secret settings. `config get KEY` accepts exactly:

| Key | Meaning |
| --- | --- |
| `schema-version` | Environment Manifest schema |
| `name` | Optional Mod Environment display name |
| `steam-app-id` | Bound game's Steam identity |
| `game-dir` | Effective Game Installation path |
| `observed-build-id` | Recorded Steam build |

The optional `nexus_api_key` in `mods.toml` is secret and is not a queryable key. `MODS_NEXUS_API_KEY` overrides it. See the [Nexus installation requirements](installation.md#nexus-url-input).

Only `game-dir` has a CLI setter. Do not infer setters for the other keys.

```powershell
mods --environment 'D:\Mod Environments\Mojave' config get observed-build-id
# After approval:
mods --environment 'D:\Mod Environments\Mojave' config set game-dir 'E:\SteamLibrary\steamapps\common\Fallout New Vegas'
```

Inspect output fields `source`, `manifest_value`, `manifest_path`, `shadowed`, and `writable` as well as the effective value. `MODS_GAME_DIR` can shadow the stored path; changing the manifest does not clear the inherited override. An effective override with a different observed build can produce `warning: MODS_GAME_DIR build does not match observed-build-id`. Ask before changing either the binding or the environment variable.

## Output contract

CLI query and preview output is line-oriented text (`key = value`, indexed fields and counts). Strings use JSON quoting, but the output is **not a JSON document**. Settings can show `unset`. Successful `init` and `config set game-dir` have empty stdout; warnings can still appear on stderr.

Installation has three distinct successful outcomes; read [installation](installation.md). Conflict reports can contain invalid resolution and scoped problems even on exit zero; read [conflicts and execution](execution.md). Capture stdout, stderr, and status together. [Troubleshooting](troubleshooting.md) defines statuses and diagnostics.
