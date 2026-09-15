---
status: accepted
---

# Keep successful mutations quiet

Successful CLI mutations emit no stdout or stderr body, while warnings and errors remain visible and read-only queries still return data. MCP mutations follow the same rule inside their protocol-required success envelope. This keeps automation output actionable and avoids making presentation summaries part of the product contract.
