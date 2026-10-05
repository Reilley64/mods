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
