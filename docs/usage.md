# Managed execution

Run a program in the selected Mod Environment's Virtual Game View:

```text
mods --environment PATH exec [--output-target MOD] [--cwd PATH] -- PROGRAM [ARGS...]
```

On Windows, add `--hidden` before `--` to detach the launcher console:

```text
mods --environment PATH exec --hidden -- PROGRAM [ARGS...]
```

Windows may briefly display the launcher console before detachment. Closing the launched application is the supported stop mechanism; hidden mode has no console Ctrl-C interaction.

The managed supervisor remains active until the program and its managed child processes finish. The launched application's GUI windows remain visible. A console-only child has no interactive terminal or visible output through the launcher. Windows may allocate a separate console for that child after detachment; use ordinary `exec` for interactive tools. Hidden launch or supervision errors appear in a Windows dialog with a diagnostic log path when logging is available. A child that merely exits with a nonzero status does not show an error dialog. If `mods` shares a caller terminal, detachment does not hide that terminal. On non-Windows platforms, hidden managed execution is unsupported.

# Launch Shortcuts

On Windows, create a native desktop `.lnk` without starting the program:

```text
mods --environment PATH --log-level info shortcut -- PROGRAM [ARGS...]
mods --environment PATH --log-level debug shortcut --name "My tool" --destination links --cwd tools --output-target "Generated Files" -- tool.exe [ARGS...]
```

`shortcut` uses the same executable lookup and child argument separator as `exec`. Put all shortcut options before `--`. Values after the program are child arguments, including empty values and literal `--` values. The caller's shell must pass those values intact. Executable lookup uses the caller's startup directory for relative inputs and the inherited PATH for name-only inputs, never the child working directory.

The default destination is the current user's Desktop Known Folder, including redirected desktops. `--destination` selects an existing directory; relative paths use the creation-time working directory. `--name` is a filename stem, not a path; `.lnk` is appended. Invalid explicit Windows filenames are rejected. By default, the name combines the Environment Manifest display name (or Environment Root folder name) and executable stem, for example `Vanilla Plus — FalloutNV.lnk`. Invalid filename characters in default names are replaced, trailing spaces and periods are removed, and overly long default names are shortened.

A same-named valid `.lnk` is replaced by default. Directories, symbolic links, and non-Shell-Link files are not replaced. The replacement is fully prepared before publication; preparation failures preserve the previous shortcut. Success prints nothing. The command validates the Mod Environment, Game Binding, launch inputs, and Output Target through the same preparation as `exec`, without activating a Virtual Game View or running the game/tool. Like `exec`, that preparation creates a missing `meta.toml` for enabled mods.

The link targets the current `mods` executable with `exec --hidden` and uses the selected executable's icon. It saves absolute locations for `mods`, the Environment Root, the executable, and the child working directory. When `--cwd` is omitted, the child working directory is the bound Game Installation directory, as with `exec`. A relative `--cwd` resolves against the creation-time working directory. It also saves the Output Target, child arguments, and selected log level (default `info`). Encoded saved arguments must fit the Shell Link compatibility limit of 1023 UTF-16 units; longer requests fail without replacing an existing link.

A Launch Shortcut uses the environment's **current** mods and settings, not a snapshot. Hidden launch failures appear in a Windows error dialog, with a log path when available. Console-only tools have no interactive launcher terminal; use ordinary `exec` for them. Close the launched application to stop it. Recreate shortcuts after moving captured executables, directories, or the Environment Root. You may move, rename, or delete the `.lnk` through Windows. Non-Windows shortcut creation is unsupported.
