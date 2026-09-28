# WinGet runtime r2 installation fix

Scope: repair the ZIP path layout; preserve all 23 runtime file bytes and complete source; retain all old public assets/tag and failed-test evidence. Publish additive runtime-r2 ZIP/checksums. Verify default WinGet install, registered mods help/version, owned uninstall and restored settings on reill; update existing PR installer URL/hash and its comment/checklist truthfully, then verify upstream installation validation. No game tests, binary rebuild, extractor override/security bypass, unrelated Rust cleanup or automated updates.

Expected code changes: scripts/repack-v010-runtime.ts and tests/runtime-repack.test.ts, plus this preparation note. Scratch manifests/controller change only artifact pins, fresh paths and ZIP length. Existing controller logic and its 32 passing synthetic tests remain unchanged.

Diagnosis: matching WinGet Shell operation on the existing host enumerated zero original ZIP entries and returned 0x8000FFFF; the normalized archive queued six top-level items and extracted 23 files/four directories with matching payload hashes. Root ./ and ./ prefixes removed; all retained compressed bytes and metadata unchanged (entry order/offsets differ). Evidence: /tmp/mods33-zip-path-probe-01/{report,comparison,normalized-inventory}.json. This identifies a path-layout compatibility fix; no claim of a pristine Windows host.

Candidate: mods-v0.1.0-runtime-r2-x86_64-pc-windows-msvc.zip, 5026969 bytes, SHA256 85d5066669c2f8ea2f2995629a7588a4ac1fd293f26c689beac8b33d5215572f. This is the exact diagnostic ZIP renamed; no recompression after the Windows extraction test. New verifier checks it against the pinned original ZIP and unchanged complete source sidecar.
