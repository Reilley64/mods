# Streamline exec preparation: independent spec review

> Superseded note: the missing-archive-list refusal described here was later replaced by the approved embedded default fallback. See [streamline exec preparation](streamline-exec-preparation.md).

This is the independent pre-repair review. See [the repair response](streamline-exec-preparation.md#consolidated-review-repair) for the later changes and check results.


Base: `5eb0f192d2a7c5346d66280c4a46fe6c6a894b7b`. Reviewed the uncommitted tracked diff, new execution inventory, and decision/evidence documents. This was read-only: no repository changes, game launch, installation, or remote mutation. No new test run was needed for the source-level finding below. The reported 436 Rust / 119 tool passes are implementation evidence, not tests rerun by this reviewer.

## Verdict

One implementation blocker. The missing-both-archive-lists policy remains an independent user-policy blocker, not a previously approved behavior. Windows compilation and native/JIP behavior remain unverified.

## Blocker: whole-file INI reads dropped retained safe-open checks

**Location:** `src/infrastructure/environment/src/safe_fs.rs:156-158`; callers in `src/infrastructure/environment/src/derived_profile.rs:225-237`, `:65`, and `src/infrastructure/environment/src/execution_preparation.rs:305-310`.

The new `SafeDir::read` directly invokes `self.inner.read(...)`. The previous required-content path used `read_bounded`, which opened through `SafeDir::open_regular`. That existing method (`safe_fs.rs:206-219`) explicitly opens with `FollowSymlinks::No` and rejects reparse points, nonregular files, and `nlink() != 1`. The new method does none of these object checks.

This is not limited to the intentionally relaxed asset inventory. Postrun INI preservation now uses it to read both canonical and staged child INIs. `exists()` checks reject a stable symlink at those call sites, but do not check hard-link counts or tie the subsequent open to the checked object. A hard-linked child INI is now read and can be published rather than refused and retained as before. A hard-linked canonical INI likewise passes the read/comparison step. The required Fallout.ini preparation read does not even call `exists()` first, so an in-directory relative symlink can be followed directly there.

**Decision:** The discussion's “Decision: remove execution inventory link rejection and validation-only file opens” (`docs/exec-performance-discussion.md:143-149`) expressly limits the relaxation to exec provider inventory and excludes wholesale removal of safe filesystem checks from INI preservation. “Decision: JIP execution overrides, defer GECK, remove exec read size limits” (`:180-186`) removes byte caps and custom chunk reading, not safe-open checks. The frozen contract also retains post-run preservation.

**Repair:** Keep whole-file, uncapped reads but first acquire the existing checked regular-file handle, then use a standard/library whole-file read operation on that handle. Do not restore asset validation opens, size caps, or prelaunch change detection. Add a public prepare/derive/preserve regression for hard-linked INIs (including a child edit after derivation), checking failure and retained temporary state. Where supported, cover a linked required Fallout.ini too. This is restoration of an existing boundary, not a new threat model.

## Existing unresolved policy: neither profile supplies sArchiveList

`src/infrastructure/environment/src/derived_profile.rs:88-93` refuses launch with `profile_archive_list_missing` if both canonical files omit the list. The implementation report correctly calls this a temporary explicit refusal. The decision at `docs/exec-performance-discussion.md:186` explicitly leaves this baseline policy unresolved. Do not characterize the refusal as an approved final requirement or substitute an invalidation-only list/default-file fallback without the user's decision. This blocks completion independently of the code repair.

## Reviewed requirements that appear implemented

- **Command-scoped settings:** `main.rs:29-68` bypasses manifest loading for init, selects the load mode, calls `load_settings` once, and passes typed values into command-local dependencies. Config get/list receive records. Exec receives a binding rather than a settings adapter. Installation/conflict ports capture supplied bindings. No downstream Config resolution was found. Binding capture in infrastructure-only port factories is within the clarified scope.
- **Stored/effective settings:** `LoadedSettings` retains original bytes, manifest values, and resolved provenance. Preview uses supplied values, while store compares original source bytes before publication. Export's `verify_source` calls compare bytes rather than resolve settings again; strict capture/revalidation remains. Settings update still reports invalid effective installations through its typed outcome. Help/version return before resource construction.
- **Manifest IDs:** GameBinding and both manifest schemas drop stored Steam app/build identifiers. Both parsers reject old identifier fields through `deny_unknown_fields`; no migration or automatic environment rewrite was added. Steam discovery keeps its app identity. Obsolete build comparisons are removed without removing the unrelated strict installation filesystem checks.
- **Inventory:** Enabled names and priorities are built before traversal. Top-level names are recorded before disabled descendants are skipped. Exact full-set comparison and duplicate/case-collision checks remain. Streaming winner selection uses normalized keys and rank comparisons. Retained exact/subtree tombstone ranks handle providers visited out of priority order, and namespace types remain independent of suppression. Enabled-mod metadata is created with create-new and reread; disabled mods, Overwrite, and Data do not receive default metadata.
- **Exec simplification:** The launch DTO carries no asset length/mtime or consumed-byte freshness state. Native exec no longer validates the live Steam binding or performs a prelaunch preparation/equality pass. Launch preparation and its postrun use omit fixed entry allowlists and generated BSA comparison. Strict export retains the original preparation/snapshot path and bounded source reads. The relaxed asset walker has no validation-only asset open and retains depth/entry budgets.
- **Temporary INIs:** Execution generates a temporary Custom.ini even when canonical Custom.ini is absent. Existing Custom.ini wins over Fallout.ini for archive selection, including an explicit empty list. Managed archive/save settings share the domain transform. Canonically absent Custom.ini stays absent after preservation; existing unrelated edits can persist. No game-default fallback runs in the execution branch and GECK-specific expansion was not added. The safe-open regression above is the exception to preservation compliance.
- **Order/launch inputs:** Unlisted plugins use supplied collection order. Explicit plugin order remains. Windows lookup retains captured caller PATH/cwd and argument encoding, and removes retained validation handles.

## Windows and compatibility evidence limits

I traced changed native call signatures and DTO uses: `ExecutionAdapter::new`, supplied `GameBinding`, `prepare_launch`/`PreparedLaunch`, `VisibleProfileFile`, `derive_execution_inis`, and `check_launch_with_spool`. No concrete signature mismatch was found. This is not a Windows compile result. The macOS suite does not compile `execution_adapter/native.rs` or `launch_inputs/windows_inputs.rs`.

Native linked-provider recursion, Windows process-error behavior, and JIP timing for archive/save settings remain unverified. The evidence report explicitly says the cited JIP loader source does not establish that those consumers run after the assignments. Do not claim these guarantees from the passing host suite, and do not expand into GECK work or launch a game without authorization.

## Repair boundary

Collect this blocker with the independent standards review, perform the one permitted consolidated repair cycle, and rerun relevant preservation plus full checks. Keep the unresolved archive policy and platform evidence gaps visible. No recommendation here reinstates intentionally removed exec validation.
