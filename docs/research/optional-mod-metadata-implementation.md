# Optional Data Mod metadata implementation

## Scope and behavior

This direct user request has no associated issue. A Data Mod may omit its root `meta.toml`; absence means no Tombstones and does not make settings, snapshot/execution preparation, or conflict inspection invalid. Present metadata remains validated, including schema version and Tombstone rules. Existing provider-root metadata exclusion and nested ordinary content rules remain unchanged.

The changes only remove a required-presence check from settings and snapshot provider validation, and a missing-metadata problem from conflict scanning. Existing optional metadata reads, parsing, suppression, and execution consumed-byte tracking remain in place. Modlist matching, disabled mods, install metadata generation, security checks, performance policy, dependencies, public APIs, and build targets are unchanged. No native install or real Windows environment test is part of this work. The earlier mandatory-metadata statement in `issue-33-view-mapping-model.md` records its historical design state; this decision supersedes that one statement, not its root metadata visibility analysis.

## Validation

The new settings regression first failed on the base behavior with `EnvironmentInvalid` caused by a missing `meta.toml`; it passed after the change. Focused tests cover settings missing/present-invalid metadata, snapshot missing metadata through provider validation and snapshot load, and conflict scan missing/present-invalid metadata. Existing Tombstone tests remain the suppression regression. `bun run check` passed; `git diff --check` passed. Independent spec and standards reviews found no blockers. A combined execution fixture comparing absent metadata with present tombstones remains a nonblocking follow-up. Real Windows execution verification remains unavailable here.

## Style dispositions

The new code keeps file access in infrastructure and preserves existing security validation for present files. No new module, public interface, dependency, or unsafe code is introduced. Accepted findings: `src/infrastructure/environment/src/snapshot.rs` and `src/infrastructure/environment/src/conflict_scan.rs` / Application use cases and ports / Use-case parameters. Both are infrastructure helpers, not application use cases; cancellation remains last. These manual acceptances do not clear the automated gate or an enforce-mode block. No override was used. Test-only `expect` assertions on infrastructure reports are consistent with nearby tests; they do not discard errors across a production boundary.
