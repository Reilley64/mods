---
status: accepted
---

# Use an aggregate product release

The repository has one aggregate `mods` product release in addition to the independently versioned Cargo packages. The aggregate release owns the public `vX.Y.Z` tag, GitHub Release, root changelog, and product release version. Its version reflects repository-wide release-worthy changes and does not need to match either presentation package version.

CLI and MCP are equal presentation packages. Each keeps its own version and component-prefixed bookkeeping tag, and neither owns the aggregate changelog or a separate GitHub Release. The aggregate release exists only in release metadata, so the repository root remains a virtual Cargo workspace and presentation packages continue to own their binaries and composition roots.
