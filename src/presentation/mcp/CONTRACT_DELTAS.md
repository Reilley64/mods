# MCP owner-approved contract deltas

`src/tool-schemas.json` is the #18 reference fixture with these owner-approved MVP changes:

- Rate limiting is omitted. Busy admission still uses the nonqueueing environment semaphore.
- `environment_invalid` omits `details` for every tool. Five read-tool branches no longer require the obsolete `recovery` phase. No recovery is implied.
- Complete valid UTF-8 child text is returned unchanged. It can contain paths or secrets printed by the child. Binary streams retain complete byte counts and SHA-256, without text.
- Existing execution warnings without the provenance required by the historical structured union remain visible as MCP text content. The structured `warnings` array is not exhaustive. No line numbers, positions, files, or problems are fabricated.

All other input/output wire shapes remain unchanged. Generated rmcp input schemas are checked against the reference input fixtures.

The owner selected structured mutation success during #32 repair. This supersedes the earlier bodyless-success policy. All eight tools advertise output schemas and explicit annotations. Config-set returns stored/effective binding facts and warnings. Install returns the approved plan and committed state only after publication succeeds. Its conflict view uses the scanned snapshot with the actual committed enablement, not the hypothetical-enabled preview. No post-commit query or cancellation can turn a completed mutation into failure.

Progress uses a separate typed observational port. Application phases and archive hash/extraction checkpoints reach the MCP reporter without a frequency cap. Request cancellation remains a direct `CancellationToken`.

The rmcp 3.3 writer has no commitment hook, and its reader privately emits invalid-request replies. The narrow outgoing Sink checks cancellation synchronously at serialized `start_send`; a bounded Tokio duplex relay preserves rmcp's reader and routes those protocol replies through that same writer. The existing lifecycle tracker owns the relay. No protocol parser was replaced.
