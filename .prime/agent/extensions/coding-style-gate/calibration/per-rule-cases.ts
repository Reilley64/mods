/**
 * Independent target-label fixtures. Other rules may also match a case.
 * These are review snippets, not standalone Rust compilation units.
 * Empty before/after denotes file addition/deletion in the calibration adapter.
 * Evidence limits and exception rationale: ./per-rule-fixture-notes.md.
 */
export interface PerRuleCase {
    name: string;
    ruleId: string;
    split: "train" | "validation";
    expectedViolation: boolean;
    path?: string;
    referencingFiles?: string[];
    before: string;
    after: string;
}

export const perRuleCases: PerRuleCase[] = [
  {
    "name": "fold-validation-into-publication-violation",
    "ruleId": "comments-and-documentation-readability-before-secondary-cleanup",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn publish_profiles(profiles: Vec<Profile>) -> Result<()> {\n    for profile in profiles {\n        if profile.enabled {\n            profile.validate()?;\n            publish(profile)?;\n        }\n    }\n    Ok(())\n}",
    "after": "fn publish_profiles(profiles: Vec<Profile>) -> Result<()> {\n    profiles.into_iter().try_fold((), |_, p| p.enabled.then(|| p.validate().and_then(|_| publish(p))).transpose().map(|_| ()))\n}"
  },
  {
    "name": "fold-validation-into-publication-compliant",
    "ruleId": "comments-and-documentation-readability-before-secondary-cleanup",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn publish_profiles(profiles: Vec<Profile>) -> Result<()> {\n    for profile in profiles {\n        if profile.enabled {\n            profile.validate()?;\n            publish(profile)?;\n        }\n    }\n    Ok(())\n}",
    "after": "fn publish_profiles(profiles: Vec<Profile>) -> Result<()> {\n    for profile in profiles {\n        if !profile.enabled {\n            continue;\n        }\n        profile.validate()?;\n\n        publish(profile)?;\n    }\n    Ok(())\n}"
  },
  {
    "name": "deduplicate-state-decisions-violation",
    "ruleId": "comments-and-documentation-readability-before-secondary-cleanup",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn choose_action(exists: bool, stale: bool) -> Action {\n    if !exists { return Action::Create; }\n    if stale { return Action::Replace; }\n    Action::Keep\n}",
    "after": "fn choose_action(exists: bool, stale: bool) -> Action {\n    [Action::Create, Action::Create, Action::Keep, Action::Replace][((exists as usize) << 1) | stale as usize]\n}"
  },
  {
    "name": "deduplicate-state-decisions-compliant",
    "ruleId": "comments-and-documentation-readability-before-secondary-cleanup",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn choose_action(exists: bool, stale: bool) -> Action {\n    if !exists { return Action::Create; }\n    if stale { return Action::Replace; }\n    Action::Keep\n}",
    "after": "fn choose_action(exists: bool, stale: bool) -> Action {\n    match (exists, stale) {\n        (false, _) => Action::Create,\n        (true, false) => Action::Keep,\n        (true, true) => Action::Replace,\n    }\n}"
  },
  {
    "name": "compress-recovery-violation",
    "ruleId": "comments-and-documentation-readability-before-secondary-cleanup",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn load_index() -> Result<Index> {\n    match read_index() {\n        Ok(index) => Ok(index),\n        Err(error) if error.is_missing() => rebuild_index(),\n        Err(error) => Err(error),\n    }\n}",
    "after": "fn load_index() -> Result<Index> {\n    read_index().or_else(|e| e.is_missing().then(|| rebuild_index()).unwrap_or(Err(e)))\n}"
  },
  {
    "name": "compress-recovery-compliant",
    "ruleId": "comments-and-documentation-readability-before-secondary-cleanup",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn load_index() -> Result<Index> {\n    match read_index() {\n        Ok(index) => Ok(index),\n        Err(error) if error.is_missing() => { return rebuild_index(); }\n        Err(error) => { return Err(error); }\n    }\n}",
    "after": "fn load_index() -> Result<Index> {\n    match read_index() {\n        Ok(index) => Ok(index),\n        Err(error) if error.is_missing() => rebuild_index(),\n        Err(error) => Err(error),\n    }\n}"
  },
  {
    "name": "decode-cryptic-profile-flags-violation",
    "ruleId": "comments-and-documentation-self-explanatory-code",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn select(p: &Profile) {\n    // Only process profiles that are active and not frozen.\n    if p.a && !p.f { process(p); }\n}"
  },
  {
    "name": "decode-cryptic-profile-flags-compliant",
    "ruleId": "comments-and-documentation-self-explanatory-code",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn select(profile: &Profile) {\n    if profile.is_active && !profile.is_frozen {\n        process(profile);\n    }\n}"
  },
  {
    "name": "explain-opaque-retry-number-violation",
    "ruleId": "comments-and-documentation-self-explanatory-code",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn retry(n: u32, v: &mut Vec<Job>, j: Job) {\n    // Queue the job while attempts remain.\n    if n < 4 { v.push(j); }\n}"
  },
  {
    "name": "explain-opaque-retry-number-compliant",
    "ruleId": "comments-and-documentation-self-explanatory-code",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn retry(attempts: u32, pending: &mut Vec<Job>, job: Job) {\n    // The registry rejects a fifth attempt within its retry window.\n    const MAX_ATTEMPTS: u32 = 4;\n    if attempts < MAX_ATTEMPTS {\n        pending.push(job);\n    }\n}"
  },
  {
    "name": "narrate-tangled-refresh-violation",
    "ruleId": "comments-and-documentation-self-explanatory-code",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn refresh(x: bool, y: bool) {\n    // A disconnected source needs recovery, but a connected stale one needs a reload.\n    if x { if y { reload(); } } else { recover(); }\n}"
  },
  {
    "name": "narrate-tangled-refresh-compliant",
    "ruleId": "comments-and-documentation-self-explanatory-code",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn refresh(connected: bool, stale: bool) {\n    if !connected {\n        recover();\n        return;\n    }\n\n    if stale {\n        reload();\n    }\n}"
  },
  {
    "name": "narrated-clear-violation",
    "ruleId": "comments-and-documentation-reason-comments",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn invalidate(cache: &mut Cache) {\n    // Clear the cache.\n    cache.clear();\n}"
  },
  {
    "name": "narrated-clear-compliant",
    "ruleId": "comments-and-documentation-reason-comments",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn invalidate(cache: &mut Cache) {\n    // Cached handles reference the old generation and cannot survive a switch.\n    cache.clear();\n}"
  },
  {
    "name": "rustdoc-obvious-count-violation",
    "ruleId": "comments-and-documentation-reason-comments",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// Returns the entry count.\npub fn entry_count(&self) -> usize {\n    self.entries.len()\n}"
  },
  {
    "name": "rustdoc-obvious-count-compliant",
    "ruleId": "comments-and-documentation-reason-comments",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// Returns the entry count, including entries pending durable publication.\n///\n/// The count must not be used to infer how many entries survived a crash.\npub fn entry_count(&self) -> usize {\n    self.entries.len()\n}"
  },
  {
    "name": "doc-struct-fields-violation",
    "ruleId": "comments-and-documentation-reason-comments",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// A lease with a token and expiry.\npub struct Lease {\n    token: Token,\n    expires_at: Instant,\n}"
  },
  {
    "name": "doc-struct-fields-compliant",
    "ruleId": "comments-and-documentation-reason-comments",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// Grants exclusive publication rights until expiry.\n///\n/// Dropping a lease does not revoke work already submitted to the publisher.\npub struct Lease {\n    token: Token,\n    expires_at: Instant,\n}"
  },
  {
    "name": "ordinary-api-contract-violation",
    "ruleId": "comments-and-documentation-rustdoc-format",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "// Acquires exclusive publication access. Returns Busy when another writer owns it.\npub fn acquire_writer() -> Result<Writer, AcquireWriterError> { acquire() }"
  },
  {
    "name": "ordinary-api-contract-compliant",
    "ruleId": "comments-and-documentation-rustdoc-format",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// Acquires exclusive publication access.\n///\n/// # Errors\n///\n/// Returns `Busy` when another writer owns the publication lease.\npub fn acquire_writer() -> Result<Writer, AcquireWriterError> { acquire() }"
  },
  {
    "name": "module-doc-form-violation",
    "ruleId": "comments-and-documentation-rustdoc-format",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/filesystem/src/recovery.rs",
    "before": "",
    "after": "// Recovery policy for this module: interrupted copies remain available to resume.\n// All paths accepted by this module are relative to the selected profile.\n\nuse super::ProfilePath;\n\nfn resume_copy(path: &ProfilePath) { wait_for_rename(path); }"
  },
  {
    "name": "module-doc-form-compliant",
    "ruleId": "comments-and-documentation-rustdoc-format",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/filesystem/src/recovery.rs",
    "before": "",
    "after": "//! Recovery keeps interrupted copies available for the next invocation.\n\nfn resume_copy(path: &ProfilePath) {\n    // The platform may retain the handle until the pending rename completes.\n    wait_for_rename(path);\n}"
  },
  {
    "name": "unstructured-panic-contract-violation",
    "ruleId": "comments-and-documentation-rustdoc-format",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// Borrows the active profile. Panics if the session has already ended.\npub fn active_profile(&self) -> &Profile { self.profile.as_ref().expect(\"session ended\") }"
  },
  {
    "name": "unstructured-panic-contract-compliant",
    "ruleId": "comments-and-documentation-rustdoc-format",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// Borrows the active profile.\n///\n/// # Panics\n///\n/// Panics if the session has already ended.\npub fn active_profile(&self) -> &Profile { self.profile.as_ref().expect(\"session ended\") }"
  },
  {
    "name": "local-tree-import-violation",
    "ruleId": "formatting-and-imports-import-placement-and-use",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn sorted_ids(ids: Vec<Id>) -> Vec<Id> {\n    use std::collections::BTreeSet;\n    ids.into_iter().collect::<BTreeSet<_>>().into_iter().collect()\n}"
  },
  {
    "name": "local-tree-import-compliant",
    "ruleId": "formatting-and-imports-import-placement-and-use",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "use std::collections::BTreeSet;\n\nfn sorted_ids(ids: Vec<Id>) -> Vec<Id> {\n    ids.into_iter().collect::<BTreeSet<_>>().into_iter().collect()\n}"
  },
  {
    "name": "qualified-call-sites-violation",
    "ruleId": "formatting-and-imports-import-placement-and-use",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/catalog/src/load.rs",
    "before": "",
    "after": "fn load_pair() -> io::Result<(String, String)> {\n    let header = std::fs::read_to_string(\"header\")?;\n    let body = std::fs::read_to_string(\"body\")?;\n    Ok((header, body))\n}"
  },
  {
    "name": "qualified-call-sites-compliant",
    "ruleId": "formatting-and-imports-import-placement-and-use",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/catalog/src/load.rs",
    "before": "",
    "after": "use crate::{archive, profile};\n\nfn decode_pair(bytes: &[u8]) -> Result<(Archive, Profile)> {\n    let archive = archive::decode(bytes)?;\n    let profile = profile::decode(bytes)?;\n    Ok((archive, profile))\n}"
  },
  {
    "name": "conditional-block-import-violation",
    "ruleId": "formatting-and-imports-import-placement-and-use",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn platform_name() -> &'static str {\n    #[cfg(windows)]\n    use crate::windows::NAME;\n    #[cfg(not(windows))]\n    use crate::portable::NAME;\n    NAME\n}"
  },
  {
    "name": "conditional-block-import-compliant",
    "ruleId": "formatting-and-imports-import-placement-and-use",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "#[cfg(windows)]\nuse crate::windows::NAME;\n#[cfg(not(windows))]\nuse crate::portable::NAME;\n\nfn platform_name() -> &'static str { NAME }"
  },
  {
    "name": "load-transform-save-violation",
    "ruleId": "formatting-and-imports-phase-spacing",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn compact_index(id: Id) -> Result<Summary> {\n    require_writable(id)?;\n    let entries = load_entries(id)?;\n    let compacted = deduplicate(entries);\n    save_entries(id, &compacted)?;\n    Ok(Summary::from(compacted))\n}"
  },
  {
    "name": "load-transform-save-compliant",
    "ruleId": "formatting-and-imports-phase-spacing",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn compact_index(id: Id) -> Result<Summary> {\n    require_writable(id)?;\n\n    let entries = load_entries(id)?;\n\n    let compacted = deduplicate(entries);\n\n    save_entries(id, &compacted)?;\n\n    Ok(Summary::from(compacted))\n}"
  },
  {
    "name": "split-single-buffer-operation-violation",
    "ruleId": "formatting-and-imports-phase-spacing",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn encoded_header(magic: &[u8], version: u8) -> Vec<u8> {\n    let mut header = Vec::new();\n\n    header.extend_from_slice(magic);\n\n    header.push(version);\n\n    header\n}"
  },
  {
    "name": "split-single-buffer-operation-compliant",
    "ruleId": "formatting-and-imports-phase-spacing",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn encoded_header(magic: &[u8], version: u8) -> Vec<u8> {\n    let mut header = Vec::new();\n    header.extend_from_slice(magic);\n    header.push(version);\n\n    header\n}"
  },
  {
    "name": "validate-acquire-publish-violation",
    "ruleId": "formatting-and-imports-phase-spacing",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "async fn activate(id: Id) -> Result<Active> {\n    if id.is_reserved() { return Err(Reserved.into_report()); }\n    let lease = acquire_lease(id).await?;\n    publish_active(&lease).await?;\n    Ok(Active::new(id, lease))\n}"
  },
  {
    "name": "validate-acquire-publish-compliant",
    "ruleId": "formatting-and-imports-phase-spacing",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "async fn activate(id: Id) -> Result<Active> {\n    if id.is_reserved() { return Err(Reserved.into_report()); }\n\n    let lease = acquire_lease(id).await?;\n\n    publish_active(&lease).await?;\n\n    Ok(Active::new(id, lease))\n}"
  },
  {
    "name": "domain-transport-type-violation",
    "ruleId": "architecture-and-modules-dependency-direction-and-composition-roots",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/registry/snapshot.rs",
    "before": "",
    "after": "use reqwest::Response;\n\npub struct RegistrySnapshot {\n    pub response: Response,\n}"
  },
  {
    "name": "domain-transport-type-compliant",
    "ruleId": "architecture-and-modules-dependency-direction-and-composition-roots",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/registry/snapshot.rs",
    "before": "",
    "after": "pub struct RegistrySnapshot {\n    pub revision: Revision,\n    pub entries: Vec<RegistryEntry>,\n}"
  },
  {
    "name": "application-filesystem-handle-violation",
    "ruleId": "architecture-and-modules-dependency-direction-and-composition-roots",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/manifest/read_manifest.rs",
    "before": "",
    "after": "use std::fs::File;\n\npub struct ReadManifestDependencies {\n    pub manifest: File,\n}"
  },
  {
    "name": "infrastructure-filesystem-handle-exception",
    "ruleId": "architecture-and-modules-dependency-direction-and-composition-roots",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/manifest/src/reader.rs",
    "before": "",
    "after": "use std::fs::File;\n\npub struct ManifestReader {\n    manifest: File,\n}"
  },
  {
    "name": "application-provider-error-violation",
    "ruleId": "architecture-and-modules-dependency-direction-and-composition-roots",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/registry/refresh_registry.rs",
    "before": "",
    "after": "use reqwest::Error;\n\npub async fn refresh_registry() -> Result<Registry, Error> {\n    fetch_registry().await\n}"
  },
  {
    "name": "application-provider-error-compliant",
    "ruleId": "architecture-and-modules-dependency-direction-and-composition-roots",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/registry/refresh_registry.rs",
    "before": "",
    "after": "pub async fn refresh_registry(dependencies: RefreshRegistryDependencies) -> rootcause::Result<Registry, RefreshRegistryError> {\n    dependencies.fetch_registry.call(()).await.context(RefreshRegistryError)\n}"
  },
  {
    "name": "export-leaf-parser-violation",
    "ruleId": "architecture-and-modules-capability-modules-and-public-apis",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/manifest/mod.rs",
    "before": "",
    "after": "pub mod manifest_parser;\npub mod token_cursor;"
  },
  {
    "name": "export-leaf-parser-compliant",
    "ruleId": "architecture-and-modules-capability-modules-and-public-apis",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/manifest/mod.rs",
    "before": "",
    "after": "mod manifest_parser;\nmod token_cursor;\n\npub use manifest_parser::{Manifest, parse_manifest};"
  },
  {
    "name": "dumping-ground-module-violation",
    "ruleId": "architecture-and-modules-capability-modules-and-public-apis",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/lib.rs",
    "before": "",
    "after": "pub mod misc;\npub mod helpers;\npub use helpers::*;"
  },
  {
    "name": "dumping-ground-module-compliant",
    "ruleId": "architecture-and-modules-capability-modules-and-public-apis",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/lib.rs",
    "before": "",
    "after": "pub mod catalog;\npub mod profiles;\n\npub use catalog::Catalog;\npub use profiles::Profile;"
  },
  {
    "name": "expose-parser-internals-violation",
    "ruleId": "architecture-and-modules-capability-modules-and-public-apis",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/domain/src/manifest/token_cursor.rs",
    "before": "",
    "after": "pub struct TokenCursor {\n    pub offset: usize,\n    pub pending_tokens: Vec<Token>,\n}\n\npub fn advance_cursor(cursor: &mut TokenCursor) { cursor.offset += 1; }"
  },
  {
    "name": "expose-parser-internals-compliant",
    "ruleId": "architecture-and-modules-capability-modules-and-public-apis",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/domain/src/manifest/token_cursor.rs",
    "before": "",
    "after": "struct TokenCursor {\n    offset: usize,\n    pending_tokens: Vec<Token>,\n}\n\nfn advance_cursor(cursor: &mut TokenCursor) { cursor.offset += 1; }"
  },
  {
    "name": "handwritten-toml-table-violation",
    "ruleId": "dependencies-prefer-established-crates",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/registry/src/auth.rs",
    "before": "",
    "after": "use toml::Value;\n\nfn parse_settings(text: &str) -> Value {\n    let mut table = toml::map::Map::new();\n    for line in text.lines() {\n        let Some((key, value)) = line.split_once('=') else { continue; };\n        table.insert(key.trim().into(), Value::String(value.trim().trim_matches('\"').into()));\n    }\n    Value::Table(table)\n}"
  },
  {
    "name": "handwritten-toml-table-compliant",
    "ruleId": "dependencies-prefer-established-crates",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/registry/src/auth.rs",
    "before": "",
    "after": "use toml::{Value, from_str};\n\nfn parse_settings(text: &str) -> rootcause::Result<Value, ParseSettingsError> {\n    from_str(text).context(ParseSettingsError)\n}"
  },
  {
    "name": "handwritten-text-decoder-violation",
    "ruleId": "dependencies-prefer-established-crates",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/registry/src/encoding.rs",
    "before": "",
    "after": "use encoding_rs::UTF_16LE;\n\nfn decode_provider_text(bytes: &[u8]) -> String {\n    let units = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]));\n    let mut output = String::new();\n    let mut pending_high = None;\n    for unit in units {\n        if (0xD800..=0xDBFF).contains(&unit) { pending_high = Some(unit); continue; }\n        let scalar = if let Some(high) = pending_high.take() {\n            0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00)\n        } else { u32::from(unit) };\n        output.push(char::from_u32(scalar).unwrap_or(char::REPLACEMENT_CHARACTER));\n    }\n    output\n}"
  },
  {
    "name": "handwritten-text-decoder-compliant",
    "ruleId": "dependencies-prefer-established-crates",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/registry/src/encoding.rs",
    "before": "",
    "after": "// The legacy manifest doubles semicolons, not URL percent escapes. General\n// URL decoders would change literal percent signs; change only this delimiter.\nfn decode_manifest_label(value: &str) -> String {\n    value.replace(\";;\", \";\")\n}"
  },
  {
    "name": "handwritten-asynchronous-delay-violation",
    "ruleId": "dependencies-prefer-established-crates",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/registry/src/digest.rs",
    "before": "",
    "after": "use tokio::task::yield_now;\nuse std::time::{Duration, Instant};\n\nasync fn retry_delay(duration: Duration) {\n    let deadline = Instant::now() + duration;\n    while Instant::now() < deadline {\n        yield_now().await;\n    }\n}"
  },
  {
    "name": "handwritten-asynchronous-delay-compliant",
    "ruleId": "dependencies-prefer-established-crates",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/registry/src/digest.rs",
    "before": "",
    "after": "use tokio::time::sleep;\nuse std::time::Duration;\n\nasync fn retry_delay(duration: Duration) {\n    sleep(duration).await;\n}"
  },
  {
    "name": "unbounded-compatibility-framework-violation",
    "ruleId": "dependencies-narrow-custom-implementations",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/index/src/legacy_label.rs",
    "before": "",
    "after": "// The old index doubles colons in labels.\nstruct LegacyCompatibility {\n    scheduler: Scheduler,\n    http_pool: HttpPool,\n    cache: Cache,\n    label_decoder: LabelDecoder,\n}\n\nimpl LegacyCompatibility {\n    fn decode_label(&self, label: &str) -> String { label.replace(\"::\", \":\") }\n    fn start_workers(&self) { self.scheduler.start(); }\n    fn fetch(&self, url: Url) { self.http_pool.fetch(url); }\n}"
  },
  {
    "name": "unbounded-compatibility-framework-compliant",
    "ruleId": "dependencies-narrow-custom-implementations",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/index/src/legacy_label.rs",
    "before": "",
    "after": "// The legacy index uses doubled colons, not percent escapes; URL decoding changes\n// valid labels. This adapter changes only escaped colons and preserves other bytes.\nfn decode_legacy_label(label: &str) -> String {\n    label.replace(\"::\", \":\")\n}"
  },
  {
    "name": "missing-custom-decoder-gap-violation",
    "ruleId": "dependencies-narrow-custom-implementations",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/registry/src/wire_name.rs",
    "before": "",
    "after": "fn decode_wire_name(name: &str) -> String {\n    name.replace(\"~s\", \"/\").replace(\"~~\", \"~\")\n}"
  },
  {
    "name": "missing-custom-decoder-gap-compliant",
    "ruleId": "dependencies-narrow-custom-implementations",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/registry/src/wire_name.rs",
    "before": "",
    "after": "// Registry v1 uses ~s for slash and ~~ for tilde; standard percent decoding does\n// not implement this grammar. Consume escapes once so decoded tildes stay literal.\nfn decode_wire_name(name: &str) -> Result<String, InvalidEscape> {\n    let mut output = String::new();\n    let mut chars = name.chars();\n    while let Some(ch) = chars.next() {\n        if ch != '~' { output.push(ch); continue; }\n        match chars.next() {\n            Some('s') => output.push('/'),\n            Some('~') => output.push('~'),\n            _ => return Err(InvalidEscape),\n        }\n    }\n    Ok(output)\n}"
  },
  {
    "name": "custom-retry-suite-violation",
    "ruleId": "dependencies-narrow-custom-implementations",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/registry/src/retry.rs",
    "before": "",
    "after": "struct RegistryExecutor {\n    timers: Vec<Timer>,\n    dns_cache: DnsCache,\n    connection_pool: ConnectionPool,\n    retry_queue: Vec<Request>,\n}\n\nimpl RegistryExecutor {\n    fn run(&mut self) {\n        self.timers.sort_by_key(|timer| timer.deadline);\n        self.dns_cache.refresh();\n        self.connection_pool.flush();\n    }\n}"
  },
  {
    "name": "custom-retry-suite-compliant",
    "ruleId": "dependencies-narrow-custom-implementations",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/registry/src/retry.rs",
    "before": "",
    "after": "// The registry's Retry-After value counts catalog revisions rather than seconds.\n// Keep the HTTP client's pooling and DNS; translate only this provider extension.\nfn retry_revision(header: &str) -> Result<Revision, InvalidRevision> {\n    let digits = header.strip_prefix(\"revision=\").ok_or(InvalidRevision)?;\n    let revision = digits.parse().map_err(|_| InvalidRevision)?;\n    Ok(Revision(revision))\n}"
  },
  {
    "name": "swapped-output-dependencies-violation",
    "ruleId": "application-use-cases-and-ports-use-case-declaration-order",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub struct RefreshCatalogOutput;\npub struct RefreshCatalogDependencies;\npub struct RefreshCatalogError;\n\n#[tracing::instrument(skip_all)]\npub async fn refresh_catalog(dependencies: RefreshCatalogDependencies) {}"
  },
  {
    "name": "swapped-output-dependencies-compliant",
    "ruleId": "application-use-cases-and-ports-use-case-declaration-order",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub struct RefreshCatalogDependencies;\npub struct RefreshCatalogOutput;\npub struct RefreshCatalogError;\n\n#[tracing::instrument(skip_all)]\npub async fn refresh_catalog(dependencies: RefreshCatalogDependencies) {}"
  },
  {
    "name": "missing-output-declaration-violation",
    "ruleId": "application-use-cases-and-ports-use-case-declaration-order",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/history/prune_history.rs",
    "before": "",
    "after": "pub struct PruneHistoryDependencies;\npub struct PruneHistoryError;\n\n#[tracing::instrument(skip_all)]\npub async fn prune_history(dependencies: PruneHistoryDependencies) {}"
  },
  {
    "name": "missing-output-declaration-compliant",
    "ruleId": "application-use-cases-and-ports-use-case-declaration-order",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/history/prune_history.rs",
    "before": "",
    "after": "use crate::history::Revision;\n\npub struct PruneHistoryDependencies;\npub struct PruneHistoryOutput { pub retained: Revision }\npub struct PruneHistoryError;\n\nconst MINIMUM_HISTORY: usize = 2;\n\n#[tracing::instrument(skip_all)]\npub async fn prune_history(dependencies: PruneHistoryDependencies) {}"
  },
  {
    "name": "remove-instrumentation-violation",
    "ruleId": "application-use-cases-and-ports-use-case-declaration-order",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/profiles/select_profile.rs",
    "before": "pub struct SelectProfileDependencies;\npub struct SelectProfileOutput;\npub struct SelectProfileError;\n\n#[tracing::instrument(skip_all)]\npub async fn select_profile(dependencies: SelectProfileDependencies) {}",
    "after": "pub struct SelectProfileDependencies;\npub struct SelectProfileOutput;\npub struct SelectProfileError;\n\npub async fn select_profile(dependencies: SelectProfileDependencies) {}"
  },
  {
    "name": "remove-instrumentation-compliant",
    "ruleId": "application-use-cases-and-ports-use-case-declaration-order",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/profiles/select_profile.rs",
    "before": "pub struct SelectProfileDependencies;\npub struct SelectProfileOutput;\npub struct SelectProfileError;\n\n#[tracing::instrument(skip_all)]\npub async fn select_profile(dependencies: SelectProfileDependencies) {}",
    "after": "pub struct SelectProfileDependencies;\npub struct SelectProfileOutput;\npub struct SelectProfileError;\n\n#[tracing::instrument(skip_all)]\npub async fn select_profile(dependencies: SelectProfileDependencies) {\n    let _dependencies = dependencies;\n}"
  },
  {
    "name": "borrowed-dependency-bundle-violation",
    "ruleId": "application-use-cases-and-ports-use-case-parameters",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/profiles/remove_profile.rs",
    "before": "",
    "after": "pub async fn remove_profile(dependencies: &RemoveProfileDependencies, profile: ProfileId) {}"
  },
  {
    "name": "borrowed-dependency-bundle-compliant",
    "ruleId": "application-use-cases-and-ports-use-case-parameters",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/profiles/remove_profile.rs",
    "before": "",
    "after": "pub async fn remove_profile(dependencies: RemoveProfileDependencies, profile: ProfileId) {}"
  },
  {
    "name": "business-values-in-dependency-bag-violation",
    "ruleId": "application-use-cases-and-ports-use-case-parameters",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/profiles/rename_profile.rs",
    "before": "",
    "after": "pub struct RenameProfileDependencies {\n    pub rename: RenameProfile,\n    pub profile: ProfileId,\n    pub new_name: ProfileName,\n}\npub async fn rename_profile(dependencies: RenameProfileDependencies) {}"
  },
  {
    "name": "business-values-in-dependency-bag-compliant",
    "ruleId": "application-use-cases-and-ports-use-case-parameters",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/profiles/rename_profile.rs",
    "before": "",
    "after": "pub struct RenameProfileDependencies { pub rename: RenameProfile }\n\npub async fn rename_profile(dependencies: RenameProfileDependencies, profile: ProfileId, new_name: ProfileName) {}"
  },
  {
    "name": "cancellation-in-middle-violation",
    "ruleId": "application-use-cases-and-ports-use-case-parameters",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub async fn refresh_catalog(dependencies: RefreshCatalogDependencies, cancellation: CancellationToken, revision: Revision) {}"
  },
  {
    "name": "cancellation-in-middle-compliant",
    "ruleId": "application-use-cases-and-ports-use-case-parameters",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub async fn refresh_catalog(dependencies: RefreshCatalogDependencies, revision: Revision, cancellation: CancellationToken) {}"
  },
  {
    "name": "zero-argument-callable-port-violation",
    "ruleId": "application-use-cases-and-ports-callable-port-invocation",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub struct RefreshCatalogDependencies { pub list_entries: Box<dyn Fn() -> Vec<Entry>> }\nfn run(dependencies: RefreshCatalogDependencies) -> Vec<Entry> {\n    (dependencies.list_entries)()\n}"
  },
  {
    "name": "zero-argument-callable-port-compliant",
    "ruleId": "application-use-cases-and-ports-callable-port-invocation",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub struct RefreshCatalogDependencies { pub list_entries: Box<dyn Fn() -> Vec<Entry>> }\nfn run(dependencies: RefreshCatalogDependencies) -> Vec<Entry> {\n    dependencies.list_entries.call(())\n}"
  },
  {
    "name": "two-argument-callable-port-violation",
    "ruleId": "application-use-cases-and-ports-callable-port-invocation",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/profiles/rename_profile.rs",
    "before": "",
    "after": "pub struct RenameProfileDependencies { pub rename: Box<dyn Fn(ProfileId, ProfileName)> }\nfn run(dependencies: RenameProfileDependencies, id: ProfileId, name: ProfileName) {\n    (dependencies.rename)(id, name);\n}"
  },
  {
    "name": "two-argument-callable-port-compliant",
    "ruleId": "application-use-cases-and-ports-callable-port-invocation",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/profiles/rename_profile.rs",
    "before": "",
    "after": "fn normalize_label(label: &str) -> String { label.trim().to_owned() }\n\npub async fn rename_profile(dependencies: RenameProfileDependencies, id: ProfileId, label: String) {\n    let name = normalize_label(&label);\n    dependencies.rename.call((id, name)).await;\n}"
  },
  {
    "name": "one-argument-callable-port-violation",
    "ruleId": "application-use-cases-and-ports-callable-port-invocation",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/profiles/remove_profile.rs",
    "before": "",
    "after": "pub struct RemoveProfileDependencies { pub remove: Box<dyn Fn(ProfileId)> }\nfn run(dependencies: RemoveProfileDependencies, id: ProfileId) {\n    (dependencies.remove)(id);\n}"
  },
  {
    "name": "one-argument-callable-port-compliant",
    "ruleId": "application-use-cases-and-ports-callable-port-invocation",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/profiles/remove_profile.rs",
    "before": "",
    "after": "pub struct RemoveProfileDependencies { pub remove: Box<dyn Fn(ProfileId)> }\nfn run(dependencies: RemoveProfileDependencies, id: ProfileId) {\n    dependencies.remove.call((id,));\n}"
  },
  {
    "name": "shared-sort-in-use-case-violation",
    "ruleId": "application-use-cases-and-ports-focused-use-case-orchestration",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub async fn refresh_catalog(dependencies: RefreshCatalogDependencies) {\n    let entries = dependencies.load.call(()).await;\n    let ordered = order_revisions(entries);\n    dependencies.save.call((ordered,)).await;\n}\n\npub(crate) fn order_revisions(mut entries: Vec<Entry>) -> Vec<Entry> {\n    entries.sort_by_key(|entry| (entry.revision, entry.id));\n    entries.dedup_by_key(|entry| entry.id);\n    entries\n}",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/export_catalog.rs"
    ]
  },
  {
    "name": "shared-sort-in-use-case-compliant",
    "ruleId": "application-use-cases-and-ports-focused-use-case-orchestration",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "use super::revision_order::order_revisions;\n\npub async fn refresh_catalog(dependencies: RefreshCatalogDependencies) {\n    let entries = dependencies.load.call(()).await;\n\n    let ordered = order_revisions(entries);\n\n    dependencies.save.call((ordered,)).await;\n}",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/export_catalog.rs"
    ]
  },
  {
    "name": "reuse-moved-to-helpers-violation",
    "ruleId": "application-use-cases-and-ports-focused-use-case-orchestration",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "mod utils;\nuse utils::merge_revisions;\n\npub async fn refresh_catalog(dependencies: RefreshCatalogDependencies) {\n    let remote = dependencies.load.call(()).await;\n    let merged = merge_revisions(remote);\n    dependencies.save.call((merged,)).await;\n}"
  },
  {
    "name": "reuse-moved-to-helpers-compliant",
    "ruleId": "application-use-cases-and-ports-focused-use-case-orchestration",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub async fn refresh_catalog(dependencies: RefreshCatalogDependencies) {\n    let remote = dependencies.load.call(()).await;\n\n    let mut revisions = Vec::new();\n    for entry in remote {\n        if entry.is_current() { revisions.push(entry.revision); }\n    }\n\n    dependencies.save.call((revisions,)).await;\n}"
  },
  {
    "name": "shared-decoder-kept-in-export-violation",
    "ruleId": "application-use-cases-and-ports-focused-use-case-orchestration",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/export_catalog.rs",
    "before": "",
    "after": "pub async fn export_catalog(dependencies: ExportCatalogDependencies) {\n    let bytes = dependencies.load.call(()).await;\n    let revisions = decode_revisions(&bytes);\n    dependencies.export.call((revisions,)).await;\n}\n\npub(crate) fn decode_revisions(bytes: &[u8]) -> Vec<u32> {\n    bytes.chunks_exact(4).map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])).collect()\n}",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/refresh_catalog.rs"
    ]
  },
  {
    "name": "shared-decoder-kept-in-export-compliant",
    "ruleId": "application-use-cases-and-ports-focused-use-case-orchestration",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/export_catalog.rs",
    "before": "",
    "after": "use super::revision_codec::decode_revisions;\n\npub async fn export_catalog(dependencies: ExportCatalogDependencies) {\n    let bytes = dependencies.load.call(()).await;\n\n    let revisions = decode_revisions(&bytes);\n\n    dependencies.export.call((revisions,)).await;\n}",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/refresh_catalog.rs"
    ]
  },
  {
    "name": "single-owner-capability-helper",
    "ruleId": "application-use-cases-and-ports-use-case-local-implementation-modules",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/revision_selection.rs",
    "before": "",
    "after": "pub(super) fn select_revisions(entries: &[Entry], minimum: Revision) -> Vec<Revision> {\n    let mut revisions: Vec<_> = entries.iter().filter(|entry| entry.revision >= minimum).map(|entry| entry.revision).collect();\n    revisions.sort_unstable();\n    revisions.dedup();\n    revisions\n}",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/refresh_catalog.rs"
    ]
  },
  {
    "name": "nested-single-owner-helper",
    "ruleId": "application-use-cases-and-ports-use-case-local-implementation-modules",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog/revision_selection.rs",
    "before": "",
    "after": "pub(super) fn select_revisions(entries: &[Entry], minimum: Revision) -> Vec<Revision> {\n    let mut revisions: Vec<_> = entries.iter().filter(|entry| entry.revision >= minimum).map(|entry| entry.revision).collect();\n    revisions.sort_unstable();\n    revisions.dedup();\n    revisions\n}",
    "referencingFiles": [
      "src/application/src/catalog/refresh_catalog.rs"
    ]
  },
  {
    "name": "generic-child-helper",
    "ruleId": "application-use-cases-and-ports-use-case-local-implementation-modules",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog/common.rs",
    "before": "",
    "after": "pub(super) fn select_revisions(entries: &[Entry], minimum: Revision) -> Vec<Revision> {\n    let mut revisions: Vec<_> = entries.iter().filter(|entry| entry.revision >= minimum).map(|entry| entry.revision).collect();\n    revisions.sort_unstable();\n    revisions.dedup();\n    revisions\n}",
    "referencingFiles": [
      "src/application/src/catalog/refresh_catalog.rs"
    ]
  },
  {
    "name": "repeated-owner-capability-helper",
    "ruleId": "application-use-cases-and-ports-use-case-local-implementation-modules",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/revision_selection.rs",
    "before": "",
    "after": "pub(super) fn select_revisions(entries: &[Entry], minimum: Revision) -> Vec<Revision> {\n    let mut revisions: Vec<_> = entries.iter().filter(|entry| entry.revision >= minimum).map(|entry| entry.revision).collect();\n    revisions.sort_unstable();\n    revisions.dedup();\n    revisions\n}",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/refresh_catalog.rs",
      "src/application/src/catalog/export_catalog.rs"
    ]
  },
  {
    "name": "single-owner-label-helper",
    "ruleId": "application-use-cases-and-ports-use-case-local-implementation-modules",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/label_normalization.rs",
    "before": "",
    "after": "pub(super) fn normalize_labels(entries: &mut [Entry]) {\n    for entry in entries {\n        entry.label = entry.label.trim().to_lowercase();\n        entry.aliases.retain(|alias| alias != &entry.label);\n        entry.aliases.sort();\n        entry.aliases.dedup();\n    }\n}",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/import_catalog.rs"
    ]
  },
  {
    "name": "delete-old-capability-helper",
    "ruleId": "application-use-cases-and-ports-use-case-local-implementation-modules",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/revision_selection.rs",
    "before": "pub(super) fn select_revisions(entries: &[Entry], minimum: Revision) -> Vec<Revision> {\n    let mut revisions: Vec<_> = entries.iter().filter(|entry| entry.revision >= minimum).map(|entry| entry.revision).collect();\n    revisions.sort_unstable();\n    revisions.dedup();\n    revisions\n}",
    "after": "",
    "referencingFiles": [
      "src/application/src/catalog/mod.rs",
      "src/application/src/catalog/refresh_catalog.rs"
    ]
  },
  {
    "name": "application-anyhow-return-violation",
    "ruleId": "errors-rootcause-lower-layer-results",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub async fn refresh_catalog() -> anyhow::Result<Catalog> {\n    load_catalog().await\n}"
  },
  {
    "name": "application-anyhow-return-compliant",
    "ruleId": "errors-rootcause-lower-layer-results",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub async fn refresh_catalog() -> rootcause::Result<Catalog, RefreshCatalogError> {\n    load_catalog().await.context(RefreshCatalogError)\n}"
  },
  {
    "name": "string-domain-error-violation",
    "ruleId": "errors-rootcause-lower-layer-results",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/catalog/revision.rs",
    "before": "",
    "after": "pub fn parse_revision(text: &str) -> Result<Revision, String> {\n    text.parse().map(Revision).map_err(|error| format!(\"invalid revision: {error}\"))\n}"
  },
  {
    "name": "string-domain-error-compliant",
    "ruleId": "errors-rootcause-lower-layer-results",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/catalog/revision.rs",
    "before": "",
    "after": "pub fn parse_revision(text: &str) -> rootcause::Result<Revision, InvalidRevision> {\n    text.parse().map(Revision).context(InvalidRevision)\n}\n\nfn revision_is_current(revision: Revision) -> bool { revision.0 > 0 }"
  },
  {
    "name": "infrastructure-eyre-return-violation",
    "ruleId": "errors-rootcause-lower-layer-results",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/settings/src/read_manifest.rs",
    "before": "",
    "after": "pub fn read_manifest(path: &Path) -> color_eyre::Result<String> {\n    Ok(read_to_string(path)?)\n}"
  },
  {
    "name": "infrastructure-eyre-return-compliant",
    "ruleId": "errors-rootcause-lower-layer-results",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/settings/src/read_manifest.rs",
    "before": "",
    "after": "pub fn read_manifest(path: &Path) -> rootcause::Result<String, ReadManifestError> {\n    read_to_string(path).context(ReadManifestError)\n}"
  },
  {
    "name": "erase-io-cause-violation",
    "ruleId": "errors-preserve-causes-at-owned-boundaries",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/settings/src/read_manifest.rs",
    "before": "fn read_manifest(path: &Path) -> rootcause::Result<String, ReadManifestError> {\n    read_to_string(path).context(ReadManifestError)\n}",
    "after": "fn read_manifest(path: &Path) -> rootcause::Result<String, ReadManifestError> {\n    read_to_string(path).map_err(|_| ReadManifestError.into_report())\n}"
  },
  {
    "name": "erase-io-cause-compliant",
    "ruleId": "errors-preserve-causes-at-owned-boundaries",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/settings/src/read_manifest.rs",
    "before": "fn read_manifest(path: &Path) -> rootcause::Result<String, ReadManifestError> {\n    read_to_string(path).context(ReadManifestError)\n}",
    "after": "fn read_manifest(path: &Path) -> rootcause::Result<String, ReadManifestError> {\n    let manifest = read_to_string(path).context(ReadManifestError)?;\n    Ok(manifest)\n}"
  },
  {
    "name": "stringify-provider-cause-violation",
    "ruleId": "errors-preserve-causes-at-owned-boundaries",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/game_platform/src/revision.rs",
    "before": "",
    "after": "async fn fetch_revision(client: &Client) -> rootcause::Result<Revision, FetchRevisionError> {\n    client.revision().await.map_err(|error| rootcause::report!(error.to_string()).context(FetchRevisionError))\n}"
  },
  {
    "name": "stringify-provider-cause-compliant",
    "ruleId": "errors-preserve-causes-at-owned-boundaries",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/game_platform/src/revision.rs",
    "before": "",
    "after": "fn validate_revision(revision: Revision) -> rootcause::Result<(), InvalidRevision> {\n    if revision.0 == 0 {\n        return Err(InvalidRevision.into_report());\n    }\n    Ok(())\n}"
  },
  {
    "name": "remove-owned-context-violation",
    "ruleId": "errors-preserve-causes-at-owned-boundaries",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/import_catalog.rs",
    "before": "async fn import_catalog(dependencies: ImportCatalogDependencies) -> rootcause::Result<Catalog, ImportCatalogError> {\n    dependencies.read.call(()).await.context(ImportCatalogError)\n}",
    "after": "async fn import_catalog(dependencies: ImportCatalogDependencies) -> rootcause::Result<Catalog, ImportCatalogError> {\n    dependencies.read.call(()).await.map_err(|_| ImportCatalogError.into_report())\n}"
  },
  {
    "name": "remove-owned-context-compliant",
    "ruleId": "errors-preserve-causes-at-owned-boundaries",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/import_catalog.rs",
    "before": "async fn import_catalog(dependencies: ImportCatalogDependencies) -> rootcause::Result<Catalog, ImportCatalogError> {\n    dependencies.read.call(()).await.context(ImportCatalogError)\n}",
    "after": "async fn import_catalog(dependencies: ImportCatalogDependencies) -> rootcause::Result<Catalog, ImportCatalogError> {\n    let catalog = dependencies.read.call(()).await.context(ImportCatalogError)?;\n    Ok(catalog)\n}"
  },
  {
    "name": "transform-owned-boundary-violation",
    "ruleId": "errors-context-propagation",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "async fn refresh_catalog(dependencies: RefreshCatalogDependencies) -> rootcause::Result<Catalog, RefreshCatalogError> {\n    dependencies.load.call(()).await.context(RefreshCatalogError)\n}",
    "after": "async fn refresh_catalog(dependencies: RefreshCatalogDependencies) -> rootcause::Result<Catalog, RefreshCatalogError> {\n    dependencies.load.call(()).await.context_transform(RefreshCatalogError)\n}"
  },
  {
    "name": "transform-owned-boundary-compliant",
    "ruleId": "errors-context-propagation",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "async fn refresh_catalog(dependencies: RefreshCatalogDependencies) -> rootcause::Result<Catalog, RefreshCatalogError> {\n    dependencies.load.call(()).await.context(RefreshCatalogError)\n}",
    "after": "async fn refresh_catalog(dependencies: RefreshCatalogDependencies) -> rootcause::Result<Catalog, RefreshCatalogError> {\n    let catalog = dependencies.load.call(()).await.context(RefreshCatalogError)?;\n    Ok(catalog)\n}"
  },
  {
    "name": "competing-use-case-contexts-violation",
    "ruleId": "errors-context-propagation",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/synchronize.rs",
    "before": "",
    "after": "pub async fn synchronize(dependencies: SynchronizeDependencies) -> rootcause::Result<()> {\n    dependencies.read.call(()).await.context(ReadStageError)?;\n    dependencies.write.call(()).await.context(WriteStageError)?;\n    Ok(())\n}"
  },
  {
    "name": "competing-use-case-contexts-compliant",
    "ruleId": "errors-context-propagation",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/synchronize.rs",
    "before": "",
    "after": "pub async fn synchronize(dependencies: SynchronizeDependencies) -> rootcause::Result<(), SynchronizeError> {\n    dependencies.read.call(()).await.context(SynchronizeError)?;\n    dependencies.write.call(()).await.context(SynchronizeError)?;\n    Ok(())\n}\n\nfn forward_report(result: rootcause::Result<()>) -> rootcause::Result<()> {\n    result.into_report()\n}"
  },
  {
    "name": "remove-child-semantic-marker-violation",
    "ruleId": "errors-context-propagation",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/profiles/choose_profile.rs",
    "before": "pub async fn choose_profile(dependencies: ChooseProfileDependencies) -> rootcause::Result<(), ChooseProfileError> {\n    dependencies.select.call(()).await.context(ChooseProfileError)\n}",
    "after": "pub async fn choose_profile(dependencies: ChooseProfileDependencies) -> rootcause::Result<(), ChooseProfileError> {\n    dependencies.select.call(()).await.context_transform(ChooseProfileError)\n}"
  },
  {
    "name": "remove-child-semantic-marker-compliant",
    "ruleId": "errors-context-propagation",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/profiles/choose_profile.rs",
    "before": "pub async fn choose_profile(dependencies: ChooseProfileDependencies) -> rootcause::Result<(), ChooseProfileError> {\n    dependencies.select.call(()).await.context(ChooseProfileError)\n}",
    "after": "pub async fn choose_profile(dependencies: ChooseProfileDependencies) -> rootcause::Result<(), ChooseProfileError> {\n    let selected = dependencies.select.call(()).await.context(ChooseProfileError)?;\n    Ok(selected)\n}"
  },
  {
    "name": "terminal-debug-report-violation",
    "ruleId": "errors-presentation-error-allowlists",
    "split": "train",
    "expectedViolation": true,
    "path": "src/presentation/cli/src/catalog.rs",
    "before": "",
    "after": "fn print_failure(report: &Report) {\n    eprintln!(\"Catalog refresh failed: {report:#?}\");\n}"
  },
  {
    "name": "terminal-debug-report-compliant",
    "ruleId": "errors-presentation-error-allowlists",
    "split": "train",
    "expectedViolation": false,
    "path": "src/presentation/cli/src/catalog.rs",
    "before": "",
    "after": "fn print_failure(report: &Report) {\n    let message = if report.contains::<Offline>() { \"Registry unavailable\" } else { \"Refresh failed\" };\n    eprintln!(\"{message}\");\n}"
  },
  {
    "name": "transport-display-report-violation",
    "ruleId": "errors-presentation-error-allowlists",
    "split": "train",
    "expectedViolation": true,
    "path": "src/presentation/mcp/src/errors.rs",
    "before": "",
    "after": "fn public_error(report: &Report) -> RpcError {\n    RpcError { code: 500, message: report.to_string() }\n}"
  },
  {
    "name": "transport-display-report-compliant",
    "ruleId": "errors-presentation-error-allowlists",
    "split": "train",
    "expectedViolation": false,
    "path": "src/presentation/mcp/src/errors.rs",
    "before": "",
    "after": "fn public_error(report: &Report) -> RpcError {\n    if report.contains::<NotFound>() {\n        return RpcError { code: 404, message: \"Profile not found\".into() };\n    }\n    RpcError { code: 500, message: \"Operation failed\".into() }\n}"
  },
  {
    "name": "expose-report-as-json-detail-violation",
    "ruleId": "errors-presentation-error-allowlists",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/presentation/mcp/src/catalog.rs",
    "before": "",
    "after": "fn failure_payload(report: &Report) -> Value {\n    json!({\"status\": \"failed\", \"detail\": format!(\"{report}\")})\n}"
  },
  {
    "name": "expose-report-as-json-detail-compliant",
    "ruleId": "errors-presentation-error-allowlists",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/presentation/mcp/src/catalog.rs",
    "before": "",
    "after": "fn failure_payload(report: &Report) -> Value {\n    let code = if report.contains::<Cancelled>() { \"cancelled\" } else { \"failed\" };\n    json!({\"status\": code})\n}"
  },
  {
    "name": "custom-one-shot-signal-violation",
    "ruleId": "runtime-primitives-primitive-selection-order",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/execution/src/ready.rs",
    "before": "",
    "after": "struct ReadySignal {\n    ready: AtomicBool,\n    waiter: Mutex<Option<Waker>>,\n}\n\nimpl ReadySignal {\n    fn signal(&self) {\n        self.ready.store(true, Ordering::Release);\n        if let Some(waiter) = self.waiter.lock().unwrap().take() { waiter.wake(); }\n    }\n}"
  },
  {
    "name": "custom-one-shot-signal-compliant",
    "ruleId": "runtime-primitives-primitive-selection-order",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/execution/src/ready.rs",
    "before": "",
    "after": "use tokio::sync::oneshot;\n\nasync fn wait_for_ready() -> Result<(), ReadyError> {\n    let (sender, receiver) = oneshot::channel();\n    start_worker(sender);\n    receiver.await.context(ReadyError)\n}"
  },
  {
    "name": "custom-counting-permit-violation",
    "ruleId": "runtime-primitives-primitive-selection-order",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/execution/src/pending.rs",
    "before": "",
    "after": "struct PermitPool { available: AtomicUsize }\nimpl PermitPool {\n    fn acquire(&self) {\n        while self.available.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| count.checked_sub(1)).is_err() {\n            thread::yield_now();\n        }\n    }\n    fn release(&self) { self.available.fetch_add(1, Ordering::Release); }\n}"
  },
  {
    "name": "custom-counting-permit-compliant",
    "ruleId": "runtime-primitives-primitive-selection-order",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/execution/src/pending.rs",
    "before": "",
    "after": "use std::collections::VecDeque;\n\nstruct PendingImports { profiles: VecDeque<ProfileId> }\n\nimpl PendingImports {\n    fn push(&mut self, profile: ProfileId) { self.profiles.push_back(profile); }\n    fn next(&mut self) -> Option<ProfileId> { self.profiles.pop_front() }\n}"
  },
  {
    "name": "custom-async-notification-violation",
    "ruleId": "runtime-primitives-primitive-selection-order",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/execution/src/notification.rs",
    "before": "",
    "after": "struct ChangeNotification { generation: AtomicU64, waiters: Mutex<Vec<Waker>> }\nimpl ChangeNotification {\n    fn notify(&self) {\n        self.generation.fetch_add(1, Ordering::Release);\n        for waiter in self.waiters.lock().unwrap().drain(..) { waiter.wake(); }\n    }\n}"
  },
  {
    "name": "custom-async-notification-compliant",
    "ruleId": "runtime-primitives-primitive-selection-order",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/execution/src/notification.rs",
    "before": "",
    "after": "use tokio::sync::Notify;\n\nstruct ChangeNotification { notify: Notify }\nimpl ChangeNotification {\n    fn notify(&self) { self.notify.notify_waiters(); }\n}"
  },
  {
    "name": "unjustified-custom-latch-violation",
    "ruleId": "runtime-primitives-custom-primitive-justification",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/execution/src/commit_agreement.rs",
    "before": "",
    "after": "struct CompletionLatch {\n    remaining: AtomicUsize,\n    waiters: Mutex<Vec<Waker>>,\n}\nimpl CompletionLatch {\n    fn complete(&self) {\n        if self.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {\n            for waiter in self.waiters.lock().unwrap().drain(..) { waiter.wake(); }\n        }\n    }\n}"
  },
  {
    "name": "unjustified-custom-latch-compliant",
    "ruleId": "runtime-primitives-custom-primitive-justification",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/execution/src/commit_agreement.rs",
    "before": "",
    "after": "// The provider acknowledges a commit only after both the journal and catalog\n// reach the same revision; a generic latch cannot express revision equality.\n// Completion is published only for equal revisions, never for mixed generations.\nstruct CommitAgreement { journal: Option<Revision>, catalog: Option<Revision> }\nimpl CommitAgreement {\n    fn agreed_revision(&self) -> Option<Revision> {\n        match (self.journal, self.catalog) {\n            (Some(journal), Some(catalog)) if journal == catalog => Some(journal),\n            _ => None,\n        }\n    }\n}"
  },
  {
    "name": "replace-ordinary-collection-violation",
    "ruleId": "runtime-primitives-custom-primitive-justification",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/settings/src/recent.rs",
    "before": "",
    "after": "struct TinyStack<T> { slots: [Option<T>; 8], used: usize }\nimpl<T> TinyStack<T> {\n    fn push(&mut self, value: T) { self.slots[self.used] = Some(value); self.used += 1; }\n    fn pop(&mut self) -> Option<T> {\n        if self.used == 0 { return None; }\n        self.used -= 1;\n        self.slots[self.used].take()\n    }\n}"
  },
  {
    "name": "replace-ordinary-collection-compliant",
    "ruleId": "runtime-primitives-custom-primitive-justification",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/settings/src/recent.rs",
    "before": "",
    "after": "struct RecentProfiles { profiles: Vec<ProfileId> }\nimpl RecentProfiles {\n    fn record(&mut self, profile: ProfileId) { self.profiles.push(profile); }\n    fn latest(&self) -> Option<&ProfileId> { self.profiles.last() }\n}"
  },
  {
    "name": "remove-custom-invariants-violation",
    "ruleId": "runtime-primitives-custom-primitive-justification",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/execution/src/generation_barrier.rs",
    "before": "// Generic barriers count arrivals; this protocol requires matching revisions.\n// Publication is allowed only when staged and durable revisions are identical.\nstruct GenerationBarrier { staged: Revision, durable: Revision }\nimpl GenerationBarrier {\n    fn ready(&self) -> bool { self.staged == self.durable }\n}",
    "after": "struct GenerationBarrier { staged: Revision, durable: Revision }\nimpl GenerationBarrier {\n    fn ready(&self) -> bool { self.staged == self.durable }\n}"
  },
  {
    "name": "remove-custom-invariants-compliant",
    "ruleId": "runtime-primitives-custom-primitive-justification",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/execution/src/generation_barrier.rs",
    "before": "// Generic barriers count arrivals; this protocol requires matching revisions.\n// Publication is allowed only when staged and durable revisions are identical.\nstruct GenerationBarrier { staged: Revision, durable: Revision }\nimpl GenerationBarrier {\n    fn ready(&self) -> bool { self.staged == self.durable }\n}",
    "after": "// Generic barriers count arrivals; this protocol requires matching revisions.\n// Publication is allowed only when staged and durable revisions are identical.\nstruct GenerationBarrier { staged: Revision, durable: Revision }\nimpl GenerationBarrier {\n    fn ready(&self) -> bool { self.durable == self.staged }\n}"
  },
  {
    "name": "wrapped-port-cancellation-violation",
    "ruleId": "runtime-primitives-cancellation-propagation-and-checkpoints",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/profiles/transfer_profile.rs",
    "before": "",
    "after": "pub struct TransferControl { pub token: CancellationToken }\npub async fn transfer_profile(dependencies: TransferProfileDependencies, id: ProfileId, cancellation: CancellationToken) {\n    dependencies.copy.call((id, TransferControl { token: cancellation })).await;\n}"
  },
  {
    "name": "wrapped-port-cancellation-compliant",
    "ruleId": "runtime-primitives-cancellation-propagation-and-checkpoints",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/profiles/transfer_profile.rs",
    "before": "",
    "after": "pub async fn transfer_profile(dependencies: TransferProfileDependencies, id: ProfileId, cancellation: CancellationToken) {\n    dependencies.copy.call((id, cancellation)).await;\n}"
  },
  {
    "name": "helper-checkpoint-violation",
    "ruleId": "runtime-primitives-cancellation-propagation-and-checkpoints",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/archive/src/scan.rs",
    "before": "",
    "after": "fn ensure_running(token: &CancellationToken) -> rootcause::Result<(), ScanCancelled> {\n    if token.is_cancelled() { return Err(ScanCancelled.into_report()); }\n    Ok(())\n}\nasync fn scan(paths: Vec<PathBuf>, cancellation: CancellationToken) -> rootcause::Result<(), ScanCancelled> {\n    for path in paths {\n        ensure_running(&cancellation)?;\n        scan_path(path).await;\n    }\n    Ok(())\n}"
  },
  {
    "name": "helper-checkpoint-compliant",
    "ruleId": "runtime-primitives-cancellation-propagation-and-checkpoints",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/archive/src/scan.rs",
    "before": "",
    "after": "async fn scan(paths: Vec<PathBuf>, cancellation: CancellationToken) -> rootcause::Result<(), ScanCancelled> {\n    for path in paths {\n        if cancellation.is_cancelled() { return Err(ScanCancelled.into_report()); }\n\n        scan_path(path).await;\n    }\n    Ok(())\n}"
  },
  {
    "name": "replace-incoming-token-violation",
    "ruleId": "runtime-primitives-cancellation-propagation-and-checkpoints",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/export_catalog.rs",
    "before": "",
    "after": "pub async fn export_catalog(dependencies: ExportCatalogDependencies, cancellation: CancellationToken) {\n    dependencies.write.call((CancellationToken::new(),)).await;\n}"
  },
  {
    "name": "replace-incoming-token-compliant",
    "ruleId": "runtime-primitives-cancellation-propagation-and-checkpoints",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/export_catalog.rs",
    "before": "",
    "after": "pub async fn export_catalog(dependencies: ExportCatalogDependencies, cancellation: CancellationToken) {\n    dependencies.write.call((cancellation,)).await;\n}"
  },
  {
    "name": "rollback-partial-copy-violation",
    "ruleId": "runtime-primitives-cancellation-state-preservation",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/archive/src/copy_records.rs",
    "before": "",
    "after": "async fn copy_records(staging: &Path, cancellation: CancellationToken) -> Result<()> {\n    write_first_batch(staging).await?;\n    if cancellation.is_cancelled() {\n        truncate_staging(staging).await?;\n        return Err(CopyCancelled.into_report());\n    }\n    write_remaining_batches(staging).await\n}"
  },
  {
    "name": "rollback-partial-copy-compliant",
    "ruleId": "runtime-primitives-cancellation-state-preservation",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/archive/src/copy_records.rs",
    "before": "",
    "after": "async fn copy_records(staging: &Path, cancellation: CancellationToken) -> Result<()> {\n    write_first_batch(staging).await?;\n    if cancellation.is_cancelled() {\n        return Err(CopyCancelled.into_report());\n    }\n    write_remaining_batches(staging).await\n}"
  },
  {
    "name": "cancel-after-final-publication-violation",
    "ruleId": "runtime-primitives-cancellation-state-preservation",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/archive/src/publish.rs",
    "before": "",
    "after": "async fn publish(staging: &Path, target: &Path, cancellation: CancellationToken) -> Result<()> {\n    // This rename is the final irreversible mutation; no writes follow.\n    rename(staging, target).await?;\n    if cancellation.is_cancelled() { return Err(PublishCancelled.into_report()); }\n    Ok(())\n}"
  },
  {
    "name": "cancel-after-final-publication-compliant",
    "ruleId": "runtime-primitives-cancellation-state-preservation",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/archive/src/publish.rs",
    "before": "",
    "after": "async fn publish(staging: &Path, target: &Path, cancellation: CancellationToken) -> Result<()> {\n    if cancellation.is_cancelled() { return Err(PublishCancelled.into_report()); }\n\n    // This rename is the final irreversible mutation; cancellation cannot undo it.\n    rename(staging, target).await?;\n    Ok(())\n}"
  },
  {
    "name": "delete-completed-success-guard-violation",
    "ruleId": "runtime-primitives-cancellation-state-preservation",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/archive/src/settle_copy.rs",
    "before": "// published is true only after the final irreversible rename succeeded.\nasync fn settle_copy(published: bool, cancellation: CancellationToken) -> Result<()> {\n    if published { return Ok(()); }\n    if cancellation.is_cancelled() { return Err(CopyCancelled.into_report()); }\n    Ok(())\n}",
    "after": "// published is true only after the final irreversible rename succeeded.\nasync fn settle_copy(published: bool, cancellation: CancellationToken) -> Result<()> {\n    if cancellation.is_cancelled() { return Err(CopyCancelled.into_report()); }\n    Ok(())\n}"
  },
  {
    "name": "delete-completed-success-guard-compliant",
    "ruleId": "runtime-primitives-cancellation-state-preservation",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/archive/src/settle_copy.rs",
    "before": "// published is true only after the final irreversible rename succeeded.\nasync fn settle_copy(published: bool, cancellation: CancellationToken) -> Result<()> {\n    if published { return Ok(()); }\n    if cancellation.is_cancelled() { return Err(CopyCancelled.into_report()); }\n    Ok(())\n}",
    "after": "// published is true only after the final irreversible rename succeeded.\nasync fn settle_copy(published: bool, cancellation: CancellationToken) -> Result<()> {\n    if published { return Ok(()); }\n    if cancellation.is_cancelled() { return Err(CopyCancelled.into_report()); }\n\n    Ok(())\n}"
  },
  {
    "name": "callback-cancels-copy-violation",
    "ruleId": "runtime-primitives-separate-progress-reporting",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/archive/src/copy_chunks.rs",
    "before": "",
    "after": "type ReportCopyProgress = Box<dyn Fn(u64) -> bool>;\nasync fn copy_chunks(progress: ReportCopyProgress) -> Result<()> {\n    for chunk in chunks() {\n        if !progress(chunk.index) { return Err(CopyCancelled.into_report()); }\n        write_chunk(chunk).await?;\n    }\n    Ok(())\n}"
  },
  {
    "name": "callback-cancels-copy-compliant",
    "ruleId": "runtime-primitives-separate-progress-reporting",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/archive/src/copy_chunks.rs",
    "before": "",
    "after": "type ReportCopyProgress = Box<dyn Fn(u64)>;\nasync fn copy_chunks(progress: ReportCopyProgress, cancellation: CancellationToken) -> Result<()> {\n    for chunk in chunks() {\n        if cancellation.is_cancelled() { return Err(CopyCancelled.into_report()); }\n        progress(chunk.index);\n        write_chunk(chunk).await?;\n    }\n    Ok(())\n}"
  },
  {
    "name": "combined-control-port-violation",
    "ruleId": "runtime-primitives-separate-progress-reporting",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/archive/ports.rs",
    "before": "",
    "after": "pub trait ScanObserver {\n    fn item_scanned(&self, path: &ArchivePath);\n    fn should_stop(&self) -> bool;\n}"
  },
  {
    "name": "combined-control-port-compliant",
    "ruleId": "runtime-primitives-separate-progress-reporting",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/archive/ports.rs",
    "before": "",
    "after": "pub trait ScanObserver {\n    fn item_scanned(&self, path: &ArchivePath);\n    fn bytes_scanned(&self, bytes: u64);\n}\n\npub struct ScanDependencies { pub progress: Box<dyn ScanObserver> }\n\npub async fn scan(dependencies: ScanDependencies, cancellation: CancellationToken) {}"
  },
  {
    "name": "progress-event-returns-stop-command-violation",
    "ruleId": "runtime-primitives-separate-progress-reporting",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/archive/ports.rs",
    "before": "",
    "after": "pub enum ProgressReply { Continue, Cancel }\npub type ReportProgress = Box<dyn Fn(ProgressEvent) -> ProgressReply>;"
  },
  {
    "name": "progress-event-returns-stop-command-compliant",
    "ruleId": "runtime-primitives-separate-progress-reporting",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/archive/ports.rs",
    "before": "",
    "after": "pub type ReportProgress = Box<dyn Fn(ProgressEvent)>;\n\npub async fn transfer(dependencies: TransferDependencies, progress: ReportProgress, cancellation: CancellationToken) {}"
  },
  {
    "name": "boolean-dispatch-violation",
    "ruleId": "control-flow-choose-the-narrow-conditional-form",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn update_catalog(offline: bool) {\n    match offline {\n        true => use_cached_catalog(),\n        false => fetch_catalog(),\n    }\n}"
  },
  {
    "name": "boolean-dispatch-compliant",
    "ruleId": "control-flow-choose-the-narrow-conditional-form",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn update_catalog(offline: bool) {\n    if offline { use_cached_catalog(); } else { fetch_catalog(); }\n}"
  },
  {
    "name": "required-id-nested-match-violation",
    "ruleId": "control-flow-choose-the-narrow-conditional-form",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn remove_selected(selected: Option<ProfileId>) -> Result<()> {\n    match selected {\n        Some(id) => { remove(id)?; Ok(()) }\n        None => return Err(NoSelection.into_report()),\n    }\n}"
  },
  {
    "name": "required-id-nested-match-compliant",
    "ruleId": "control-flow-choose-the-narrow-conditional-form",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn action_label(action: Action) -> &'static str {\n    match action {\n        Action::Install => \"Install\",\n        Action::Repair => \"Repair\",\n        Action::Remove => \"Remove\",\n    }\n}"
  },
  {
    "name": "optional-notification-match-violation",
    "ruleId": "control-flow-choose-the-narrow-conditional-form",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn notify_selected(selected: Option<ProfileId>) {\n    match selected {\n        Some(id) => notify(id),\n        None => (),\n    }\n}"
  },
  {
    "name": "optional-notification-match-compliant",
    "ruleId": "control-flow-choose-the-narrow-conditional-form",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn notify_selected(selected: Option<ProfileId>) {\n    if let Some(id) = selected { notify(id); }\n}"
  },
  {
    "name": "nested-writable-success-violation",
    "ruleId": "control-flow-guard-clauses",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn rename_profile(profile: &Profile) -> Result<()> {\n    if profile.writable {\n        reserve_name(profile)?;\n        rename(profile)?;\n        Ok(())\n    } else {\n        Err(ReadOnly.into_report())\n    }\n}"
  },
  {
    "name": "nested-writable-success-compliant",
    "ruleId": "control-flow-guard-clauses",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn rename_profile(profile: &Profile) -> Result<()> {\n    if !profile.writable { return Err(ReadOnly.into_report()); }\n\n    reserve_name(profile)?;\n    rename(profile)?;\n    Ok(())\n}"
  },
  {
    "name": "else-after-continue-violation",
    "ruleId": "control-flow-guard-clauses",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn enqueue(profiles: Vec<Profile>) {\n    for profile in profiles {\n        if profile.archived {\n            continue;\n        } else {\n            queue(profile);\n        }\n    }\n}"
  },
  {
    "name": "else-after-continue-compliant",
    "ruleId": "control-flow-guard-clauses",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn enqueue(profiles: Vec<Profile>) {\n    for profile in profiles {\n        if profile.archived { queue_cold(profile); } else { queue_hot(profile); }\n        record_queue_event();\n    }\n}"
  },
  {
    "name": "else-after-return-violation",
    "ruleId": "control-flow-guard-clauses",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn select(id: ProfileId) -> Result<Profile> {\n    if id.is_reserved() {\n        return Err(Reserved.into_report());\n    } else {\n        load_profile(id)\n    }\n}"
  },
  {
    "name": "else-after-return-compliant",
    "ruleId": "control-flow-guard-clauses",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn select(id: ProfileId) -> Result<Profile> {\n    if id.is_reserved() { return Err(Reserved.into_report()); }\n\n    load_profile(id)\n}"
  },
  {
    "name": "match-only-error-pattern-violation",
    "ruleId": "control-flow-match-only-for-multi-way-logic",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn record_result(result: Result<Profile, LoadError>) {\n    match result {\n        Err(error) => record_failure(error),\n        _ => (),\n    }\n}"
  },
  {
    "name": "match-only-error-pattern-compliant",
    "ruleId": "control-flow-match-only-for-multi-way-logic",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn record_result(result: Result<Profile, LoadError>) {\n    if let Err(error) = result { record_failure(error); }\n}"
  },
  {
    "name": "boolean-side-effect-match-violation",
    "ruleId": "control-flow-match-only-for-multi-way-logic",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn set_visibility(visible: bool) {\n    match visible {\n        true => show_profiles(),\n        false => hide_profiles(),\n    }\n}"
  },
  {
    "name": "boolean-side-effect-match-compliant",
    "ruleId": "control-flow-match-only-for-multi-way-logic",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn severity_label(severity: Severity) -> &'static str {\n    match severity {\n        Severity::Warning => \"warning\",\n        Severity::Error => \"error\",\n    }\n}"
  },
  {
    "name": "single-relevant-enum-variant-violation",
    "ruleId": "control-flow-match-only-for-multi-way-logic",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn observe(event: Event) {\n    match event {\n        Event::Published(revision) => announce(revision),\n        _ => {},\n    }\n}"
  },
  {
    "name": "single-relevant-enum-variant-compliant",
    "ruleId": "control-flow-match-only-for-multi-way-logic",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn observe(event: Event) {\n    if let Event::Published(revision) = event { announce(revision); }\n}"
  },
  {
    "name": "fragment-single-use-label-flow-violation",
    "ruleId": "functions-and-tests-cohesive-orchestration",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn rename_profile(profile: &mut Profile, label: String) -> Result<()> {\n    if label.trim().is_empty() { return Err(EmptyLabel.into_report()); }\n\n    profile.label = label.trim().to_owned();\n    persist_profile(profile)\n}",
    "after": "fn rename_profile(profile: &mut Profile, label: String) -> Result<()> {\n    check_label(&label)?;\n    assign_label(profile, label);\n    finish_rename(profile)\n}\nfn check_label(label: &str) -> Result<()> {\n    if label.trim().is_empty() { return Err(EmptyLabel.into_report()); }\n    Ok(())\n}\nfn assign_label(profile: &mut Profile, label: String) { profile.label = label.trim().to_owned(); }\nfn finish_rename(profile: &Profile) -> Result<()> { persist_profile(profile) }"
  },
  {
    "name": "fragment-single-use-label-flow-compliant",
    "ruleId": "functions-and-tests-cohesive-orchestration",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn rename_profile(profile: &mut Profile, label: String) -> Result<()> {\n    if label.trim().is_empty() { return Err(EmptyLabel.into_report()); }\n\n    profile.label = label.trim().to_owned();\n    persist_profile(profile)\n}",
    "after": "fn rename_profile(profile: &mut Profile, label: String) -> Result<()> {\n    if label.trim().is_empty() { return Err(EmptyLabel.into_report()); }\n\n    profile.label = label.trim().to_owned();\n\n    persist_profile(profile)\n}"
  },
  {
    "name": "fragment-one-use-queueing-violation",
    "ruleId": "functions-and-tests-cohesive-orchestration",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn schedule(profiles: Vec<Profile>) -> Vec<Job> {\n    profiles.into_iter().filter(|profile| profile.enabled).map(Job::from).collect()\n}",
    "after": "fn schedule(profiles: Vec<Profile>) -> Vec<Job> {\n    let selected = select_profiles(profiles);\n    make_jobs(selected)\n}\nfn select_profiles(profiles: Vec<Profile>) -> Vec<Profile> {\n    profiles.into_iter().filter(|profile| profile.enabled).collect()\n}\nfn make_jobs(profiles: Vec<Profile>) -> Vec<Job> { profiles.into_iter().map(Job::from).collect() }"
  },
  {
    "name": "fragment-one-use-queueing-compliant",
    "ruleId": "functions-and-tests-cohesive-orchestration",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn schedule(profiles: Vec<Profile>) -> Vec<Job> {\n    profiles.into_iter().filter(|profile| profile.enabled).map(Job::from).collect()\n}",
    "after": "fn schedule(profiles: Vec<Profile>) -> Vec<Job> {\n    profiles.into_iter().filter(|profile| profile.enabled).map(Job::from).collect()\n}\n\nfn restore(bytes: &[u8]) -> Result<Manifest> {\n    // Decoding is a shared wire-format algorithm, not a scheduling step.\n    manifest_codec::decode(bytes)\n}"
  },
  {
    "name": "extract-numbered-steps-violation",
    "ruleId": "functions-and-tests-cohesive-orchestration",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn activate(profile: &mut Profile) -> Result<()> {\n    require_ready(profile)?;\n    profile.active = true;\n    save(profile)\n}",
    "after": "fn activate(profile: &mut Profile) -> Result<()> {\n    step_one(profile)?;\n    step_two(profile);\n    step_three(profile)\n}\nfn step_one(profile: &Profile) -> Result<()> { require_ready(profile) }\nfn step_two(profile: &mut Profile) { profile.active = true; }\nfn step_three(profile: &Profile) -> Result<()> { save(profile) }"
  },
  {
    "name": "extract-numbered-steps-compliant",
    "ruleId": "functions-and-tests-cohesive-orchestration",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "fn activate(profile: &mut Profile) -> Result<()> {\n    require_ready(profile)?;\n    profile.active = true;\n    save(profile)\n}",
    "after": "fn activate(profile: &mut Profile) -> Result<()> {\n    require_ready(profile)?;\n\n    profile.active = true;\n\n    save(profile)\n}"
  },
  {
    "name": "one-use-condition-wrapper-violation",
    "ruleId": "functions-and-tests-helpers-earn-an-interface",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn can_refresh(revision: Revision) -> bool { revision.0 > 0 }\n\nfn refresh(revision: Revision) {\n    if can_refresh(revision) { enqueue(revision); }\n}"
  },
  {
    "name": "one-use-condition-wrapper-compliant",
    "ruleId": "functions-and-tests-helpers-earn-an-interface",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn refresh(revision: Revision) {\n    if revision.0 > 0 { enqueue(revision); }\n}"
  },
  {
    "name": "single-forwarder-without-seam-violation",
    "ruleId": "functions-and-tests-helpers-earn-an-interface",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn save_now(profile: &Profile) -> Result<()> { save(profile) }\n\nfn rename(profile: &mut Profile, name: String) -> Result<()> {\n    profile.name = name;\n    save_now(profile)\n}"
  },
  {
    "name": "single-forwarder-without-seam-compliant",
    "ruleId": "functions-and-tests-helpers-earn-an-interface",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn reserved_profile_name(name: &str) -> bool {\n    name.eq_ignore_ascii_case(\"system\") || name.eq_ignore_ascii_case(\"default\")\n}\n\nfn create(name: &str) -> Result<()> {\n    if reserved_profile_name(name) { return Err(ReservedName.into_report()); }\n    create_record(name)\n}\n\nfn rename(id: ProfileId, name: &str) -> Result<()> {\n    if reserved_profile_name(name) { return Err(ReservedName.into_report()); }\n    rename_record(id, name)\n}"
  },
  {
    "name": "single-expression-transform-helper-violation",
    "ruleId": "functions-and-tests-helpers-earn-an-interface",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn label_text(label: &str) -> String { label.trim().to_owned() }\n\nfn rename(profile: &mut Profile, label: &str) { profile.label = label_text(label); }"
  },
  {
    "name": "single-expression-transform-helper-compliant",
    "ruleId": "functions-and-tests-helpers-earn-an-interface",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn rename(profile: &mut Profile, label: &str) { profile.label = label.trim().to_owned(); }"
  },
  {
    "name": "integration-test-target",
    "ruleId": "functions-and-tests-pre-mvp-test-placement",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/tests/profile_roundtrip.rs",
    "before": "",
    "after": "#[test]\nfn public_profile_roundtrip() {\n    let profile = domain::Profile::new(\"work\");\n    assert_eq!(profile.name(), \"work\");\n}"
  },
  {
    "name": "colocated-profile-unit-test",
    "ruleId": "functions-and-tests-pre-mvp-test-placement",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/profiles/name.rs",
    "before": "",
    "after": "pub fn normalize_profile_name(name: &str) -> String { name.trim().to_owned() }\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn trims_display_name() { assert_eq!(normalize_profile_name(\" work \"), \"work\"); }\n}"
  },
  {
    "name": "cross-file-test-module",
    "ruleId": "functions-and-tests-pre-mvp-test-placement",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/profiles/name.rs",
    "before": "",
    "after": "pub fn is_reserved(name: &str) -> bool { name == \"system\" }\n\n#[cfg(test)]\nmod tests;"
  },
  {
    "name": "test-fixture-inside-unit-module",
    "ruleId": "functions-and-tests-pre-mvp-test-placement",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/profiles/profile.rs",
    "before": "",
    "after": "#[cfg(test)]\nmod tests {\n    use super::*;\n\n    fn writable_profile() -> Profile { Profile::new(\"work\", Access::Writable) }\n\n    #[test]\n    fn writable_profile_can_be_renamed() {\n        let mut profile = writable_profile();\n        profile.rename(\"play\").unwrap();\n        assert_eq!(profile.name(), \"play\");\n    }\n}"
  },
  {
    "name": "path-attribute-test-module",
    "ruleId": "functions-and-tests-pre-mvp-test-placement",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "#[cfg(test)]\n#[path = \"fixtures/catalog_tests.rs\"]\nmod tests;"
  },
  {
    "name": "delete-separate-test-target",
    "ruleId": "functions-and-tests-pre-mvp-test-placement",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/domain/tests/profile_roundtrip.rs",
    "before": "#[test]\nfn profile_roundtrip() { assert_eq!(Profile::new(\"work\").name(), \"work\"); }\n",
    "after": ""
  },
  {
    "name": "private-container-layout-test-violation",
    "ruleId": "functions-and-tests-test-public-behavior",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/catalog.rs",
    "before": "",
    "after": "#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn new_catalog_reserves_exactly_sixteen_slots() {\n        let catalog = Catalog::new();\n        assert_eq!(catalog.entries.capacity(), 16);\n    }\n}"
  },
  {
    "name": "private-container-layout-test-compliant",
    "ruleId": "functions-and-tests-test-public-behavior",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/catalog.rs",
    "before": "",
    "after": "#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn new_catalog_has_no_selected_profile() {\n        assert_eq!(Catalog::new().selected_profile(), None);\n    }\n}"
  },
  {
    "name": "dependency-collection-suite-violation",
    "ruleId": "functions-and-tests-test-public-behavior",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/catalog.rs",
    "before": "",
    "after": "#[cfg(test)]\nmod tests {\n    use std::collections::BTreeMap;\n    #[test]\n    fn btree_orders_all_insertion_permutations() {\n        for order in [[3, 1, 2], [2, 3, 1], [1, 2, 3], [3, 2, 1]] {\n            let map: BTreeMap<_, _> = order.into_iter().map(|key| (key, key)).collect();\n            assert_eq!(map.keys().copied().collect::<Vec<_>>(), [1, 2, 3]);\n        }\n    }\n}"
  },
  {
    "name": "dependency-collection-suite-compliant",
    "ruleId": "functions-and-tests-test-public-behavior",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/catalog.rs",
    "before": "",
    "after": "#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn catalog_export_orders_ids_for_reproducible_digests() {\n        let catalog = Catalog::from_profiles([profile(3), profile(1)]);\n        assert_eq!(catalog.export().ids(), [1, 3]);\n    }\n}"
  },
  {
    "name": "dependency-toml-syntax-suite-violation",
    "ruleId": "functions-and-tests-test-public-behavior",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/settings/src/parse_settings.rs",
    "before": "",
    "after": "#[cfg(test)]\nmod tests {\n    #[test]\n    fn toml_supports_numeric_spellings() {\n        for input in [\"n = 1\", \"n = 0x01\", \"n = 0o1\", \"n = 0b1\", \"n = +1\"] {\n            let value: toml::Value = toml::from_str(input).unwrap();\n            assert_eq!(value[\"n\"].as_integer(), Some(1));\n        }\n    }\n}"
  },
  {
    "name": "dependency-toml-syntax-suite-compliant",
    "ruleId": "functions-and-tests-test-public-behavior",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/settings/src/parse_settings.rs",
    "before": "",
    "after": "#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn settings_reject_nonpositive_history_limit() {\n        let error = parse_settings(\"history_limit = 0\").unwrap_err();\n        assert!(error.contains::<InvalidHistoryLimit>());\n    }\n}"
  },
  {
    "name": "domain-unchecked-string-violation",
    "ruleId": "unsafe-rust-unsafe-layer-confinement",
    "split": "train",
    "expectedViolation": true,
    "path": "src/domain/src/profiles/name.rs",
    "before": "",
    "after": "pub fn profile_name(bytes: &[u8]) -> &str {\n    unsafe { std::str::from_utf8_unchecked(bytes) }\n}"
  },
  {
    "name": "domain-unchecked-string-compliant",
    "ruleId": "unsafe-rust-unsafe-layer-confinement",
    "split": "train",
    "expectedViolation": false,
    "path": "src/domain/src/profiles/name.rs",
    "before": "",
    "after": "pub fn profile_name(bytes: &[u8]) -> rootcause::Result<&str, InvalidProfileName> {\n    std::str::from_utf8(bytes).context(InvalidProfileName)\n}"
  },
  {
    "name": "presentation-ffi-call",
    "ruleId": "unsafe-rust-unsafe-layer-confinement",
    "split": "train",
    "expectedViolation": true,
    "path": "src/presentation/cli/src/process.rs",
    "before": "",
    "after": "pub fn process_id() -> u32 {\n    unsafe { GetCurrentProcessId() }\n}"
  },
  {
    "name": "dedicated-infrastructure-ffi-wrapper",
    "ruleId": "unsafe-rust-unsafe-layer-confinement",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/environment/src/ffi/process.rs",
    "before": "",
    "after": "pub fn process_id() -> u32 {\n    // SAFETY: GetCurrentProcessId takes no pointers, has no caller preconditions,\n    // and returns an identifier without transferring ownership of a handle.\n    unsafe { GetCurrentProcessId() }\n}"
  },
  {
    "name": "application-unchecked-index-violation",
    "ruleId": "unsafe-rust-unsafe-layer-confinement",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/first_revision.rs",
    "before": "",
    "after": "pub fn first_revision(revisions: &[Revision]) -> Revision {\n    unsafe { *revisions.get_unchecked(0) }\n}"
  },
  {
    "name": "application-unchecked-index-compliant",
    "ruleId": "unsafe-rust-unsafe-layer-confinement",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/first_revision.rs",
    "before": "",
    "after": "pub fn first_revision(revisions: &[Revision]) -> Option<Revision> {\n    revisions.first().copied()\n}"
  },
  {
    "name": "missing-pointer-proof-violation",
    "ruleId": "unsafe-rust-safety-proofs",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/environment/src/ffi/revision.rs",
    "before": "",
    "after": "fn read_revision(revision: &u32) -> u32 {\n    unsafe { std::ptr::read(revision) }\n}"
  },
  {
    "name": "missing-pointer-proof-compliant",
    "ruleId": "unsafe-rust-safety-proofs",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/environment/src/ffi/revision.rs",
    "before": "",
    "after": "fn read_revision(revision: &u32) -> u32 {\n    // SAFETY: The shared reference is aligned, initialized, and live for this read.\n    // u32 is Copy, so reading a second value does not duplicate owned resources.\n    unsafe { std::ptr::read(revision) }\n}"
  },
  {
    "name": "unsafe-api-without-caller-contract-violation",
    "ruleId": "unsafe-rust-safety-proofs",
    "split": "train",
    "expectedViolation": true,
    "path": "src/infrastructure/environment/src/ffi/revision.rs",
    "before": "",
    "after": "pub unsafe fn copy_revision(source: *const u32) -> u32 {\n    // SAFETY: The caller provides an aligned initialized u32 valid for this read.\n    unsafe { source.read() }\n}"
  },
  {
    "name": "unsafe-api-without-caller-contract-compliant",
    "ruleId": "unsafe-rust-safety-proofs",
    "split": "train",
    "expectedViolation": false,
    "path": "src/infrastructure/environment/src/ffi/revision.rs",
    "before": "",
    "after": "/// Copies a revision from provider memory.\n///\n/// # Safety\n///\n/// `source` must point to an aligned, initialized `u32` that remains readable for\n/// this call. No other thread may mutate that memory during the read.\npub unsafe fn copy_revision(source: *const u32) -> u32 {\n    // SAFETY: The caller guarantees alignment, initialization, read access, and\n    // the absence of concurrent mutation for this read. u32 has no owned resources.\n    unsafe { source.read() }\n}"
  },
  {
    "name": "delete-immediate-safety-proof-violation",
    "ruleId": "unsafe-rust-safety-proofs",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/archive/src/ffi/bytes.rs",
    "before": "fn duplicate_bytes(bytes: &[u8]) -> Vec<u8> {\n    let mut copy = vec![0; bytes.len()];\n    // SAFETY: Both slices cover bytes.len() initialized bytes and are live for\n    // this copy. The fresh Vec allocation is disjoint from the source slice.\n    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), copy.as_mut_ptr(), bytes.len()); }\n    copy\n}",
    "after": "fn duplicate_bytes(bytes: &[u8]) -> Vec<u8> {\n    let mut copy = vec![0; bytes.len()];\n    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), copy.as_mut_ptr(), bytes.len()); }\n    copy\n}"
  },
  {
    "name": "delete-immediate-safety-proof-compliant",
    "ruleId": "unsafe-rust-safety-proofs",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/archive/src/ffi/bytes.rs",
    "before": "fn duplicate_bytes(bytes: &[u8]) -> Vec<u8> {\n    let mut copy = vec![0; bytes.len()];\n    // SAFETY: Both slices cover bytes.len() initialized bytes and are live for\n    // this copy. The fresh Vec allocation is disjoint from the source slice.\n    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), copy.as_mut_ptr(), bytes.len()); }\n    copy\n}",
    "after": "fn duplicate_bytes(bytes: &[u8]) -> Vec<u8> {\n    let mut copy = vec![0; bytes.len()];\n    // SAFETY: Both slices cover bytes.len() initialized bytes and are live for\n    // this copy. The fresh Vec allocation is disjoint from the source slice.\n    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), copy.as_mut_ptr(), bytes.len()); }\n\n    copy\n}"
  },
  {
    "name": "panic-expected-cache-miss-violation",
    "ruleId": "linux-review-principles-retained-language-neutral-review-priorities",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn cached_catalog(cache: &Cache) -> Catalog {\n    // A cache miss is normal on first launch; skip recovery to save branches.\n    cache.load().unwrap()\n}"
  },
  {
    "name": "panic-expected-cache-miss-compliant",
    "ruleId": "linux-review-principles-retained-language-neutral-review-priorities",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn cached_catalog(cache: &Cache) -> Result<Catalog> {\n    match cache.load() {\n        Ok(catalog) => Ok(catalog),\n        Err(error) if error.contains::<CacheMissing>() => rebuild_catalog(),\n        Err(error) => Err(error),\n    }\n}"
  },
  {
    "name": "ignore-durable-write-failure-violation",
    "ruleId": "linux-review-principles-retained-language-neutral-review-priorities",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn update_selection(id: ProfileId) -> Result<()> {\n    // Durability errors are possible, but reporting them lengthens this path.\n    let _ = persist_selection(id);\n    Ok(())\n}"
  },
  {
    "name": "ignore-durable-write-failure-compliant",
    "ruleId": "linux-review-principles-retained-language-neutral-review-priorities",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn update_selection(id: ProfileId) -> Result<()> {\n    persist_selection(id)?;\n\n    // Telemetry is explicitly best-effort and must not undo a durable selection.\n    if let Err(error) = record_selection_metric(id) { tracing::debug!(?error, \"metric unavailable\"); }\n    Ok(())\n}"
  },
  {
    "name": "replace-recovery-with-default-violation",
    "ruleId": "linux-review-principles-retained-language-neutral-review-priorities",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn load_preferences() -> Preferences {\n    // Malformed user files should be reported, but a default keeps this short.\n    read_preferences().unwrap_or_default()\n}"
  },
  {
    "name": "replace-recovery-with-default-compliant",
    "ruleId": "linux-review-principles-retained-language-neutral-review-priorities",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "fn load_preferences() -> Result<Preferences> {\n    match read_preferences() {\n        Ok(preferences) => Ok(preferences),\n        Err(error) if error.contains::<PreferencesMissing>() => Ok(Preferences::default()),\n        Err(error) => Err(error),\n    }\n}"
  },
  {
    "name": "errno-return-convention-violation",
    "ruleId": "linux-review-principles-retained-rust-conventions-over-linux-c-idioms",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub fn select_profile(id: u32) -> i32 {\n    if id == 0 { return -2; }\n    if !exists(id) { return -22; }\n    activate(id);\n    0\n}"
  },
  {
    "name": "errno-return-convention-compliant",
    "ruleId": "linux-review-principles-retained-rust-conventions-over-linux-c-idioms",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "pub fn select_profile(id: ProfileId) -> rootcause::Result<(), SelectProfileError> {\n    if !exists(id) { return Err(SelectProfileError.into_report()); }\n    activate(id);\n    Ok(())\n}"
  },
  {
    "name": "kernel-doc-item-comment-violation",
    "ruleId": "linux-review-principles-retained-rust-conventions-over-linux-c-idioms",
    "split": "train",
    "expectedViolation": true,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/**\n * rename_profile - change a profile display name\n * @profile: profile to rename\n * @name: replacement display name\n * Return: 0 on success, negative errno on failure\n */\npub fn rename_profile(profile: &mut Profile, name: &str) -> i32 {\n    profile.name = name.to_owned();\n    0\n}"
  },
  {
    "name": "kernel-doc-item-comment-compliant",
    "ruleId": "linux-review-principles-retained-rust-conventions-over-linux-c-idioms",
    "split": "train",
    "expectedViolation": false,
    "path": "src/application/src/catalog/refresh_catalog.rs",
    "before": "",
    "after": "/// Updates a profile display name without changing its stable identifier.\npub fn rename_profile(profile: &mut Profile, name: &str) {\n    profile.name = name.to_owned();\n}\n\n#[cfg(windows)]\nfn platform_separator() -> char { '\\\\' }"
  },
  {
    "name": "cleanup-label-emulation-violation",
    "ruleId": "linux-review-principles-retained-rust-conventions-over-linux-c-idioms",
    "split": "validation",
    "expectedViolation": true,
    "path": "src/infrastructure/settings/src/write_index.rs",
    "before": "",
    "after": "fn write_index() -> i32 {\n    let mut status = 0;\n    let handle = open_index();\n    'out_free: loop {\n        if handle.is_null() { status = -12; break 'out_free; }\n        if write_records(handle) != 0 { status = -5; break 'out_free; }\n        break 'out_free;\n    }\n    close_index(handle);\n    status\n}"
  },
  {
    "name": "cleanup-label-emulation-compliant",
    "ruleId": "linux-review-principles-retained-rust-conventions-over-linux-c-idioms",
    "split": "validation",
    "expectedViolation": false,
    "path": "src/infrastructure/settings/src/write_index.rs",
    "before": "",
    "after": "fn write_index() -> rootcause::Result<(), WriteIndexError> {\n    let mut file = open_index().context(WriteIndexError)?;\n    write_records(&mut file).context(WriteIndexError)\n}"
  }
];
