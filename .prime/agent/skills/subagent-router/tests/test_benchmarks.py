import asyncio
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import httpx

from subagent_router import benchmarks


MODELS = benchmarks.MODELS


def response_for(request):
    params = request.url.params
    if params.get("source") == "artificial-analysis":
        data = [{"model_permaslug": "openai/" + name + "-20260922", "display_name": name + " (max)",
                 "intelligence_index": score} for name, score in zip(MODELS, (37.3, 47.5, 52.7))]
        return httpx.Response(200, json={"data": data, "meta": {"as_of": "2026-09-27T00:00:00Z"}})
    benchmark = params.get("benchmark_type")
    if benchmark not in ("gpqa_diamond", "tau_bench_verified_airline"):
        return httpx.Response(400)
    data = [{"model_permaslug": "openai/" + name + "-20260922", "accuracy": score,
             "avg_cost_per_task": 0.03, "total_tasks": 50, "last_run_timestamp": "2026-09-27T00:00:00Z"}
            for name, score in zip(MODELS, (0.8, 0.9, 0.95))]
    data.append({"model_permaslug": "openai/gpt-6-luna-pro-20260922", "accuracy": 1.0,
                 "avg_cost_per_task": 0.05, "total_tasks": 50})
    return httpx.Response(200, json={"data": data})


class BenchmarkTests(unittest.IsolatedAsyncioTestCase):
    async def test_fetches_three_comparable_sources_then_reuses_disk_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            calls = []
            def respond(request):
                calls.append(request)
                return response_for(request)
            with patch.object(benchmarks, "cache_path", return_value=Path(directory) / "scores.json"):
                async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as client:
                    scores, simultaneous = await asyncio.gather(
                        benchmarks.load(client, "test-only"), benchmarks.load(client, "test-only")
                    )
                    again = await benchmarks.load(client, "test-only")
            self.assertEqual(scores, simultaneous)
            self.assertEqual(len(calls), 3)
            self.assertEqual(scores, again)
            self.assertEqual(scores["gpt-6-luna"]["intelligence"]["index_at_max_effort"], 37.3)
            self.assertEqual(scores["gpt-6-astra"]["gpqa_diamond"]["accuracy"], 0.95)
            self.assertEqual(scores["gpt-6-luna"]["tau_bench_airline"]["sample_count"], 50)
            self.assertEqual(calls[0].headers["authorization"], "Bearer test-only")

    async def test_missing_comparable_model_score_fails_closed(self):
        responses = {
            "intelligence": response_for(httpx.Request("GET", benchmarks.URL, params={"source": "artificial-analysis"})).json(),
            "gpqa_diamond": response_for(httpx.Request("GET", benchmarks.URL, params={"benchmark_type": "gpqa_diamond"})).json(),
            "tau_bench_airline": response_for(httpx.Request("GET", benchmarks.URL, params={"benchmark_type": "tau_bench_verified_airline"})).json(),
        }
        responses["intelligence"]["data"].pop()
        with self.assertRaises(ValueError):
            benchmarks._summarize(responses)


if __name__ == "__main__":
    unittest.main()
