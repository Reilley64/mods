# v0.1.0 additive runtime preparation (not publication)

## Frozen contract

This one-time recipe repacks the approved existing v0.1.0 ZIP. It does not build
Rust/native code, change dependencies, rewrite source, or modify a release.
It adds no publication command. Legacy `scripts/package-windows.ps1`,
`scripts/stable-release.ts`, workflows and Winget automation remain unchanged.
The old ZIP, `SHA256SUMS`, tag and all source/native bytes remain immutable.
Only the new runtime's `BUILD-AND-SOURCE.md` changes.

The script accepts only original ZIP SHA-256
`a1c01c39412f34cea07c6a26c944cc3303e2f4451872049256a99a9b48231610`.
It preserves the complete embedded source verbatim as
`mods-v0.1.0-source.tar.gz`: 99,068,488 bytes, SHA-256
`ba8bf1b99786a4addbed6c99235bbc87ee04a0f07edb7a8d204c1344efc56cca`,
revision `1635e408c913181c15b518bc95339ebdd6926169`. No fixtures are removed,
renamed, encrypted or filtered. It is not a source-rebuild recipe.

## Local commands

Use repository Bun and existing bsdtar with ZIP support (the same `tar -a -cf`
interface used by the Windows packager). PowerShell is not needed to prepare
bytes. There are no new dependencies. The parent output directory must exist;
the output path itself must not exist, even if empty. All failures retain any
created files for inspection. Retry with a different fresh path, never overwrite.

```text
bun scripts/repack-v010-runtime.ts prepare <original.zip> <fresh-output-directory>
bun scripts/repack-v010-runtime.ts verify <original.zip> <output-directory>
bun test tests/runtime-repack.test.ts tests/distribution.test.ts tests/stable-release.test.ts
bun run check:tools
```

`prepare` checks the input hash and exact inventory before writing. It creates:

- `mods-v0.1.0-runtime-x86_64-pc-windows-msvc.zip` (23 regular files).
- `mods-v0.1.0-source.tar.gz` (unchanged source member).
- `mods-v0.1.0-runtime-SHA256SUMS` (new checksum identity).
- `repack-receipt.json` (local evidence, not a release asset).
- `work/` (retained runtime staging, not a release asset).

The checksum text is exactly two lowercase SHA-256 lines, runtime first, source
second, two spaces before each exact filename and one LF after each line. It is
not the legacy `SHA256SUMS` and must never replace or extend that file.

`verify` is read-only. It checks ZIP format, exact case-sensitive member and
directory names with no duplicates, all 22 unchanged runtime/license files
against the **pinned original ZIP**, the exact replacement document in
`docs/runtime-v0.1.0-BUILD-AND-SOURCE.md`, the source hash/size/revision and the
exact two-line checksum. It does not trust a receipt or staging directory.
The receipt records input/output size and SHA-256 per runtime file, source
mapping, artifact identities and preparation tool versions. ZIP metadata and
compression can differ between hosts; equal runtime member bytes do not imply
identical ZIP bytes or Windows extractor compatibility.

## Approval and evidence boundaries

These commands establish local byte identity only. They do not prove public
availability, legal completeness, security approval, a scanner pass, native or
Rust reproducibility, or Windows installation/game behavior. PowerShell is not
available on the preparation host. Existing-host Winget tar success, default
Shell failure, non-pristine-host limits, and the disconnected-rebuild waiver
remain historical limitations, not new passes. The untouched worktree baseline
has 38 reported Rust findings; they are unconfirmed/out of scope, not a clean
Rust review. Do not bypass or override any enforcing gate.

Before any separately approved publication or Winget change:

1. Review the recipe and independently audit all file/source mappings.
2. Validate the candidate on Windows, including the actual ZIP extractor and
   bounded install/help/version/alias/uninstall evidence. Game tests are outside
   this preparation approval.
3. Obtain separate approval to add the three new assets, without altering old
   assets. Source must be public first; an incomplete set is not ready. Inspect
   conflicts and delayed visibility rather than overwrite. No upload automation
   is provided here.
4. Anonymously verify the exact public runtime, source and checksum bytes, and
   add prominent **Complete Corresponding Source for mods v0.1.0** directions
   beside the runtime on the release page. Use the exact URL/hash/revision in
   the in-package instructions. Retain all published assets indefinitely.
5. Only after separate approval, update and validate the human-controlled
   Winget manifest. Automated Winget remains disabled. A scanner failure must
   follow Microsoft's normal resolution path, not a bypass or assumed waiver.

The legacy stable verifier still verifies only the original all-in-one ZIP.
Its success says nothing about this variant. Public complete-set verification
and Windows acceptance remain later work; these local commands do not replace
those gates.
