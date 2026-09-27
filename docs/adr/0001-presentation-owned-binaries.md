---
status: accepted
---

# Presentation-owned binaries

The repository root is a virtual Cargo workspace. Each binary-only presentation package owns its executable and composition root: `mods.exe` for the CLI and `mods-mcp.exe` for MCP. This keeps product composition at the presentation boundary, so the old top-level binary and the nested `mods mcp start` command are removed.

For the CLI-only MVP, ADR-0006 supersedes the MCP package and executable part of this decision. The virtual workspace and CLI ownership rule remain in force.
