---
status: accepted
---

# Presentation-owned binaries

The repository root is a virtual Cargo workspace. Each binary-only presentation package owns its executable and composition root: `mods.exe` for the CLI and `mods-mcp.exe` for MCP. This keeps product composition at the presentation boundary, so the old top-level binary and the nested `mods mcp start` command are removed.
