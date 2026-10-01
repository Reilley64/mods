# Mod management

This context defines the user-visible concepts for isolated Fallout: New Vegas mod setups and the game view they produce.

## Language

**Mod Environment**:
A complete isolated mod setup selected as one unit. It owns one mod priority order and one isolated set of plugin, INI, save, and writable-output state.
_Avoid_: Profile, instance, VFS folder when referring to the complete setup

**Environment Root**:
The directory that physically contains one Mod Environment and all of its managed state.
_Avoid_: VFS folder, profile directory

**Environment Manifest**:
The `mods.toml` file that marks an Environment Root and records its schema, optional display name, and Game Binding.
_Avoid_: Global registry, profile manifest

**Game Binding**:
The association between one Mod Environment and one Game Installation, including its Steam identity, path, and observed build.
_Avoid_: Profile, global game setting

**Game Installation**:
The Steam-managed Fallout: New Vegas installation that supplies the shared base for one or more Mod Environments. Its file contents stay unchanged, but `exec` sets the modification times of its Data-root plugins and BSAs to enforce the running environment's load order, as it does for plugins and BSAs in Data Mods and Overwrite. Environments that share an installation set these times again on each run, so two of them must not run at the same time.
_Avoid_: Mod Environment, profile

**Profile State**:
The single set of mod order, plugin state, INIs, and saves owned by a Mod Environment and grouped beneath its `profile` directory. `plugins.txt` lists the active plugins. Its line order is the load order. To change the load order, reorder its lines. Profile State has no `loadorder.txt`; an existing one is ignored.
_Avoid_: A separately selectable profile, nested environment

**Data Mod**:
An installed mod whose files contribute only beneath the game's `Data` directory.
_Avoid_: Root patch, executable patch

**FOMOD Choice**:
One ordered group-and-option selection supplied directly to `mods install` through `--choice`. A command carries the full choice state; the manager does not store an installation session.
_Avoid_: Wizard step, session choice

**Install Plan**:
The validated source-to-Data-destination mapping computed before extraction or publication, including destination winner decisions and overlaps on planned paths.
_Avoid_: Installed mod, staging directory, full conflict report

**Mod Name**:
The case-insensitively unique name that identifies a Data Mod in its directory name, mod list, and conflict reports.
_Avoid_: Mod ID, opaque identifier

**Mod Priority**:
The zero-based rank of a Data Mod in the complete `modlist.txt`, which uses Mod Organizer 2 order: the first listed mod has the highest priority and the last listed mod has priority 0. Game Data is always lowest. There is no migration: a list written low-to-high by earlier versions now reads reversed. Only enabled Data Mods participate in the Virtual Game View; Overwrite has a separate implicit highest rank.
_Avoid_: Plugin load order, dependency rank

**File Conflict**:
Two or more unsuppressed active non-base providers containing an ordinary file at the same case-insensitive Data-relative path. Physical BSA files are opaque ordinary files; their members are not part of MVP conflict analysis.
_Avoid_: Directory merge, ordinary Steam-base override, predicted archive-member winner

**Tombstone**:
Canonical provider-owned metadata that suppresses a lower-priority file or inclusive directory subtree during installation and conflict analysis, without supplying a file. In that analysis, the effective path is absent when no higher file overrides the controlling tombstone. Tombstones do not require suppression in managed execution.
_Avoid_: File winner, deletion from Steam Data

**Overwrite**:
The implicit Mod Environment-owned Data provider with highest mod priority. It is the default Output Target for a managed execution.
_Avoid_: Root output, temporary directory

**Output Target**:
Overwrite or one installed, enabled Data Mod selected to receive new Data files and new copy or file-move destinations for one managed execution. Selection does not change Mod Priority or relocate existing destination files from their current provider.
_Avoid_: Profile State, staging directory

**Diagnostic Session**:
The bounded diagnostic scope for one environment-bound CLI command, including its nested work and cleanup. After the MVP, an MCP request or MCP server lifecycle may also have its own Diagnostic Session.
_Avoid_: FOMOD choice session, stored workflow, Mod Environment lifetime

**Virtual Game View**:
The merged game namespace observed by a game or tool, formed from a Game Installation, ordered mods, and Mod Environment-owned files.
_Avoid_: Mod Environment, VFS folder

**MCP Presentation**:
The after-MVP local stdio tool adapter started directly as `mods-mcp.exe` and bound to one Environment Root at process startup. It exposes every approved CLI application use case except Environment initialization and owns no interaction or task session. The CLI-only MVP source and workspace do not include this presentation.
_Avoid_: `mods mcp start`, remote service, authorization boundary, second application core
