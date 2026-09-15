---
status: accepted
---

# Use library JSON files for MVP diagnostics

The CLI and MCP presentations use `tracing-subscriber`'s default JSON formatter with a synchronous `tracing-appender` file appender. Each enabled operation writes one UUID-named file and simple session boundary events. Appender setup failure produces one fixed warning without changing the operation result.

The MVP trusts these dependency contracts. It does not own an OpenTelemetry mapping, JSON schema, redaction, retention, producer identity, diagnostic filesystem hardening, exact deletion, asynchronous sink handling, or record limits. A later need for those guarantees requires a separate decision and tests of project-owned behavior.
