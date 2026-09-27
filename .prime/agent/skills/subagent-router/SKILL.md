---
name: subagent-router
description: Route an orchestrator's planned subagent task with Jev to a Codex model and thinking level. Use before rlm.spawn when the user has pinned neither model nor thinking level.
---

# Jev subagent router

Immediately before an unpinned spawn, pass the actual, concise child task (not the full conversation):

```python
route = await subagent_router.run(task)
options = {"model": route.model, "thinking": route.thinking} if route.model else {}
try:
    child = await rlm.spawn(task, name="unique-child-name", **options)
except RuntimeError as error:
    if not (route.model and str(error).startswith("Requested subagent model ") and "unavailable" in str(error)):
        raise
    child = await rlm.spawn(task, name="unique-child-name")
```

The route includes `model`, `thinking`, `choice`, `confidence`, and `source`. An empty `options` mapping preserves normal session inheritance. Admission, not `find_models`, checks availability: discovery can omit a spawnable model. Retry without a model only when admission explicitly rejects the routed selector as unavailable. User-specified model and thinking choices take precedence: spawn with those choices directly instead of routing over them.

Jev classifies the task on three separate signals: reasoning depth, consequence of an error, and work pattern. Code then compares valid model × thinking pairs and selects the cheapest pair that meets a conservative capability floor. This is not a fixed model-to-thinking mapping: for example, a bounded high-impact task can choose Astra at `low`, while a hard moderate-impact diagnosis can choose Sol at `high`. Astra at `off` is excluded.

The policy uses Artificial Analysis intelligence indexes at max effort and OpenRouter GPQA Diamond and τ²-Bench Airline average API costs as *relative* model evidence. The effort factors and quality floor are explicit, uncalibrated heuristics; the benchmarks do **not** measure quality or cost for every thinking level, and API charges are **not** Pro x5 subscription usage. Benchmark data is cached for 24 hours. Do not present the result as a measured probability of success or a precise subscription cost. Calibrate with real task outcomes before relying on exact trade-offs.

The router returns only `openai-codex/*` models for children. A diffuse task signal, invalid response, unavailable benchmarks, or request failure returns `model=None` and preserves normal inheritance. `confidence` is the lowest Jev signal confidence, not the estimated quality of the selected pair. Check `source` and `choice` when diagnosing a fallback. OpenRouter is used only for Jev (`typesafe/jev-1.13`) and benchmark data, never as a child model.

The task is sent to Jev through OpenRouter. Supply a sanitized task summary if the original contains private material; do not send credentials. Keep task text under 8,000 characters. This is an explicit pre-spawn skill, not an automatic hook. Newly installed Python skills load in a new agent session (or after `/reload`).
