"""Deterministic cost/quality policy for valid model-and-effort pairs."""

from math import sqrt
from typing import Mapping


EFFORT_QUALITY = {"off": 0.70, "low": 0.82, "medium": 0.92, "high": 1.0}
EFFORT_COST = {"off": 0.45, "low": 0.65, "medium": 0.80, "high": 1.0}
MODEL_ORDER = ("gpt-6-luna", "gpt-6-sol", "gpt-6-astra")


def upper_quantile(probabilities: Mapping[str, float], order: tuple[str, ...]) -> str:
    """Choose a cautious 80th-percentile ordinal category, not a numeric Jev score."""
    cumulative = 0.0
    for label in order:
        cumulative += probabilities[label]
        if cumulative >= 0.8:
            return label
    return order[-1]


def select_pair(
    probabilities: Mapping[str, Mapping[str, float]],
    benchmarks: Mapping[str, Mapping[str, object]],
) -> tuple[str, str]:
    """Pick the least costly sufficient pair; quality and effort factors are heuristics.

    Benchmark index and per-task API costs compare models only at benchmark
    settings. Effort factors are policy assumptions, not measured per-level data.
    """
    depth = upper_quantile(probabilities["depth"], ("direct", "involved", "open_ended"))
    impact_probabilities = probabilities["impact"]
    # Require positive evidence before paying for the highest-stakes model.
    # Otherwise use the cautious ordinal percentile among low/moderate risk.
    if impact_probabilities["high"] >= 0.40:
        impact = "high"
    else:
        total = impact_probabilities["low"] + impact_probabilities["moderate"]
        if total <= 0:
            raise ValueError("impact probabilities are invalid")
        impact = upper_quantile(
            {label: impact_probabilities[label] / total for label in ("low", "moderate")},
            ("low", "moderate"),
        )
    shape = probabilities["shape"]
    work_pattern = "repetitive" if shape["repetitive"] >= 0.65 and shape["iterative"] < 0.25 else (
        "iterative" if shape["iterative"] >= 0.25 else "single_pass"
    )

    requirement = (
        0.50 + 0.135 * ("direct", "involved", "open_ended").index(depth)
        + 0.075 * ("low", "moderate", "high").index(impact)
        + {"single_pass": 0.0, "iterative": 0.05, "repetitive": -0.035}[work_pattern]
    )
    max_index = max(benchmarks[model]["intelligence"]["index_at_max_effort"] for model in MODEL_ORDER)
    if max_index <= 0:
        raise ValueError("benchmark intelligence index must be positive")

    candidates: list[tuple[float, float, str, str]] = []
    for model in MODEL_ORDER:
        if impact == "high" and model != "gpt-6-astra":
            continue
        data = benchmarks[model]
        quality = data["intelligence"]["index_at_max_effort"] / max_index
        api_cost = sqrt(
            data["gpqa_diamond"]["average_openrouter_usd_per_task"]
            * data["tau_bench_airline"]["average_openrouter_usd_per_task"]
        )
        for effort, quality_factor in EFFORT_QUALITY.items():
            if model == "gpt-6-astra" and effort == "off":
                continue
            effective_quality = quality * quality_factor
            if effective_quality + 1e-9 >= requirement:
                candidates.append((api_cost * EFFORT_COST[effort], -effective_quality, model, effort))

    if not candidates:
        raise ValueError("no pair meets the policy quality floor")
    _, _, model, effort = min(candidates)
    return model, effort
