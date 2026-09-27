"""Route a subagent task through TypeSafe Jev before spawning it."""

from dataclasses import dataclass
import os

import httpx

from . import benchmarks, policy


MODEL_SELECTORS = {
    "gpt-6-luna": "openai-codex/gpt-6-luna",
    "gpt-6-sol": "openai-codex/gpt-6-sol",
    "gpt-6-astra": "openai-codex/gpt-6-astra",
}
JEV_MODEL = "typesafe/jev-1.13"
PINNED_JEV_MODEL = "typesafe/jev-1.13-20260917"
ENDPOINT = "https://openrouter.ai/api/v1/systemone"
MIN_SIGNAL_CONFIDENCE = 0.20
MAX_TASK_LENGTH = 8_000

QUESTIONS = {
    "depth": {
        "type": "choice",
        "instructions": "How much novel reasoning does the subagent task require? Judge the work, not the length of its prompt.",
        "criteria": {
            "direct": {
                "what": "A lookup, extraction, or mechanical check with a known procedure.",
                "not_for": "Implementation with trade-offs, unclear symptoms, or novel design.",
                "examples": ["Locate one symbol and return its path.", "Check one config value."],
            },
            "involved": {
                "what": "A bounded implementation, ordinary review, or research requiring several reasoned decisions.",
                "not_for": "A simple lookup, open-ended design, or hard diagnosis.",
                "examples": ["Fix a CLI error message and test it.", "Review a focused change."],
            },
            "open_ended": {
                "what": "Unclear requirements, hard diagnosis, architecture, or new design requiring sustained judgment.",
                "not_for": "A focused change with clear acceptance conditions.",
                "examples": ["Design a cross-platform security interface.", "Diagnose an intermittent multi-system failure."],
            },
        },
    },
    "impact": {
        "type": "choice",
        "instructions": "What is the consequence of a wrong result for the subagent task?",
        "criteria": {
            "low": "Read-only or easily reversible work with obvious verification.",
            "moderate": "Behavior or decisions change; mistakes need tests or review to catch.",
            "high": "Security, data integrity, destructive actions, or costly correctness errors.",
        },
    },
    "shape": {
        "type": "choice",
        "instructions": "Which work pattern dominates the subagent task?",
        "criteria": {
            "single_pass": "One short lookup, edit, or review with a clear endpoint.",
            "iterative": "Several different steps, tool checks, or revisions inform each other.",
            "repetitive": "Many similar units follow the same clear procedure.",
        },
    },
}


@dataclass(frozen=True)
class Route:
    """A model-and-thinking recommendation or fallback to session inheritance."""

    model: str | None
    thinking: str | None
    choice: str | None
    confidence: float | None
    source: str


def _fallback(source: str) -> Route:
    return Route(model=None, thinking=None, choice=None, confidence=None, source=source)


def _parse_choice(answer: object, labels: set[str]) -> tuple[str, float, dict[str, float]] | None:
    if not isinstance(answer, dict) or answer.get("type") != "choice":
        return None
    choice, confidence, probabilities = answer.get("choice"), answer.get("confidence"), answer.get("probabilities")
    if not isinstance(choice, str) or choice not in labels or not isinstance(probabilities, dict):
        return None
    if set(probabilities) != labels or any(
        isinstance(value, bool) or not isinstance(value, (int, float)) or not 0 <= value <= 1
        for value in probabilities.values()
    ):
        return None
    if isinstance(confidence, bool) or not isinstance(confidence, (int, float)) or not 0 <= confidence <= 1:
        return None
    if abs(sum(probabilities.values()) - 1) > 0.02 or probabilities[choice] < max(probabilities.values()):
        return None
    return choice, float(confidence), probabilities


def _parse_reply(data: object) -> dict[str, tuple[str, float, dict[str, float]]] | None:
    if not isinstance(data, dict) or data.get("model") not in (JEV_MODEL, PINNED_JEV_MODEL):
        return None
    answers = data.get("answers")
    if not isinstance(answers, dict):
        return None
    parsed = {name: _parse_choice(answers.get(name), set(question["criteria"]))
              for name, question in QUESTIONS.items()}
    if any(answer is None for answer in parsed.values()):
        return None
    return parsed


async def run(task: str) -> Route:
    """Select a Codex model and thinking level using Jev's task judgments.

    If Jev or benchmarks fail, inherit session settings. Model availability is
    checked by ``rlm.spawn`` admission, not by model discovery.
    """
    if not isinstance(task, str) or not task.strip() or len(task) > MAX_TASK_LENGTH:
        return _fallback("invalid_task")
    key = os.environ.get("OPENROUTER_API_KEY")
    if not key:
        return _fallback("missing_api_key")

    try:
        async with httpx.AsyncClient(timeout=10) as client:
            try:
                scores = await benchmarks.load(client, key)
            except (httpx.HTTPError, ValueError, KeyError, TypeError):
                return _fallback("benchmarks_unavailable")
            payload = {
                "model": JEV_MODEL,
                "state": {"subagent_task": task},
                "questions": QUESTIONS,
            }
            response = await client.post(ENDPOINT, json=payload, headers={"Authorization": f"Bearer {key}"})
            response.raise_for_status()
            answers = _parse_reply(response.json())
    except (httpx.HTTPError, ValueError, KeyError, TypeError):
        return _fallback("classification_failed")
    if answers is None:
        return _fallback("invalid_response")
    confidence = min(answer[1] for answer in answers.values())
    if confidence < MIN_SIGNAL_CONFIDENCE:
        return Route(None, None, None, confidence, "uncertain")
    try:
        model, thinking = policy.select_pair(
            {name: answer[2] for name, answer in answers.items()}, scores
        )
    except (KeyError, TypeError, ValueError, ZeroDivisionError):
        return _fallback("policy_failed")
    return Route(MODEL_SELECTORS[model], thinking, model, confidence, "jev_policy")
