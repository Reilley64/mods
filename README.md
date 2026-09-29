# mods

A Windows CLI for isolated Fallout: New Vegas mod setups. Each Mod Environment has its own Data Mods, mod order, plugin state, INIs, saves, and writable output. A Steam Game Installation supplies the shared base.

## Install the CLI

Requires Windows 11 x64, a Steam installation of Fallout: New Vegas, and both x86 and x64 Microsoft Visual C++ 2015-2022 Redistributables.

```powershell
winget install Reilley64.Mods
mods --version
mods --help
```

If the terminal cannot find `mods` after installation, open a new terminal. For ZIP installation, runtime files, and setup errors, see the [setup guide](skills/mods-cli/references/setup.md). Release binaries and corresponding source are available on [GitHub Releases](https://github.com/Reilley64/mods/releases).

## Start a Mod Environment

Replace these paths with a new Environment Root and your Steam Game Installation:

```powershell
mods --environment 'D:\Mod Environments\Mojave' init --game-install 'D:\SteamLibrary\steamapps\common\Fallout New Vegas'
mods --environment 'D:\Mod Environments\Mojave' config list
```

Use the same `--environment` path for later commands. Without it, mods uses `%LOCALAPPDATA%\mods\environments\default`.

Preview a Data Mod installation before writing files:

```powershell
mods --environment 'D:\Mod Environments\Mojave' install 'D:\Downloads\Textures.zip' --name 'Mojave Textures' --dry-run
```

Review the Install Plan, then rerun without `--dry-run` to install. If the archive requires FOMOD Choices, follow the [installation guide](skills/mods-cli/references/installation.md). Exit zero can mean more choices are required or a preview is ready, rather than a completed installation.

New Data Mods start disabled. The released CLI has no enable command. Installation alone does not make a mod participate in the Virtual Game View.

## Commands and limits

- `init` creates a Mod Environment.
- `config` reads settings or changes the Game Binding.
- `install` previews, installs, or replaces a Data Mod.
- `conflicts` lists File Conflicts, inspects a Data Mod, or explains a Data-relative path.
- `export` previews or publishes a resolved Data/Profile payload for manual placement. See the [export guide](skills/mods-cli/references/export.md).
- `exec` runs a game or tool with a Virtual Game View. Overwrite is the default Output Target.

Run `mods <command> --help` for the installed command's options. See the [command reference](skills/mods-cli/references/commands.md) for examples.

Conflict analysis does not inspect BSA members or prove runtime compatibility. Managed execution does not guarantee analytical Tombstone suppression or enforce projected plugin order through virtual timestamps. Read the [conflict and execution guide](skills/mods-cli/references/execution.md) before relying on those results.

Preview before exporting. The output must be a new folder outside the Environment Root and Game Installation. Export does not install into a game or launch it.

## Install the agent skill

The `mods-cli` skill gives your coding agent a CLI reference, including FOMOD Choices, Install Plan previews, and approval rules for changes and program execution.

With Node.js and npm available, run:

```sh
npx skills add Reilley64/mods --skill mods-cli
```

Choose your agent and installation scope when prompted. This installs instructions for the agent, not the CLI. To update installed skills:

```sh
npx skills update
```

The skill uses `v0.1.0` as its tested reference. It allows minor and patch version differences when installed help confirms the requested commands and options. Read the [skill](skills/mods-cli/SKILL.md) or learn about [skills.sh](https://skills.sh/docs).

## AI in development

We use AI agents to research changes, write code and tests, update documentation, and review code. Maintainers set the scope and remain responsible for what ships. Agents follow the repository's [coding standards](CODING_STYLE.md) and task instructions in [AGENTS.md](AGENTS.md).

We check changes through human code reviews, compiler checks, formatting, linting, and tests. An AI-assisted style check reviews Rust changes against the coding standards. These checks and agent reviews can miss bugs. They do not replace testing with the game. Agents must report checks they could not run and limits in what they tested.

The optional [agent skill](skills/mods-cli/SKILL.md) lets your agent use the CLI. You can use the CLI without an AI agent.

## Documentation

- [Troubleshooting and diagnostics](skills/mods-cli/references/troubleshooting.md)
- [Report a defect](https://github.com/Reilley64/mods/issues/new?template=defect.yml)
- [Domain glossary](CONTEXT.md)
- [Reference evidence and validation limits](skills/mods-cli/references/validation.md)
- [Contributing](CONTRIBUTING.md)
- [Changelog](CHANGELOG.md)

## License

mods is licensed under [GPL-3.0-or-later](LICENSE). See [copyright and license details](COPYRIGHT.md) and the [upstream notices](licenses/usvfs).
