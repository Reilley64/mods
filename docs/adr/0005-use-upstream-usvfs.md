---
status: accepted
---

# Use upstream usvfs

Use pinned, unmodified upstream usvfs through rust-bindgen and a small safe Rust adapter instead of maintaining a custom filesystem mutation broker and native fork. The owner accepted upstream overlay behavior, including physical writes/deletes, absent copy-on-write and durable execution Tombstones, non-opaque namespaces, and non-fail-closed descendant injection, to keep the integration small.

Rust retains Mod Priority, Output Target selection, Profile State configuration and process supervision. Test only mods-owned behavior and integration seams; do not duplicate upstream tests. The replacement contracts are [#30](https://github.com/Reilley64/mods/issues/30) and [#31](https://github.com/Reilley64/mods/issues/31); the [evaluation](../research/issue-30-upstream-usvfs.md) records evidence and limitations. The former custom protocols, exact handshake and transactional application-write requirements are superseded, not deferred implementation obligations.
