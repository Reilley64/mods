# Fallout: New Vegas normal Quit to Desktop: evidence and limits

Research date: 2026-09-28. Research only; no game launches, fixture changes, fixes, or acceptance changes.

## Conclusion

A zero `WinMain` return does **not** prove clean process termination. The matching upstream xNVSE loader logs that return before returning to its caller. Windows can still execute DLL shutdown code. ReShade **6.8.0** contains an explicit warning about add-ons causing a crash on exit. These are concrete reasons not to equate the recorded `WinMain(0)` with the final process status.

There is also a developer-maintained, New Vegas-specific **Fast Exit** implementation in NVTF. It bypasses normal teardown with `TerminateProcess(..., 0)`. This proves that a fast-exit path exists for this game, **not** that vanilla New Vegas commonly crashes on normal quit, nor that this run hit a known game bug. NVTF was not established as installed in the recorded fixture. No primary source reviewed identifies this exact fixture's failure or rules out `mods`/its integration.

## Scope and recorded observations

The parent investigation supplied these observations; this note did not independently reproduce them:

- The fixture contains NVSE 6.4.8 + ZeGaryHax, DXVK 3 HDR, and ReShade 6.8 + framepacer.
- An in-game save was observed in the Mod Environment-owned Profile State.
- The loader logged a zero return from `WinMain`.
- The final CLI status was `0xC0000005`.
- No WER dump was matched to the recorded run.

A fork name, version string, or presence in the fixture does not establish the exact upstream build, loaded code path, active setting, or faulting module.

## Primary-source findings

### 1. xNVSE distinguishes menu exit from other exit paths

In upstream **6.4.8**, pinned commit `062bccb15abd0397aaeb0a2cf58d7c3ca6140618`, the plugin API declares:

```cpp
kMessage_ExitGame,               // exit to windows from main menu or in-game menu
kMessage_ExitToMainMenu,         // exit to main menu from in-game menu
```

It separately declares:

```cpp
kMessage_ExitGame_Console,      // exit game using 'qqq' console command
```

Sources: [PluginAPI.h, lines 213–239](https://github.com/xNVSE/NVSE/blob/062bccb15abd0397aaeb0a2cf58d7c3ca6140618/nvse/nvse/PluginAPI.h#L213-L239); [menu/console hooks and dispatch, lines 1068–1120](https://github.com/xNVSE/NVSE/blob/062bccb15abd0397aaeb0a2cf58d7c3ca6140618/nvse/nvse/Hooks_Gameplay.cpp#L1068-L1120).

**Strength:** Direct, version-pinned evidence that normal menu quit has an explicit plugin notification path. It is not evidence that a callback failed. A console quit or process kill is not equivalent evidence for the menu-quit path.

### 2. The zero-return log is an intermediate boundary

The same 6.4.8 source says exactly:

```cpp
int result = g_hookedWinMain(hInstance, hPrevInstance, lpCmdLine, nCmdShow);

_MESSAGE("returned from winmain (%d)", result);

return result;
```

Source: [steam_loader/main.cpp, lines 48–67](https://github.com/xNVSE/NVSE/blob/062bccb15abd0397aaeb0a2cf58d7c3ca6140618/nvse/steam_loader/main.cpp#L48-L67).

**Strength:** Strong explanation of the log's meaning in upstream 6.4.8. It records the wrapped function's return, not completed process teardown. The exact deployed loader binary was not source-matched here, and the separate ZeGaryHax plugin was not audited. Even with a matching current-run log, this alone cannot locate the subsequent fault, identify the faulting thread, or exclude earlier memory corruption.

The official [6.4.8 release](https://github.com/xNVSE/NVSE/releases/tag/6.4.8) links its [changelog](https://github.com/xNVSE/NVSE/wiki/xNVSE-6.1,-6.2,-6.3,-6.4:-What's-New#648-changelog). The reviewed 6.4.8 section lists form/3D events, an `ar_Cat` fix, and UI/vector/quaternion commands; it makes no exit-crash claim. That is a bounded negative finding, not proof that the version has no exit defect.

### 3. Late cleanup is real, but is not a game-specific diagnosis

Microsoft documents the following steps in [`ExitProcess`](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-exitprocess):

> “The entry-point functions of all loaded dynamic-link libraries (DLLs) are called with DLL_PROCESS_DETACH.”

> “After all attached DLLs have executed any process termination code, the ExitProcess function terminates the current process, including the calling thread.”

Microsoft also describes a specific possible hang:

> “If one of the terminated threads in the process holds a lock and the DLL detach code in one of the loaded DLLs attempts to acquire the same lock, then calling ExitProcess results in a deadlock.”

Its [CRT termination documentation](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/exit-exit-exit?view=msvc-170) states:

> “The exit function calls destructors for thread-local objects, then calls—in last-in-first-out (LIFO) order—the functions that are registered by atexit and _onexit, and then flushes all file buffers before it terminates the process.”

**Strength:** Authoritative platform mechanisms. They explain why late faults or hangs remain possible after application-level return. They do not establish this executable's exact CRT path, that a lock deadlock happened here, or that a particular DLL is responsible. The supplied final exception-like status is not itself evidence of a hang.

### 4. ReShade 6.8.0 explicitly warns about exit-time add-on state

Official tag `v6.8.0`, commit `18deaa52de0c425a78b329e9cb3c497281cd00ec`, contains this `DLL_PROCESS_DETACH` branch:

```cpp
reshade::log::message(reshade::log::level::info, "Exiting ...");

#if RESHADE_ADDON >= 2
if (reshade::has_loaded_addons())
    reshade::log::message(reshade::log::level::warning, "Add-ons are still loaded! Application may crash on exit.");
#endif

reshade::hooks::uninstall();
```

The same branch includes this exact comment:

> “At that point it would return to code that was already unloaded and crash”

The surrounding comments explain a pending `GetMessage` hook call on another thread; the implementation signals an exit event to address that scenario.

Source: [source/dll_main.cpp, lines 375–405](https://github.com/crosire/reshade/blob/18deaa52de0c425a78b329e9cb3c497281cd00ec/source/dll_main.cpp#L375-L405).

**Strength:** Strong, primary, exit-specific evidence for a developer-recognized risk in the version-aligned upstream renderer add-on framework. It is **not New Vegas-specific**. The warning is conditional on the build and add-on state. We have not established that the fixture uses this exact source, that the warning occurred in this run, or that framepacer caused a fault. The defensive hook comment describes a scenario the code attempts to handle, not proof of an outstanding bug in this run.

### 5. NVTF has a game-specific fast-exit bypass

The developer repository's [README](https://github.com/WallSoGB/New-Vegas-Tick-Fix/blob/f442e43d5b82cd229d5ea84ff181142e8a233ec6/README.md) lists “Fast Exit.” under current features. At that pinned revision, its implementation is:

```cpp
void FastExitHook() {
    StartMenu* pStartMenu = StartMenu::GetSingleton();
    if (pStartMenu && pStartMenu->GetSettingsChanged())
        pStartMenu->SaveSettings();

    TerminateProcess(GetCurrentProcess(), 0);
}
```

Source: [FastExit.cpp](https://github.com/WallSoGB/New-Vegas-Tick-Fix/blob/f442e43d5b82cd229d5ea84ff181142e8a233ec6/nvtf/internal/FastExit.cpp). The feature is gated by `Setting::bFastExit` in [main.cpp](https://github.com/WallSoGB/New-Vegas-Tick-Fix/blob/f442e43d5b82cd229d5ea84ff181142e8a233ec6/nvtf/main.cpp). A historical developer commit, [“Alpha 6 update / Updated Fast Exit hook”](https://github.com/WallSoGB/New-Vegas-Tick-Fix/commit/ba5bea616ac59cfa49ef34a5be34be36228a38fd), also shows the settings-save step before forced termination.

Microsoft's `ExitProcess` documentation explicitly contrasts the two termination paths:

> “In contrast, if a process terminates by calling TerminateProcess, the DLLs that the process is attached to are not notified of the process termination.”

**Strength:** Direct evidence of a New Vegas-specific teardown bypass, including an explicit zero status. The reviewed primary NVTF sources do **not** say this feature fixes a particular crash/hang, establish prevalence, or identify its cause. They must not be cited as proof that the recorded crash is expected vanilla behavior. NVTF installation or activation was not observed in this fixture. A hypothetical clean status after enabling this bypass would not prove the original cleanup defect was repaired. No change is recommended or authorized by this note.

### 6. Stewie Tweaks and DXVK: attribution not established

The [Stewie Tweaks developer listing](https://www.nexusmods.com/newvegas/mods/66347) and its changelog tab returned HTTP 403 during this research. No inspectable developer quotation or version-matched source was obtained establishing a Stewie fast-exit fix for this scenario. The NVTF README credits lStewieAl among feature developers, but that does not establish the same feature in the separately distributed Stewie Tweaks mod. Do not relabel the NVTF implementation as Stewie Tweaks evidence.

Targeted public upstream issue searches for exit-specific New Vegas reports in [DXVK](https://github.com/doitsujin/dxvk) and [WallSoGB's fork](https://github.com/WallSoGB/dxvk) did not produce a version/setup-matched explanation. This was not an exhaustive audit. “DXVK 3 HDR” alone is insufficient to select a source revision or attribute a fault. General rendering instability, compatibility advice, or unrelated fixes would not answer the shutdown question and are not used as evidence here.

## What the original stopped run proves—and does not prove

This section describes only `issue33-game-20260928T010848Z-1864`, before the
later human save/reload and weather cycles. It is not the current acceptance
status. See the [issue #33 evidence ledger](../acceptance/issue-33.md) for the
subsequent bounded passes and remaining requirements.

| Observation | Bounded interpretation | Not established |
| --- | --- | --- |
| Save observed in owned Profile State | Credit the observed save destination/persistence check. The parent contract review says persistence is required after both zero and nonzero exits. | Successful real reload, a second Mod Environment's isolation, all save paths, visual mod effects, or performance acceptance. |
| `WinMain` returned zero | The wrapped function returned zero, assuming the source/log correspondence above. | Clean teardown or final process status zero. |
| CLI status `0xC0000005` | It is consistent with the Windows access-violation exception code. If the root status was forwarded unchanged, it is compatible with abnormal root-process termination after the zero-return log. | Faulting module/thread/address, access type, first corrupting operation, or which component caused it. The CLI status alone does not identify where it originated. |
| No matched WER dump | No matched dump is available for fault localization in this evidence set. | Absence of a crash, absence of an exception, or proof that WER did/did not run. |

Microsoft's [`GetExitCodeProcess` documentation](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getexitcodeprocess) lists possible final statuses, including the `main`/`WinMain` return and:

> “The exception value for an unhandled exception that caused the process to terminate.”

Microsoft's [Access Violation C0000005 diagnostic reference](https://learn.microsoft.com/en-us/shows/inside/c0000005) distinguishes read, write, and execute access violations using exception parameters and calls for the exception context and call stack. None can be recovered from the numeric status alone. A program can also explicitly choose a status value; the number is not a substitute for a captured exception record.

## Decision boundary

- **For bounded mapping/save-routing credit:** A crash-dump detour is not intrinsically necessary to credit the already observed owned-save destination and persistence. The parent's contract review explicitly allows persistence checks on unsuccessful exits. Do not discard that evidence merely because the process status is nonzero.
- **For a clean normal-quit claim or crash attribution:** Current evidence is insufficient. A matched fault record/dump and stack, or other controlled discriminating evidence, would be needed to locate the failure. Even a stack can show where corruption surfaced rather than who first caused it. No claim that this is unrelated to `mods` is justified yet.
- **For overall acceptance:** At this original-run checkpoint, real reload, selected-environment save separation, mod-effect and performance checks were incomplete. Later human cycles established bounded save/reload, visible-effect and qualitative-performance credit; they did not establish a clean exit or identify the fault. The [current evidence ledger](../acceptance/issue-33.md) owns the remaining requirements. The known limitations are neither a blanket failure of every observed behavior nor an acceptance waiver.

No fixes, forced-exit settings, package changes, automatic cleanup, issue changes, or fixture changes follow from this note. It records a plausible late-shutdown explanation and its evidentiary limits, not a root-cause verdict.

## Research coverage

Primary source retrieval used read-only public HTTP. Web search was unavailable because Serper was not configured. Nexus blocked retrieval. Source inspection covered upstream xNVSE 6.4.8, official ReShade v6.8.0 shutdown code, the NVTF developer repository and historical fast-exit change, and Microsoft termination/exception documentation. No authoritative Bethesda diagnosis of a general vanilla New Vegas Quit-to-Desktop defect was obtained. Absence of a finding in this bounded search is not evidence of absence.
