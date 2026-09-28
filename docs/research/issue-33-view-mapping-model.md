# Virtual Game View mapping granularity (#33)

## Owner adoption — recursive Data overlays

After the policy waiver and isolated Rust prototype checks, the owner selected whole-provider recursive Data overlays for the issue #33 candidate. Preserve ordinary Mod Priority, enabled-only participation, Overwrite, independent Output Target selection, named Profile State, invalidation and save routing. Diagnostic outputs explicitly describe analytical projections rather than observed runtime visibility. Native source, pins and generated declarations are unchanged.

This supersedes earlier recommendations to retain per-file Data mapping or require selective-subtree filtering. The remaining sections preserve the decision history and original semantic comparison. Adoption does not establish a crash fix or successful runtime acceptance.

## Owner supersession — runtime metadata and Tombstone filtering

After reviewing the disposable mapping-plan prototype, the owner explicitly removed both runtime requirements: provider-root `meta.toml` may appear in the Virtual Game View, and managed execution need not apply existing Tombstone suppression. Installation and conflict analysis retain their metadata exclusion, canonical Tombstone interpretation and validation rules. Runtime visibility and the analytical projection are therefore not guaranteed to match for those entries.

This supersedes the metadata/Tombstone objections in the historical review below and resolves its question about mandatory runtime filtering. Whole-provider recursive Data overlays are now a viable design candidate for the retained ordinary priority/Output Target requirements; selective-subtree filtering is no longer required solely to satisfy those two waived behaviors.

Still unchanged: enabled-provider selection, Mod Priority, Overwrite precedence, independent Output Target selection, named Profile State mappings, invalidation mapping, save routing, native usvfs source and pin, and bindgen-generated raw declarations. Existing input validation is not waived. No implementation has changed, no native trial has run, and neither design adoption nor crash repair has been verified.

Next: define and review a Rust-only Data-overlay implementation scope, including how execution preparation uses its file projection. Keep installation/conflict projection behavior unchanged. Validate project-owned mapping plans before any claim of native behavior.

## Historical review conclusion (before the owner supersession)

Mapping every Data file individually is an implementation choice, not an explicit product requirement. However, replacing the current construction with unfiltered recursive mappings of whole provider roots would not preserve the current Virtual Game View rules. Root `meta.toml` would become game content, and lower-priority mod files suppressed by existing Tombstones could be imported again. Later winner links cannot remove entries for which there is no allowed winner.

Ordered recursive overlays can preserve ordinary Mod Priority, disabled-mod exclusion, Overwrite precedence and independent Output Target selection. The target must remain at its normal read rank. Those properties do not require per-file mapping. Existing structural validation must remain regardless of mapping granularity.

A selective-subtree hybrid is possible in principle, but only where its complete imported content is allowed under the same provider/Tombstone/metadata rules. Named Profile State files and the separately configured invalidation archive still require selective destinations. Saves already use a recursive target. Neither whole-root overlays nor file links guarantee an opaque physical namespace or the superseded copy-on-write/runtime Tombstone promises.

**Recommendation:** retain the existing mapping model for now. Do not silently replace it with whole-root recursion. A future selective-subtree design needs an explicit allowed-entry equivalence rule and project-owned configuration tests; it is not an established fix for the observed crash. No native code, binding, pin, product requirement or runtime state changed during this review.

One authority ambiguity remains: #30 says existing Tombstones "may still guide" winner/conflict computation. Preserve current runtime filtering unless the owner explicitly changes that behavior. Conflict/install Tombstone semantics and root metadata exclusion are not waived by accepting upstream physical fallback.

See [MO2 integration comparison](issue-33-mo2-integration.md) and [version provenance](issue-33-mo2-usvfs-version.md). The source-linked independent reviews below distinguish approved requirements from implementation choices.

## Contract review

## Decision

**Per-file native mapping for every Data file is not an approved product requirement.** It is the current translation choice. The retained requirements require selective namespace construction, not one particular usvfs call granularity. Pure unfiltered recursive overlays of entire provider roots are not equivalent: they import reserved root metadata and can reintroduce mod files suppressed by existing Tombstones. A hybrid can preserve ordinary overlay priority while retaining selected named-file mappings and exclusions, but its adequacy has not been established here. No mapping-model change is justified as a fault fix by this review.

Read-only review of the specified worktree and live GitHub issue bodies/comments through `gh`. No native execution, debugger, replay, code edit, or spec edit. This report is outside the repository.

## Authority

1. [#25 owner supersessions](https://github.com/Reilley64/mods/issues/25) expressly put revised [#30](https://github.com/Reilley64/mods/issues/30) and [#31](https://github.com/Reilley64/mods/issues/31) above conflicting historical handoffs. [ADR-0005](https://github.com/Reilley64/mods/blob/main/docs/adr/0005-use-upstream-usvfs.md) accepts physical mutation, no execution COW/durable Tombstones, non-opaque namespaces, and non-fail-closed injection.
2. [Historical #19 handoff](https://gist.github.com/Reilley64/76e35e2187eb5f0aff543c228a6de39f) delegates detailed retained semantics to owning decisions. [#12 priority/conflict decision](https://gist.github.com/Reilley64/a410c7fa1c2dc673a5fb28c3c7c63ba2) and current [#29](https://github.com/Reilley64/mods/issues/29) retain canonical Tombstone analysis. They do not restore superseded runtime mutation guarantees.
3. [CONTEXT.md](https://github.com/Reilley64/mods/blob/main/CONTEXT.md) names concepts; its general Tombstone absence definition must not override #30's explicit physical-base exception.
4. Local `docs/research/issue-30-upstream-usvfs.md` is historical evaluation with an approval addendum. Its opening “Not under the current acceptance contract” evaluates the **former** contract, not current rejection of upstream. Its “Prefer mapping already-resolved winners” is advice, not a mandate.
5. Local `docs/research/issue-31-execution-composition.md` and `docs/research/issue-33-mo2-integration.md` describe implementation evidence, not new product authority. The latter explicitly calls mapping-model replacement a scope decision.

## Requirement matrix

“Can” below means design capability, not runtime validation or proof of native correctness.

| Requirement and authority | Per-file projection | Recursive provider overlays | Hybrid / requirement consequence |
|---|---|---|---|
| Enabled mods low-to-high; disabled excluded; Overwrite highest (#30, #12) | Map resolved winners | Can express order by selected provider overlays | Neither requires per-file Data mapping. Rust still owns provider selection, case-insensitive policy and validation. |
| Output Target independent of read rank; default Overwrite; enabled explicit target (#30/#31, CONTEXT) | Separate creation target plus winners | Can mark the chosen provider at its normal order, not promote it by relinking last | MO2 actual launch uses this separation. Target choice is not evidence that recursion is incompatible. |
| Existing destinations remain in current provider; only new destinations use creation routing (#30) | Subject to upstream behavior | Same | Neither scheme supplies blanket output-only writes or COW. |
| Root provider `meta.toml` excluded; nested `subdir/meta.toml` ordinary (#12) | Selective links can exclude exactly the root file | Unfiltered whole-root recursion imports metadata | Requires selective construction or exact supported exclusion, not necessarily all-file mapping. Generic suffix skipping is not an acceptable substitute; bypass lists must be empty. |
| Existing file/subtree Tombstones suppress lower mod entries; higher files can override (#12/#29; #30 says existing metadata may guide winner computation) | Omit suppressed mod links | Naive recursion imports suppressed files again | Hybrid needs a selective boundary around affected entries/subtrees. No whiteout API is established. Pure whole-root overlays cannot claim exact retained projection. Whether #30's “may” makes all execution mod suppression mandatory deserves explicit clarification before weakening current behavior. Conflict/install semantics remain mandatory regardless. |
| Tombstone over physical Steam entry (#30 accepted limitation) | Omission does not hide physical destination | Overlay does not hide it either | Neither enforces absent physical base. Do not reject directory mapping on a guarantee per-file mapping also cannot provide. |
| Application deletion creates durable Tombstone / unchanged lower providers (superseded #9/#19) | Not supplied | Not supplied | Not current acceptance requirements. Preserve canonical metadata support; do not resurrect an execution mutation broker. |
| Structural collisions invalid before Tombstone filtering; disabled self-validation (#12) | Not guaranteed by links | Not guaranteed by overlays | Rust validation responsibility independent of granularity. A Tombstone cannot legalize file-versus-directory collision. |
| Physical base read fallback (#30 limitations) | Still occurs for omitted paths | Still occurs | Explicit identity links for each base winner are not stated as mandatory. Their omission would still require a reviewed configuration decision, not an assumed crash remedy. |
| Eight named Profile State files, five INIs to Documents and three state files to Local AppData (#31, #13) | Natural exact-path representation, including optional absent sources | Recursing the flat profile root into either user folder would import wrong-scope files and `saves` | Named selective mappings are required semantically. File links are the direct supported representation in the existing layout; this does not force per-file **Data** mapping. Hybrid naturally fits. |
| INI routing and generated invalidation BSA (#31/#13) | Named file links fit cache-backed BSA at reserved Data path | Provider-root recursion alone cannot express arbitrary cache-file relocation | A targeted file mapping is natural; no requirement says all other Data files must use that mechanism. INI recipe/version/path validation remains Rust-owned. |
| Full save subtree, no extension filtering/content inspection; `profile/saves` to fixed `__mods_saves` (#31, retained #21) | Enumerating existing files alone misses future/arbitrary descendants | Recursive dedicated save creation target fits | Existing implementation already hybrid: per-file Data/profile plus recursive saves. No-save-inspection contract favors subtree routing. |
| Hide unrelated physical INIs/saves, protect real `__mods_saves`, absent/deleted profile never reveals physical file (historical #13/#21) | Cannot guarantee | Cannot guarantee | Superseded by #30/#31 non-opaque namespace acceptance. Named configuration is still required; absolute physical exclusion is not. |
| Effective activation/order; no virtual timestamps (#31) | Neither mapping choice enforces engine timestamps | Same | Compute/configure order and report game-visible limitation. Do not infer guarantees from mapping shape. |
| Setup errors, 125/126/127, suspended root/Job ownership, streams, cancellation, Job drain (#30/#31) | Orthogonal | Orthogonal | Wrapper/lifecycle obligations remain; mapping BOOL failures must propagate. No scheme guarantees unreported descendant injection failures are detected. |
| Clear blacklist/suffix/directory skip lists; `.mohidden` ordinary (#30) | Required | Required | Copying MO2 skip policies is not permitted by choosing MO2-style overlays. |

## Benign namespace examples

- `Low/textures/paper.dds` and `High/textures/paper.dds`, with `Low` selected as Output Target: reads must still choose High. Both winner links and ordered overlays can express this. Selecting Low does not mean applying its read overlay last.
- Low has `meshes/chair.nif`; High owns a directory Tombstone `meshes`; Overwrite has `meshes/table.nif`. Canonical projection omits chair and includes table. Whole-root recursive overlays would include chair unless the construction handles suppression. A separate Steam `meshes/chair.nif` may still remain physically visible under accepted upstream limits, even when every mod link is filtered.
- Provider-root `meta.toml` is not game content; `docs/meta.toml` is. A blanket suffix exclusion is wrong. This is a selective-path requirement, not proof that every unrelated texture needs a native file call.
- The flat `profile` folder has both `Fallout.ini` and `plugins.txt`. They map to different user-directory namespaces. One recursive mapping of the whole profile folder is not equivalent. Saves are a separate complete-tree route.

## Exact technical evidence links

- [Pinned upstream public mapping/flag contract](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L40-L86): file links, recursive flag, creation target; containing destination directory must exist at least virtually. No documented requirement to map all Data files individually.
- [Pinned upstream mapping implementation](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/src/usvfs_dll/usvfs.cpp): basis of #31 absent-source/named-file/container configuration evidence, not opacity proof.
- [MO2 stable actual launch builder](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L2020-L2099), [development builder](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/organizercore.cpp#L2070-L2155), [connector flags](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L196-L233): ordered recursive directory overlays, selected custom creation target stays at ordinary priority. These are implementation precedent, not mods specification or runtime validation.
- [Profile #13](https://gist.github.com/Reilley64/4c7c5b7e551e8446cbab14018147ea0c), [save #21](https://gist.github.com/Reilley64/7dd26ed6a0aa8267bd43c078fa983849): named file inventory, archive invalidation and save configuration; hiding/timestamp/physical-protection portions explicitly superseded.
- [#33](https://github.com/Reilley64/mods/issues/33), [latest candidate checkpoint](https://github.com/Reilley64/mods/issues/33#issuecomment-5843718318), [disconnected rebuild waiver](https://github.com/Reilley64/mods/issues/33#issuecomment-5843638331): successful execution/save routing remains open; waiver does not change mapping semantics.

## Authority tensions and smallest decision

- #33 still says “modified-usvfs fork” and “fail-closed VFS behavior”; the handoff still says Steam unchanged, durable execution Tombstones, opaque saves and hook handshake. Those are resolved stale language, not competing current mandates: #25 explicitly elevates revised #30/#31.
- General CONTEXT Tombstone absence and #12 exact canonical resolution versus #30's physical-base visibility exception must be labeled as different views (canonical conflict projection versus upstream runtime namespace). Existing code cannot eliminate this accepted distinction simply by using file links.
- **Unresolved narrow question:** #30 says existing Tombstones “may still guide” winner/conflict computation, while it also retains priority projection and #12 read-time resolution. The safe reading is to preserve current selective mod filtering, not silently drop it. Ask whether execution must retain that filtering; do not use the ambiguity to remove conflict/install Tombstones.
- #12's reserved root metadata exclusion has no explicit supersession. An overlay redesign must account for it rather than assuming provider roots are all game content.

**Smallest scope decision:** record that per-file-all-Data is implementation choice, retain named Profile State and save routing, and preserve current selective provider metadata/Tombstone filtering until the narrow ambiguity is settled. Authorize only a design comparison of alternative projections against these retained rules if desired. Do not authorize a mapping rewrite, native change, downgrade, omitted mapping, runtime experiment, or claim of a crash fix from this review. No new backend or stronger isolation promise is needed.


## Feasibility review

## Decision

Whole-provider MO2-style recursive Data overlays are **not a drop-in replacement** for mods' resolved file links. They can express ordinary ordered file overlays and independent creation routing. They do not, from the public contract reviewed, express mods' provider-specific tombstone filtering or root-only metadata exclusion. Keep the current hybrid: resolved Data files, nonrecursive containers and Data creation target, named Profile State files, and a recursive save creation target. This is a semantic recommendation, not a crash fix or a claim that current native behavior is proved correct.

Read-only scope: local worktree `/Users/reilley/Repositories/mods/.worktrees/issue-33-distribution`, local reports and pinned public header. No execution, build, reproduction, tracing, path experiments, harness creation, native/pin changes, or repository edits.

## Authority and actual callers

- `CONTEXT.md`: enabled mods only; complete low-to-high priority; Overwrite highest; Output Target selection does not reorder providers or relocate existing destinations. Tombstones suppress lower files or inclusive subtrees.
- `docs/adr/0005-use-upstream-usvfs.md`: accepted physical mutation, no copy-on-write or durable execution Tombstones, non-opaque namespaces, and non-fail-closed descendant injection. Rust still owns Mod Priority, Output Target and Profile State.
- `docs/research/issue-31-execution-composition.md`: named optional source files need not be synthesized; virtual user-directory containers precede files; only saves are a recursive profile creation target; timestamp load order is not enforced.
- `docs/research/issue-33-mo2-integration.md`, Mapping callers and S1–S5/D1–D5: real launch uses `fileMapping(profileName, customOverwrite)`, not the alternate resolved-file helper. Enabled regular mods map in ascending priority; selected enabled custom target receives CREATETARGET at its normal rank; Overwrite maps last and is default creation target; connector makes directory entries recursive and preserves order.
- `docs/research/issue-33-mo2-usvfs-version.md`: stable MO2 v2.5.2 uses the v0.5.0 dependency (shipped DLL resources 0.5.6.1). Development defaults matched upstream `57f1ea5e6ad13f7435a7af184748e6c1312c5637` at the report snapshot, not a binary identity guarantee. mods fork is `eb4949fb2439fe5b98901e2fb1afceee752a6133`. MO2 success cannot validate this adapter or a redesign.

Pinned primary API: [usvfs.h](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L40-L86), directly read for this review. RECURSIVE means recursive linking. CREATETARGET redirects creation, including copy/move, replaces a prior destination target, and uses the innermost target. File destination parents must exist at least virtually. [Skip API, lines 168–194](https://github.com/ModOrganizer2/usvfs/blob/57f1ea5e6ad13f7435a7af184748e6c1312c5637/include/usvfs/usvfs.h#L168-L194) documents suffix matching and directory **names, not paths**. FAILIFSKIPPED reports skipping; it is not an exclusion predicate. No reviewed public mapping contract provides provider-ranked tombstones, exact root-only file exclusions, or an opaque directory operation. The initial “Virtual operations” comment is not an exported deletion API contract.

MO2 primary launch sources: [stable builder](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/organizercore.cpp#L2020-L2099), [development builder](https://github.com/ModOrganizer2/modorganizer/blob/efe2a02d5dc641946baaa8db1440800f38d07837/src/organizercore.cpp#L2070-L2155), [connector](https://github.com/ModOrganizer2/modorganizer/blob/v2.5.2/src/usvfsconnector.cpp#L196-L233). These source-linked findings are recorded in the local integration report; their order policy is not proof of all upstream collision behavior.

## Concrete semantic cases

1. **Normal priority / disabled exclusion.** Low and High both contain `textures/a.dds`; disabled D contains `textures/b.dds`; Overwrite contains `textures/a.dds`. Recursive overlays can follow Low, High, Overwrite and omit D. Overwrite should win `a`, D must not contribute `b`. Mapping every installed directory would violate participation. Rust resolution/validation remains necessary even if links become coarser.
2. **Low custom Output Target.** Low is selected, High contains `a`, Overwrite may also contain `a`. Putting a recursive Low mapping last to obtain creation routing reintroduces Low's read content at the wrong rank. MO2 avoids that by marking Low in its normal position and leaving Overwrite non-creation-target. Current mods avoids it by applying a nonrecursive target before explicit winners. In either design, selecting Low must not move an existing `a` there. New `new.dds` goes to Low; later resolution still gives Low its ordinary rank. Read overlays and creation routing are separate decisions.
3. **Non-base tombstone, no physical fallback.** Low has `textures/a.dds`; High's metadata tombstones that path; Steam has no such file. Current winner filtering supplies no Low link. Whole-root Low recursion supplies it, and High's lack of a replacement file does not remove it. Linking explicit winners *after* recursion cannot repair this negative case: there is no winner to link. For an inclusive subtree tombstone, High can suppress Low's `textures/old.dds`, while a still-higher provider restores only `textures/new.dds`; a blanket skip of `textures` would wrongly suppress the restoration too.
4. **Physical fallback is different.** Add Steam `textures/a.dds` to case 3. Omitting all virtual links still does not hide that physical file. This is an accepted non-opaque-namespace limitation, explicitly noted in `configuration.rs`. It does not excuse recursively importing Low in case 3: that is an avoidable caller-policy regression, not physical fallback. Neither construction guarantees tombstone absence over physical Steam entries.
5. **Metadata exposure.** Every installed Data Mod requires root `meta.toml`; Overwrite may have it. `snapshot.rs:881` excludes root metadata from ordinary entries. Whole-root recursion would import it into `Data/meta.toml`. Skipping suffix `meta.toml` is broader than root-only exclusion: legitimate `textures/meta.toml` or `othermeta.toml` can match too. Skipping all `.toml` is broader still. No documented flag makes a root-only exclusion. Do not assume path/glob syntax or per-provider masking from the skip API.
6. **File/directory collision.** Low contains file `meshes/item`; High contains `meshes/item/part.nif` (or the reverse type arrangement). Current execution namespace validation rejects file/directory disagreement rather than selecting an upstream-dependent winner. Recursive overlays cannot replace that validation with “last provider wins.” This is an invalid-input counterexample, not a valid namespace that mods promises to run. `snapshot.rs:649–809` applies participating providers and rejects differing entry kinds at a key; conflict diagnostics separately report these collisions (`conflicts/projection.rs:681–707`). Keep rejection in every option.

Provider semantics are explicit in `src/domain/src/providers.rs` and `provider_resolution.rs`: Base < Regular(priority) < Overwrite; controlling tombstones are rank-based, exact or inclusive subtree; higher files survive. `src/infrastructure/environment/src/snapshot.rs:318–389,504–563,649–809,881` validates metadata, resolves providers, rejects collisions and excludes root metadata. `execution_preparation.rs:83–120,207–233` passes enabled state and only EffectiveResult::File winners while tracking metadata for revalidation. None of these responsibilities disappears with directory links.

## Options

| Construction | Feasibility / boundary |
|---|---|
| Resolved per-file Data | Preserves selected positive winners and avoids importing suppressed non-base files/root metadata. Requires parent containers. Does not close physical namespaces or enforce copy-on-write. Current supported baseline. |
| Whole provider recursive Data | Reasonable for a reduced model of ordinary clean overlays. Requires enabled filtering, ascending rank, Overwrite last, correct independent target marking and existing validation. Not equivalent for current tombstones/metadata. “Overlay then patch winners” still leaves unwanted negative entries. |
| Selective Data hybrid | Potentially feasible only for validated subtrees whose full recursively imported content is allowed, with no suppressed entries, excluded metadata, structural conflicts, or unresolved priority/case-policy differences. Keep everything else per-file; never recursively map an ancestor of an excluded path. Must reason about interactions across *all* contributing providers, not just whether the selected subtree owner has tombstones. This needs an explicit design and caller-level equivalence tests; it is not presently established or needed. |
| Existing profile/save hybrid | Already matches differing namespace purposes. Eight named files (five Documents INIs; three Local App Data state files) plus virtual containers; recursive `profile/saves` to Documents `__mods_saves`. Do not recursively map the entire profile to either destination: that exposes unrelated `modlist.txt`, duplicates state into the wrong directory, and misroutes arbitrary writes. |

Profile mapping authority: `src/infrastructure/execution/src/profile.rs:339–386`; Data apply order: `configuration.rs:54–156`. Preserve the separately configured invalidation archive as well; it is not ordinary provider-root content.

## Accepted limits and minimal next step

Neither scheme enforces immutable physical providers, copy-on-write, durable tombstones for runtime deletes, a closed namespace, timestamp-based game load order, or fail-closed injection into every descendant. Directory recursion is not a sandbox or a repair for these accepted limits. Static review also does not establish runtime equivalence for Unicode case keys, mapping collisions, optional absent sources, or source changes after configuration.

Recommendation: retain the existing Data/profile/save hybrid. If a redesign is still desired, first approve its scope and require a static caller-policy specification proving the imported set equals the allowed set (especially negative tombstone and metadata cases), plus focused project-owned configuration tests. Determine any selective-subtree eligibility rule before implementation. No native change, pin change, broad new harness, or fault investigation follows from this report.
