# Codex discovery version

Prime Agent 0.9.6 sends `client_version=0.153.4` when fetching the Codex model catalog. That version omits GPT-6 Sol and Luna for this account. This project extension rewrites only the Codex models request to `0.156.1` before model discovery. It does not change response requests or use OpenRouter for agent models.

The extension is idempotent when loaded by child sessions in the same process. Remove it once Prime Agent ships the upstream version fix. See [upstream discussion #2544](https://github.com/PrimeIntellect-ai/prime-agent/discussions/2544).

Use `/reload` or restart the agent after adding it. A catalog cached before reload can remain stale for up to five minutes; a new process avoids that cache.
