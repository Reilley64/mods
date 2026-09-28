# MO2 profile state and provider metadata (#33)

## Answer and scope

MO2 keeps profile state in a separate profile directory and maps selected files to the game's expected user directories. It does not map the whole profile root to Data. Saves have a separate recursive mapping. Mod metadata is different: `mods/<mod>/meta.ini` stays physically inside the mod provider directory.

**Important correction: stable MO2 v2.5.2 does not include `meta.ini` in its default usvfs suffix skip list.** The default is only `.mohidden`; the directory skip default is `.git`. MO2 filters root `meta.ini` from internal file-tree views, but its actual launch path supplies recursive provider-directory links, not those filtered trees. Under those reviewed defaults, root `meta.ini` is not excluded from the recursive native Data links. Do not describe the internal-tree filter as a native visibility guarantee.

Read-only static research, verified 2026-09-26. Read the existing integration, provenance and view-mapping reports. No downloaded program execution, builds, runtime tests, fault tracing, native edits, source edits or pin changes. Only this report was written. Claims concern the official stable source path and default settings, not an audited user's customized installation or every third-party plugin.

## Exact stable sources

The official [v2.5.2 release](https://github.com/ModOrganizer2/modorganizer/releases/tag/v2.5.2) [commit manifest](https://github.com/ModOrganizer2/modorganizer/releases/download/v2.5.2/Mod.Organizer-2.5.2-commits.txt) records:

| Component | Manifest identity used here |
|---|---|
| modorganizer | `9c130cbf2fc7225fb2916e46419af50671772aa0` (source citations use `v2.5.2`) |
| game_falloutnv | `52ea004207cd3834d65290b2b0e34f7132858982` |
| game_gamebryo | `0076e5431bd7fffb4977c724a92027e6e4f11f2e` |
| tool_inibakery | `de678f72ea8bf13c72456644d81da5b93c82274d` |
| usvfs | `v0.5.0`, resolved in the provenance report to `9f7fd9660d51784aa2117cb45f2095e87312d558` |

These are stable plugin pins, not moving master. See [version provenance](issue-33-mo2-usvfs-version.md) for the distinction between dependency label v0.5.0 and shipped DLL resource version 0.5.6.1. This report does not equate stable usvfs with mods' newer fork.

## Profile layout and game-facing routes

`profiles/<name>` means the configured profiles base, not necessarily a hard-coded installation-relative path. The constructor creates that directory, opens `settings.ini`, touches `modlist.txt` and `archives.txt`, and delegates game-specific initialization. [Profile construction](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/profile.cpp#L76-L109).

| Profile entry | Purpose and route |
|---|---|
| `modlist.txt` | MO2 mod enablement/priority state. MO2 reads it and enumerates active providers in priority order; it is not one of FalloutNV's game-facing named mappings. |
| `plugins.txt`, `loadorder.txt` | Gamebryo reads/writes these per-profile lists. FalloutNV maps each individually to Local AppData `FalloutNV/<same name>`, independently of the local-INI/local-save toggles. The Epic variant adds a second destination using its game-directory name. This is availability to the process, not a claim that the engine consumes `loadorder.txt` as its native load-order authority. |
| `fallout.ini`, `falloutprefs.ini`, `falloutcustom.ini`, `GECKCustom.ini`, `GECKPrefs.ini` | FalloutNV's exact declared INI list. With local settings enabled, INI Bakery maps each profile basename to `game->documentsDirectory()/iniFile`. Gamebryo defines that directory as its My Games directory. With local settings disabled, this mapper emits no INI links. |
| `saves/` | Only local saves enabled causes OrganizerCore to add the LocalSavegames mapping. Gamebryo maps the whole subtree to `My Games/<game>/__MO_Saves`, with directory and creation-target flags. |
| `settings.ini`, `archives.txt`, `lockedorder.txt`, tweak/backup files; `savepath.ini` when needed | Manager/profile bookkeeping, not a recursive profile-root projection. `settings.ini` stores the local-profile choices; `savepath.ini` backs up save-related INI settings. This is not an exhaustive list of every plugin's auxiliary file. |

Sources: [modlist read](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/profile.cpp#L424-L434), [active providers](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/profile.cpp#L604-L613), [list filenames](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/profile.cpp#L945-L967); [Gamebryo list read/write](https://github.com/ModOrganizer2/modorganizer-game_gamebryo/blob/0076e5431bd7fffb4977c724a92027e6e4f11f2e/src/gamebryo/gamebryogameplugins.cpp#L24-L60); [FalloutNV named mappings](https://github.com/ModOrganizer2/modorganizer-game_falloutnv/blob/52ea004207cd3834d65290b2b0e34f7132858982/src/gamefalloutnv.cpp#L311-L326); [five INIs](https://github.com/ModOrganizer2/modorganizer-game_falloutnv/blob/52ea004207cd3834d65290b2b0e34f7132858982/src/gamefalloutnv.cpp#L276-L280); [conditional INI mappings](https://github.com/ModOrganizer2/modorganizer-tool_inibakery/blob/de678f72ea8bf13c72456644d81da5b93c82274d/src/inibakery.cpp#L78-L95); [My Games directory](https://github.com/ModOrganizer2/modorganizer-game_gamebryo/blob/0076e5431bd7fffb4977c724a92027e6e4f11f2e/src/gamebryo/gamegamebryo.cpp#L79-L86); [save and plugin mapping composition](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L2071-L2099).

### Toggles are independent of the named plugin-list mappings

`LocalSaves` and `LocalSettings` are profile settings, with global defaults used when absent. Enabling local saves creates `saves/`; disabling can retain those physical files. Enabling local settings asks the game plugin to initialize configuration. FalloutNV initialization copies the five INIs (using `fallout_default.ini` for the main INI when defaults are requested or the real file is absent), plus `plugins.txt` for the MODS flag. Gamebryo's copy helper does not overwrite existing profile files; if copying fails, it tries to create an empty destination. File presence alone is therefore not the toggle or routing decision. [toggle implementation](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/profile.cpp#L850-L941); [profile initialization](https://github.com/ModOrganizer2/modorganizer-game_falloutnv/blob/52ea004207cd3834d65290b2b0e34f7132858982/src/gamefalloutnv.cpp#L208-L228); [copy helper](https://github.com/ModOrganizer2/modorganizer-game_gamebryo/blob/0076e5431bd7fffb4977c724a92027e6e4f11f2e/src/gamebryo/gamegamebryo.cpp#L267-L285).

The game normally asks for its expected paths. Injected-process usvfs mappings route those paths to selected profile backing files; it need not know `profiles/<name>`. Connector directory mappings use `RECURSIVE` and optional `CREATETARGET`; named files use file links with flags 0. [connector translation](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L211-L232).

For saves, FalloutNV registers `GamebryoLocalSavegames(this, "fallout.ini")`. INI Bakery calls its `prepareProfile` before launch. That function sets `[General] sLocalSavePath=__MO_Saves\` and `bUseMyGamesDirectory=1`, backing up previous values to profile `savepath.ini`. It edits the profile INI if local settings are enabled, **otherwise the physical My Games INI**. Thus local saves do not require local INIs, and MO2 is not evidence of an always-unchanged physical user configuration. Disabling saves restores/removes those INI values using the backup. [feature registration](https://github.com/ModOrganizer2/modorganizer-game_falloutnv/blob/52ea004207cd3834d65290b2b0e34f7132858982/src/gamefalloutnv.cpp#L38-L48); [before-run callback](https://github.com/ModOrganizer2/modorganizer-tool_inibakery/blob/de678f72ea8bf13c72456644d81da5b93c82274d/src/inibakery.cpp#L16-L22); [profile preparation](https://github.com/ModOrganizer2/modorganizer-tool_inibakery/blob/de678f72ea8bf13c72456644d81da5b93c82274d/src/inibakery.cpp#L60-L74); [save mapping and INI preparation](https://github.com/ModOrganizer2/modorganizer-game_gamebryo/blob/0076e5431bd7fffb4977c724a92027e6e4f11f2e/src/gamebryo/gamebryolocalsavegames.cpp#L34-L139).

No whole-profile-root mapping appears in this reviewed stable core/FalloutNV/INI Bakery path. The save subtree is the deliberate directory exception. Arbitrary extra file-mapper plugins could add their own mappings; this statement is not a universal plugin restriction.

## Mod-root `meta.ini`: physical storage is not exclusion

MO2 reads `m_Path + "/meta.ini"` and writes `absolutePath() + "/meta.ini"` using QSettings. It stores mod information such as comments, notes, game name, Nexus ID and version there. The metadata stays inside the physical mod directory. [metadata read](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/modinforegular.cpp#L85-L95); [metadata write](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/modinforegular.cpp#L246-L259).

Two internal filtering paths must not be confused with native links:

- `QDirRootFileTreeImpl` excludes an **exact basename** `meta.ini`, case-insensitively, only for root files. Subdirectories use the unfiltered base implementation. This is not suffix matching and does not exclude nested `subdir/meta.ini`. [root-only file-tree filter](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/qdirfiletree.cpp#L69-L95).
- `DirectoryRefresher::cleanStructure` removes `meta.ini`, `readme.txt` and `fomod` from its internal root structure. Its caller passes `m_Root`. That alone says nothing about which files a native directory link imports. [internal cleanup](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/directoryrefresher.cpp#L191-L202); [root cleanup caller](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/directoryrefresher.cpp#L488-L494).

The launch builder instead takes each active regular mod's physical root and maps it to Data, then Overwrite and plugin mappings. The connector recurses those physical roots. It does not enumerate the cleaned internal tree to construct these links. [provider-root mapping](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L2046-L2063); [Overwrite/plugin mappings](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L2083-L2099); [recursive flags](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L223-L230).

## Trace the actual exclusion policy

1. Stable `Settings::skipFileSuffixes()` defaults to **only `.mohidden`**, unless overridden by `Settings/skip_file_suffixes`. `skipDirectories()` defaults to **only `.git`**, unless overridden by `Settings/skip_directories`. Neither default contains `meta.ini`. [actual settings defaults](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/settings.cpp#L319-L344).
2. The connector clears native suffix/directory lists and forwards these settings. It ignores empty suffix strings. There is no extra `meta.ini` insertion here. Updating parameters uses the same lists. [initial forwarding](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L163-L176); [updated forwarding](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L269-L282).
3. Stable upstream documents **file suffixes, not extensions**: both `.txt` and `some_file.txt` are valid. Directory exclusions are names, not paths, and apply regardless of directory position. [public skip contract](https://github.com/ModOrganizer2/usvfs/blob/9f7fd9660d51784aa2117cb45f2095e87312d558/include/usvfs.h#L154-L180).
4. Stable upstream implements suffix matching with `boost::algorithm::iends_with` (case-insensitive); directory names use `iequals`. Recursive traversal checks each encountered file basename, and uses the same recursive call for subdirectories. A hypothetical configured suffix `meta.ini` would skip `meta.ini`, `META.INI` and `othermeta.ini` at **every depth**, not just a root metadata file. Explicit file linking also checks the source string against the suffix list. [case and explicit-file behavior](https://github.com/ModOrganizer2/usvfs/blob/9f7fd9660d51784aa2117cb45f2095e87312d558/src/usvfs_dll/usvfs.cpp#L618-L663); [recursive checks and file insertion](https://github.com/ModOrganizer2/usvfs/blob/9f7fd9660d51784aa2117cb45f2095e87312d558/src/usvfs_dll/usvfs.cpp#L732-L781).

**Consequently, under these defaults an ordinary root or nested `meta.ini` reaches the native recursive file insertion path.** It is not excluded merely because MO2 recognizes it as manager metadata. This is static construction evidence, not a measured claim about every read operation. Skip lists also do not delete the physical source or establish an opaque filesystem: omitting a virtual link is not proof of denial of physical-path access or of an already-existing destination.

## Difference from mods

The project's retained rules reserve provider-root `meta.toml` while nested `subdir/meta.toml` remains ordinary content. Its execution policy requires empty executable blacklist, suffix-skip and directory-skip lists; `.mohidden` is ordinary content. See the source-linked requirement matrix in [view mapping model](issue-33-view-mapping-model.md), [#30](https://github.com/Reilley64/mods/issues/30), [#12 decision](https://gist.github.com/Reilley64/a410c7fa1c2dc673a5fb28c3c7c63ba2), and the [pinned shim clear sequence](https://github.com/Reilley64/usvfs-rs/blob/eb4949fb2439fe5b98901e2fb1afceee752a6133/rust/usvfs-sys/native/barrier.cpp#L103-L110).

MO2's on-disk metadata location is analogous, but its default native visibility policy is not proof of our required root-only exclusion. Adding a suffix `meta.toml` would both violate the empty-list policy and exclude nested/longer-suffix filenames. Copying MO2's internal-tree filter does not filter a recursive native provider link automatically. Likewise, MO2's named profile-file mapping precedent does not imply that whole Data providers need per-file links, or that the whole profile root should be mapped.

**Next step:** use these verified facts to correct the comparison and discuss any desired policy change explicitly. No mapping rewrite, native change, pin change or fault investigation follows from this research alone.
