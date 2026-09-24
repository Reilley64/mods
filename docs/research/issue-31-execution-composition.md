# Issue 31 execution configuration evidence

## Upstream named files and absent user directories

Pinned upstream: `57f1ea5e6ad13f7435a7af184748e6c1312c5637`.

`usvfsVirtualLinkFile` in [usvfs.cpp](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/usvfs.cpp) requires destination parents via `assertPathExists`, then inserts the supplied source path. It does not require the source file to exist. Named optional files can therefore be configured without synthesizing canonical files.

`usvfsVirtualLinkDirectoryStatic` also validates destination parents, then adds the source directory mapping. Nonrecursive identity mappings for the Fallout user-directory ancestors establish virtual containers without creating real user directories, recursively importing their contents, or making them Profile State creation targets. Named files are mapped afterward. Only the configured save directory is a recursive creation target.

This is configuration evidence, not a promise of closed namespaces, unchanged physical providers, or successful descendant injection. Those upstream limitations remain accepted under #30. Runtime interception remains upstream-owned; no additional hooks or injection tests were added.

## Load order

The builder computes the requested order and all activation sources. Execution diagnostics record them. Canonical lists and physical timestamps are not rewritten at launch. Each result warns that game-visible timestamp-based order is not enforced by the upstream adapter.

## Validation boundary

Execution accepts missing plugin lists, never inspects save contents, rejects pending mods-owned work without recovery, and revalidates consumed configuration before process creation. Post-run invalid Profile State is reported without rollback or replacing a child result.

## Platform validation

Host checks cover project-owned configuration, typed application ports, CLI output and mocked process lifecycle. Windows environment/game-platform Rust checks can run cross-target. Full Windows workspace builds and runtime execution require the Windows SDK, MSVC native dependencies, and the exact packaged upstream artifacts described in `native/README.md`. A macOS host result is not Windows execution evidence.
