---
status: accepted
---

# Defer the MCP Presentation until after the CLI-only MVP

The MVP source and virtual Cargo workspace contain the CLI Presentation and shared application/infrastructure behavior, but not the MCP Presentation. Its executable, package wiring, release metadata and initial Winget alias are not part of the MVP. This deliberately supersedes the two-presentation MVP delivery portions of ADR-0001 and ADR-0004 and the MCP diagnostics clause of ADR-0003, without changing their virtual-workspace ownership rule or the single aggregate `vX.Y.Z` product release.

The full pre-deferral work remains on `feat/post-mvp-mcp` at `96d837817a342cb9e9b562236148b332043edb14`, and #105 owns its selective return after MVP. Removing only its binary from the ZIP would leave a live workspace target and misleading MVP checks; reversing shared application use cases would instead put CLI behavior at risk. The old two-binary ZIP remains historical evidence. A new committed CLI-only ZIP and its own Windows checks are required before claiming MVP package acceptance. Stable and Winget gates stay disabled until their independent acceptance criteria pass.
