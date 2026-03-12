"""Composable objective functions for adversarial scenario evaluation.

Each objective takes a list of result dicts (from ``batch_run``) and returns
a float score.  Objectives can be combined via ``multi_objective`` for
Pareto-front searches.

Usage::

    from kubesim.objectives import cost_efficiency, availability, multi_objective

    obj = cost_efficiency
    score = obj(results)

    # Multi-objective: returns tuple of scores
    multi = multi_objective(cost_efficiency, availability)
    scores = multi(results)
"""

from __future__ import annotations

from typing import Callable

ObjectiveFn = Callable[[list[dict]], float]


def cost_efficiency(results: list[dict]) -> float:
    """Total cost per running pod (lower = more efficient)."""
    cost = sum(r.get("total_cost_per_hour", 0) for r in results)
    running = sum(r.get("running_pods", 0) for r in results)
    return cost / running if running > 0 else float("inf")


def availability(results: list[dict]) -> float:
    """Fraction of pods running vs total requested (higher = better)."""
    running = sum(r.get("running_pods", 0) for r in results)
    pending = sum(r.get("pending_pods", 0) for r in results)
    total = running + pending
    return running / total if total > 0 else 1.0


def consolidation_waste(results: list[dict]) -> float:
    """1 - (allocated_cpu / allocatable_cpu). Higher = more waste."""
    allocated = sum(r.get("total_allocated_cpu", 0) for r in results)
    allocatable = sum(r.get("total_allocatable_cpu", 0) for r in results)
    return 1.0 - (allocated / allocatable) if allocatable > 0 else 1.0


def disruption_rate(results: list[dict]) -> float:
    """Fraction of pods evicted (higher = more disruption)."""
    evicted = sum(r.get("pods_evicted", 0) for r in results)
    total = sum(r.get("running_pods", 0) + r.get("pending_pods", 0) for r in results)
    return evicted / total if total > 0 else 0.0


def scheduling_failure_rate(results: list[dict]) -> float:
    """Fraction of pods stuck pending (higher = worse scheduling)."""
    pending = sum(r.get("pending_pods", 0) for r in results)
    total = sum(r.get("running_pods", 0) + r.get("pending_pods", 0) for r in results)
    return pending / total if total > 0 else 0.0


def entropy_deviation(results: list[dict]) -> float:
    """|normalized_entropy - 1.0| — distance from uniform distribution."""
    vals = [r.get("normalized_entropy", 1.0) for r in results]
    avg = sum(vals) / len(vals) if vals else 1.0
    return abs(avg - 1.0)


def pareto_violation(results_a: list[dict], results_b: list[dict]) -> float:
    """Score where strategy A loses on BOTH cost AND availability vs B.

    Returns positive value when A is dominated (worse cost AND worse availability).
    Zero when A wins on at least one dimension.
    """
    cost_a, cost_b = cost_efficiency(results_a), cost_efficiency(results_b)
    avail_a, avail_b = availability(results_a), availability(results_b)
    # A is dominated if it has higher cost AND lower availability
    if cost_a > cost_b and avail_a < avail_b:
        return (cost_a - cost_b) + (avail_b - avail_a)
    return 0.0


def multi_objective(*objectives: ObjectiveFn) -> Callable[[list[dict]], tuple[float, ...]]:
    """Combine multiple objectives into a single callable returning a score tuple."""
    def _eval(results: list[dict]) -> tuple[float, ...]:
        return tuple(obj(results) for obj in objectives)
    return _eval


# Registry for name-based lookup
OBJECTIVES: dict[str, ObjectiveFn] = {
    "cost_efficiency": cost_efficiency,
    "availability": availability,
    "consolidation_waste": consolidation_waste,
    "disruption_rate": disruption_rate,
    "scheduling_failure_rate": scheduling_failure_rate,
    "entropy_deviation": entropy_deviation,
}
