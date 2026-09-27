"""Compact, cached OpenRouter benchmark evidence for the routed Codex models."""

import asyncio
import json
import math
import os
from pathlib import Path
import re
import tempfile
import time

import httpx


URL = "https://openrouter.ai/api/v1/benchmarks"
MODELS = ("gpt-6-luna", "gpt-6-sol", "gpt-6-astra")
CACHE_SECONDS = 24 * 60 * 60
CACHE_VERSION = 3
_cache_lock = asyncio.Lock()
SOURCES = (
    ("intelligence", {"source": "artificial-analysis", "task_type": "intelligence"}),
    ("gpqa_diamond", {"source": "openrouter", "benchmark_type": "gpqa_diamond"}),
    ("tau_bench_airline", {"source": "openrouter", "benchmark_type": "tau_bench_verified_airline"}),
)


def cache_path() -> Path:
    base = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    return base / "subagent-router" / "openrouter-benchmarks.json"


def _model_for(slug: object) -> str | None:
    if not isinstance(slug, str):
        return None
    match = re.fullmatch(r"openai/(gpt-6-(?:luna|sol|astra))(?:-\d{8})?", slug)
    return match.group(1) if match else None


def _number(value: object) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
        raise ValueError("invalid benchmark number")
    return float(value)


def _summarize(responses: dict[str, dict]) -> dict[str, dict]:
    summary = {model: {} for model in MODELS}
    for kind, body in responses.items():
        rows = body.get("data")
        if not isinstance(rows, list):
            raise ValueError("invalid benchmark response")
        for row in rows:
            if not isinstance(row, dict) or (model := _model_for(row.get("model_permaslug"))) is None:
                continue
            if kind == "intelligence":
                if "(max)" not in str(row.get("display_name", "")):
                    continue
                summary[model][kind] = {
                    "index_at_max_effort": _number(row.get("intelligence_index")),
                    "source": "Artificial Analysis via OpenRouter",
                    "as_of": body.get("meta", {}).get("as_of"),
                }
            else:
                summary[model][kind] = {
                    "accuracy": _number(row.get("accuracy")),
                    "average_openrouter_usd_per_task": _number(row.get("avg_cost_per_task")),
                    "sample_count": int(_number(row.get("total_tasks"))),
                    "last_run": row.get("last_run_timestamp"),
                }
    if any(set(scores) != {kind for kind, _ in SOURCES} for scores in summary.values()):
        raise ValueError("missing comparable benchmark data")
    return summary


async def load(client: httpx.AsyncClient, api_key: str) -> dict[str, dict]:
    """Return a 24-hour cached snapshot; raise on missing or invalid data."""
    async with _cache_lock:
        return await _load_cached(client, api_key)


async def _load_cached(client: httpx.AsyncClient, api_key: str) -> dict[str, dict]:
    path = cache_path()
    try:
        cache = json.loads(path.read_text())
        if (cache.get("version") == CACHE_VERSION and time.time() - cache["fetched_at"] < CACHE_SECONDS
            and cache["fetched_at"] <= time.time()
            and set(cache["models"]) == set(MODELS)
            and all(set(cache["models"][model]) == {kind for kind, _ in SOURCES} for model in MODELS)):
            return cache["models"]
    except (OSError, ValueError, KeyError, TypeError, AttributeError):
        pass

    responses = {}
    for kind, params in SOURCES:
        response = await client.get(URL, params=params, headers={"Authorization": f"Bearer {api_key}"})
        response.raise_for_status()
        responses[kind] = response.json()
    summary = _summarize(responses)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as tmp:
            json.dump({"version": CACHE_VERSION, "fetched_at": time.time(), "models": summary}, tmp)
            temporary = Path(tmp.name)
        temporary.replace(path)
    except OSError:
        pass  # The live snapshot is still valid if the cache is not writable.
    return summary
