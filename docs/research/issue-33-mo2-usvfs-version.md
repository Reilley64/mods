# MO2 and mods usvfs provenance (#33)

Verified against official GitHub records on 2026-09-26. This is a read-only version comparison, not runtime debugging. The downloaded DLLs were not executed.

## Answer

**No: latest stable MO2 does not ship the same usvfs revision or DLL version as mods targets.** GitHub currently identifies **MO2 v2.5.2** (published 2024-08-04) as latest stable. Its release notes and accompanying commit manifest call its dependency **usvfs v0.5.0**. However, both DLLs inside its official portable archive report **0.5.6.1** in their file/product version resources. The source header at the v0.5.0 tag agrees with 0.5.6.1. The release label and DLL version must not be conflated. [1–5]

**Yes, current development defaults select the same upstream source revision at this snapshot**, because they track upstream `master`, which currently resolves to `57f1ea5e6ad13f7435a7af184748e6c1312c5637`. This is not a frozen MO2 submodule pin or proof that a particular development artifact contains that commit. **No, that does not make MO2's build the same as mods' custom Rust-shim/fork build.** [6–10]

| Identity | Official stable MO2 v2.5.2 | mods target |
| --- | --- | --- |
| Dependency/release label | `v0.5.0` | `usvfs-0.5.7.2-rs.1` (custom release) |
| Upstream source identity | v0.5.0 currently resolves to `9f7fd9660d51784aa2117cb45f2095e87312d558` | `57f1ea5e6ad13f7435a7af184748e6c1312c5637` |
| DLL/header version | shipped DLL file/product versions **0.5.6.1** | upstream header **0.5.7.2**; custom bundle not inspected here |
| Custom fork revision | no evidence of mods' fork in the official records | `eb4949fb2439fe5b98901e2fb1afceee752a6133` |

The stable commit manifest records a **tag**, not a full usvfs hash. The hash above is the current official tag resolution, supported by matching source and shipped resource version numbers; this is not independent cryptographic proof of the exact historical source used to compile the DLL. It is enough to establish that stable MO2's documented dependency is not our newer upstream pin. [2–5]

## Stable release asset inspection

Downloaded the official `Mod.Organizer-2.5.2.7z` (149,660,212 bytes) and extracted its `usvfs_x64.dll` and `usvfs_x86.dll`. Read the embedded version resource bytes and fixed version fields; both file and product versions are **0.5.6.1**. No downloaded program was loaded or executed. The installer asset was not inspected. [3]

SHA-256 values computed locally:

- Archive: `e6376efd87fd5ddd95aee959405e8f067afa526ea6c2c0c5aa03c5108bf4a815`
- `usvfs_x64.dll`: `e2b766f418575021b9d350f384195ce6f23173169b37222cdef3d7fe5495f8b5`
- `usvfs_x86.dll`: `c89d9587c7f725927f3ce03076e85af2dfd134909d6d769455be3d21e086a478`

These hashes identify the inspected bytes, not a reproducible-build attestation.

## Development is different from stable

The inspected MO2 master tree has no usvfs gitlink/submodule pin. Its workflow requests `usvfs` through `build-with-mob-action@master`. The action clones the official repository and selects the requested branch when available; for official master builds that is usvfs master. Separately, mob's default configuration selects `usvfs = master`; its usvfs task fetches that branch and builds x86 and x64 from source. These are moving branch policies, not stable-release provenance. Branch/owner overrides and build time can change what a particular development build uses. [6–9]

Snapshot identities:

- modorganizer master: `efe2a02d5dc641946baaa8db1440800f38d07837`
- mob master: `5602cba88e01b9220c680309618f9115c1b3a0ba`
- build-with-mob-action master: `a60a32d60aa535a67b2b21e2255c506f35c0bb07`
- usvfs master: `57f1ea5e6ad13f7435a7af184748e6c1312c5637`

The newest standalone upstream usvfs release is **v0.5.7.2**, published 2025-06-01. Its tag resolves to **`a50d84c64c9244f80dc67e9fe7af209bfe514d5b`**, not the master commit mods pins. Both carry the 0.5.7.2 version label. Therefore “MO2 uses 0.5.7.2” would still be insufficient to establish source or binary identity. This freshly confirms the distinction recorded in [issue-30-upstream-usvfs.md](issue-30-upstream-usvfs.md). [10–12]

## mods boundary and conclusion

The local authority is [`native/usvfs-release.json`](../../native/usvfs-release.json): repository `Reilley64/usvfs-rs`, custom tag `usvfs-0.5.7.2-rs.1`, upstream `sourceRevision` above, separate `forkRevision` above, and hashed native/source assets. The fork release must not be described as the stock MO2 DLL bundle merely because it shares an upstream base or version string.

- **Latest stable same DLL version? No:** inspected stable DLLs are 0.5.6.1, while our upstream header is 0.5.7.2.
- **Latest stable same documented upstream revision? No:** v0.5.0 resolves to a different commit.
- **Current master development default same upstream base? Yes, at this snapshot only.**
- **Any particular development artifact same revision? Unknown:** no development artifact was audited.
- **Same custom fork build/binary? Not established, and official records select upstream rather than our custom release.**

The older upstream dependency in stable MO2 is the relevant distinction; older MO2 release DLLs were not inventoried. No runtime behavior, integration correctness, build success, ABI correctness, or fault cause follows from this comparison. MO2 working successfully would not prove mods' configuration, Rust shim, packaging, or lifecycle is correct.

**Next step:** use this provenance distinction in the issue record. No pin change or runtime investigation is needed to answer this version question.

## Primary sources

1. Latest stable API and release notes: https://api.github.com/repos/ModOrganizer2/modorganizer/releases/latest and https://github.com/ModOrganizer2/modorganizer/releases/tag/v2.5.2
2. Release commit manifest: https://github.com/ModOrganizer2/modorganizer/releases/download/v2.5.2/Mod.Organizer-2.5.2-commits.txt
3. Inspected official portable asset: https://github.com/ModOrganizer2/modorganizer/releases/download/v2.5.2/Mod.Organizer-2.5.2.7z
4. Current v0.5.0 tag resolution: https://api.github.com/repos/ModOrganizer2/usvfs/commits/v0.5.0
5. Source version header and resource definition: https://github.com/ModOrganizer2/usvfs/blob/9f7fd9660d51784aa2117cb45f2095e87312d558/include/usvfs_version.h and https://github.com/ModOrganizer2/usvfs/blob/9f7fd9660d51784aa2117cb45f2095e87312d558/src/usvfs_dll/version.rc
6. MO2 workflow: https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/.github/workflows/build.yml
7. Build action and dependency checkout: https://github.com/ModOrganizer2/build-with-mob-action/blob/a60a32d60aa535a67b2b21e2255c506f35c0bb07/action.yml and https://github.com/ModOrganizer2/build-with-mob-action/blob/a60a32d60aa535a67b2b21e2255c506f35c0bb07/checkout-mo2-dependencies.ps1
8. mob configuration: https://github.com/ModOrganizer2/mob/blob/5602cba88e01b9220c680309618f9115c1b3a0ba/mob.ini
9. mob usvfs task: https://github.com/ModOrganizer2/mob/blob/5602cba88e01b9220c680309618f9115c1b3a0ba/src/tasks/usvfs.cpp
10. Upstream master API and pinned version header: https://api.github.com/repos/ModOrganizer2/usvfs/commits/master and https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs_version.h
11. Standalone upstream release: https://github.com/ModOrganizer2/usvfs/releases/tag/v0.5.7.2
12. Standalone tag resolution: https://api.github.com/repos/ModOrganizer2/usvfs/commits/v0.5.7.2
