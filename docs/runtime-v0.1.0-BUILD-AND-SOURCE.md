# Complete Corresponding Source for mods v0.1.0

This runtime-only ZIP is `mods-v0.1.0-runtime-x86_64-pc-windows-msvc.zip`.
It contains the original v0.1.0 executables, libraries and license notices.
No Rust or native code was rebuilt. Only this instruction file changed; the
complete source moved to a separate download without changing any source bytes.
The aggregate release remains v0.1.0, not a new build or source revision.

## Exact source download

- Source: [mods-v0.1.0-source.tar.gz](https://github.com/Reilley64/mods/releases/download/v0.1.0/mods-v0.1.0-source.tar.gz)
- SHA-256: `ba8bf1b99786a4addbed6c99235bbc87ee04a0f07edb7a8d204c1344efc56cca`
- Size: 99,068,488 bytes.
- Original source revision: `1635e408c913181c15b518bc95339ebdd6926169`.
- Same-release checksum asset: `mods-v0.1.0-runtime-SHA256SUMS`.

This is the byte-identical `mods-source.tar.gz` member of the original
`mods-v0.1.0-x86_64-pc-windows-msvc.zip` (SHA-256
`a1c01c39412f34cea07c6a26c944cc3303e2f4451872049256a99a9b48231610`).
The original ZIP, its `SHA256SUMS` and the v0.1.0 tag remain unchanged.
All vendored sources, fixtures, checksums, headers, native archives and notices
remain intact. The source archive's historical instructions describe the old
all-in-one layout; this file explains the new separate download only.

The source must be available anonymously, without charge or a special key,
from the same public release before distributing this runtime variant. Candidate
preparation alone does not establish that these new URLs are public. Retain the
exact source for as long as the runtime is distributed; the project's policy is
to preserve all published assets indefinitely. Do not substitute a moving branch,
automatic GitHub source archive, or source from another version.

Download and check the source in PowerShell 7 from a fresh working directory:

```powershell
$ErrorActionPreference = "Stop"
$source = "mods-v0.1.0-source.tar.gz"
if (Test-Path -LiteralPath $source) { throw "Use a fresh download directory" }
Invoke-WebRequest "https://github.com/Reilley64/mods/releases/download/v0.1.0/mods-v0.1.0-source.tar.gz" -OutFile $source
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $source).Hash -ne "ba8bf1b99786a4addbed6c99235bbc87ee04a0f07edb7a8d204c1344efc56cca") {
  throw "Corresponding source checksum mismatch"
}
if (Test-Path -LiteralPath "mods-v0.1.0-source") { throw "Use a fresh extraction directory" }
New-Item -ItemType Directory "mods-v0.1.0-source" | Out-Null
tar -xf $source -C "mods-v0.1.0-source"
if ($LASTEXITCODE -ne 0) { throw "Source extraction failed" }
Set-Location "mods-v0.1.0-source"
if ((Get-Content -Raw SOURCE-REVISION.txt).Trim() -cne "1635e408c913181c15b518bc95339ebdd6926169") {
  throw "Unexpected source revision"
}
```

## Rebuild and runtime requirements

Follow **Consumer source rebuild** in the extracted `OFFLINE-REBUILD.md` or
`docs/distribution.md`. Preinstall the pinned Rust toolchain, PowerShell 7,
Visual Studio C++ tools, Windows SDK and LLVM/libclang; set `LIBCLANG_PATH`.
Keep `.cargo/config.toml`, `vendor/`, root `include/`, and `native/archives/`
intact. Use the documented fresh Cargo home and target directory. The owner
waived the disconnected rebuild acceptance check: that is a skipped check,
not evidence of an offline or reproducible build.

The Rust consumer recipe uses the pinned native binaries; it does **not** rebuild
usvfs. For that step, use `native/usvfs-release.json` to identify the included
native corresponding-source archive under `native/archives/`, verify its pinned
hash, extract it, and follow `source-candidate/README.md` and its native build
instructions. Preserve its historical evidence and caveats; hash identity alone
is not a native rebuild or legal-completeness finding.

Windows 11 and both x86 and x64 Microsoft Visual C++ 2015–2022 Redistributables
remain runtime prerequisites. Keep the complete `usvfs/` directory beside
`mods.exe`. Source is not required to run the program. This packaging variant
has no self-updater and does not change CLI or game behavior. New Windows
installation, extraction and scanner results must be established separately.

## License and upstream notices

mods is free software under GPL-3.0-or-later, without warranty. See the unchanged
root `LICENSE` and `COPYRIGHT.md`. Keep all `licenses/usvfs/` and
`licenses/native-release/` files with the runtime. The retained notices and
source preserve upstream grants; this packaging change grants no new exception.

usvfs - User-Space Virtual File System, Copyright (C) Sebastian Herbord

Upstream repository: <https://github.com/ModOrganizer2/usvfs>.
Pinned Rust/native integration: <https://github.com/Reilley64/usvfs-rs>.
See `licenses/usvfs/LICENSE` for GPLv3-or-later and its conditional section 7
permissions, and the other retained files for third-party terms.
