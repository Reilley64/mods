import json
import os
import unittest
from unittest.mock import AsyncMock, patch

import httpx

import subagent_router as router


SCORES = {
    "gpt-6-luna": {"intelligence": {"index_at_max_effort": 37.3},
                   "gpqa_diamond": {"average_openrouter_usd_per_task": 0.00313},
                   "tau_bench_airline": {"average_openrouter_usd_per_task": 0.00819}},
    "gpt-6-sol": {"intelligence": {"index_at_max_effort": 47.5},
                  "gpqa_diamond": {"average_openrouter_usd_per_task": 0.02642},
                  "tau_bench_airline": {"average_openrouter_usd_per_task": 0.1283}},
    "gpt-6-astra": {"intelligence": {"index_at_max_effort": 52.7},
                    "gpqa_diamond": {"average_openrouter_usd_per_task": 0.11967},
                    "tau_bench_airline": {"average_openrouter_usd_per_task": 0.6524}},
}


def choice(label, labels, confidence=0.9):
    probabilities = {key: (1 - confidence) / (len(labels) - 1) for key in labels}
    probabilities[label] = confidence
    return {"type": "choice", "choice": label, "confidence": confidence, "probabilities": probabilities}


def reply(depth="direct", impact="low", shape="single_pass", confidence=0.9, model=router.JEV_MODEL):
    values = {"depth": depth, "impact": impact, "shape": shape}
    return {"model": model, "answers": {
        name: choice(label, router.QUESTIONS[name]["criteria"], confidence)
        for name, label in values.items()
    }}


class RouterTests(unittest.IsolatedAsyncioTestCase):
    async def call_with_reply(self, body, task="Locate one Rust symbol and report its path"):
        requests = []

        def respond(request):
            requests.append(request)
            return httpx.Response(200, json=body)

        original = httpx.AsyncClient
        def mocked_client(*args, **kwargs):
            return original(transport=httpx.MockTransport(respond), *args, **kwargs)

        with patch.dict(os.environ, {"OPENROUTER_API_KEY": "test-only"}), patch.object(
            router.httpx, "AsyncClient", mocked_client
        ), patch.object(router.benchmarks, "load", AsyncMock(return_value=SCORES)):
            result = await router.run(task)
        return result, requests

    async def test_policy_selects_pairs_across_the_matrix(self):
        cases = (
            ("direct", "low", "single_pass", "gpt-6-luna", "low"),
            ("direct", "low", "repetitive", "gpt-6-luna", "off"),
            ("involved", "moderate", "repetitive", "gpt-6-luna", "high"),
            ("involved", "moderate", "single_pass", "gpt-6-sol", "low"),
            ("involved", "moderate", "iterative", "gpt-6-sol", "medium"),
            ("open_ended", "moderate", "iterative", "gpt-6-sol", "high"),
            ("involved", "high", "single_pass", "gpt-6-astra", "low"),
            ("open_ended", "high", "single_pass", "gpt-6-astra", "medium"),
            ("open_ended", "high", "iterative", "gpt-6-astra", "high"),
        )
        for depth, impact, shape, model, effort in cases:
            with self.subTest(depth=depth, impact=impact, shape=shape):
                route, requests = await self.call_with_reply(reply(depth, impact, shape))
                self.assertEqual(route, router.Route(router.MODEL_SELECTORS[model], effort, model, 0.9, "jev_policy"))
                payload = json.loads(requests[0].content)
                self.assertEqual(payload["state"]["subagent_task"], "Locate one Rust symbol and report its path")
                self.assertEqual(set(payload["questions"]), {"depth", "impact", "shape"})
                self.assertEqual(requests[0].headers["authorization"], "Bearer test-only")

    async def test_ambiguous_impact_does_not_force_the_most_expensive_model(self):
        body = reply("open_ended", "moderate", "iterative")
        body["answers"]["impact"] = {
            "type": "choice", "choice": "moderate", "confidence": 0.46,
            "probabilities": {"low": 0.0, "moderate": 0.64, "high": 0.36},
        }
        route, _ = await self.call_with_reply(body)
        self.assertEqual((route.choice, route.thinking), ("gpt-6-sol", "high"))

    async def test_diffuse_signal_inherits(self):
        uncertain = reply()
        uncertain["answers"]["depth"] = choice("direct", router.QUESTIONS["depth"]["criteria"], 0.4)
        uncertain["answers"]["depth"]["confidence"] = 0.1
        result, _ = await self.call_with_reply(uncertain)
        self.assertEqual((result.model, result.thinking, result.source), (None, None, "uncertain"))

    async def test_invalid_responses_never_choose_model(self):
        wrong_label = reply()
        wrong_label["answers"]["depth"]["choice"] = ["direct"]
        missing_answer = reply()
        del missing_answer["answers"]["shape"]
        for bad in (reply(model="other"), missing_answer, wrong_label):
            result, _ = await self.call_with_reply(bad)
            self.assertEqual(result.source, "invalid_response")
        self.assertIsNotNone(router._parse_reply(reply(model=router.PINNED_JEV_MODEL)))

    async def test_missing_credentials_and_invalid_task_do_not_call_api(self):
        with patch.dict(os.environ, {"OPENROUTER_API_KEY": ""}):
            self.assertEqual((await router.run("task")).source, "missing_api_key")
        self.assertEqual((await router.run("x" * 8001)).source, "invalid_task")

    async def test_network_failure_falls_back(self):
        def fail(request):
            raise httpx.ConnectError("offline")
        original = httpx.AsyncClient
        with patch.dict(os.environ, {"OPENROUTER_API_KEY": "test-only"}), patch.object(
            router.httpx, "AsyncClient", lambda **kwargs: original(transport=httpx.MockTransport(fail), **kwargs)
        ), patch.object(router.benchmarks, "load", AsyncMock(return_value=SCORES)):
            result = await router.run("task")
        self.assertEqual(result.source, "classification_failed")
        self.assertIsNone(result.thinking)


if __name__ == "__main__":
    unittest.main()
