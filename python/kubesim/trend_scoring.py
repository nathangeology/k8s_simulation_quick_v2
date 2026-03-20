"""Trend scoring functions for ConsolidateWhen threshold sweep analysis.

Evaluates curve-shape properties across a threshold sweep to find adversarial
scenarios where the cost-disruption tradeoff is pathological.

Three scoring functions:
1. max_slope_sensitivity — sharp knee detection
2. pareto_area_divergence — non-convexity detection
3. composite_trend_score — weighted combination (recommended)

See docs/adversarial-consolidate-when-proposal.md for design rationale.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class ThresholdResult:
    """Result of running a scenario at a specific threshold or reference policy."""

    threshold: float | None  # None for WhenEmpty/WhenEmptyOrUnderutilized
    policy: str
    cost: float       # total_cost_per_hour
    disruption: float  # pods_evicted / total_pods
    availability: float
    node_count: float


def max_slope_sensitivity(curve: list[ThresholdResult]) -> float:
    """Max normalized slope across adjacent threshold pairs.

    Finds scenarios with sharp knees where a small threshold change
    causes a large outcome change.
    """
    threshold_pts = sorted(
        [c for c in curve if c.threshold is not None], key=lambda c: c.threshold
    )
    if len(threshold_pts) < 2:
        return 0.0

    max_slope = 0.0
    for i in range(len(threshold_pts) - 1):
        dt = threshold_pts[i + 1].threshold - threshold_pts[i].threshold
        if dt <= 0:
            continue
        cost_slope = abs(threshold_pts[i + 1].cost - threshold_pts[i].cost) / dt
        disr_slope = abs(threshold_pts[i + 1].disruption - threshold_pts[i].disruption) / dt
        max_slope = max(max_slope, cost_slope, disr_slope)

    cost_range = max(c.cost for c in curve) - min(c.cost for c in curve)
    disr_range = max(c.disruption for c in curve) - min(c.disruption for c in curve)
    denom = max(cost_range + disr_range, 1e-6)
    return max_slope / denom


def pareto_area_divergence(
    curve: list[ThresholdResult],
    ref_empty: ThresholdResult,
    ref_underutilized: ThresholdResult,
) -> float:
    """Area between the threshold curve and the reference line.

    Large area means the threshold-based policy explores a wide
    cost-disruption tradeoff space. Non-convex regions (curve above
    reference) indicate pathological behavior and are weighted 2x.
    """
    ref_points = sorted([ref_empty, ref_underutilized], key=lambda p: p.cost)
    pts = sorted(
        [c for c in curve if c.threshold is not None], key=lambda c: c.cost
    )
    if len(pts) < 2:
        return 0.0

    def _interp_ref(cost: float) -> float:
        if ref_points[1].cost == ref_points[0].cost:
            return (ref_points[0].disruption + ref_points[1].disruption) / 2
        t = (cost - ref_points[0].cost) / (ref_points[1].cost - ref_points[0].cost)
        return ref_points[0].disruption + t * (ref_points[1].disruption - ref_points[0].disruption)

    total_area = 0.0
    non_convex_area = 0.0
    for i in range(len(pts) - 1):
        dx = pts[i + 1].cost - pts[i].cost
        avg_disruption = (pts[i].disruption + pts[i + 1].disruption) / 2
        ref_disruption = _interp_ref((pts[i].cost + pts[i + 1].cost) / 2)
        delta = avg_disruption - ref_disruption
        total_area += abs(delta) * abs(dx)
        if delta > 0:
            non_convex_area += delta * abs(dx)

    return total_area + non_convex_area


def composite_trend_score(
    curve: list[ThresholdResult],
    ref_empty: ThresholdResult,
    ref_underutilized: ThresholdResult,
) -> float:
    """Weighted combination of curve pathology indicators.

    Components:
    1. knee_sharpness: max normalized slope across adjacent thresholds
    2. miscalibration: WhenCostJustifiesDisruption(1.0) worse than WhenEmpty
    3. non_monotonicity: threshold intervals where both cost and disruption increase
    4. range_magnitude: total spread relative to reference policies
    """
    threshold_pts = sorted(
        [c for c in curve if c.threshold is not None], key=lambda c: c.threshold
    )

    # 1. Knee sharpness
    slopes = []
    for i in range(len(threshold_pts) - 1):
        dt = threshold_pts[i + 1].threshold - threshold_pts[i].threshold
        if dt <= 0:
            continue
        cost_slope = abs(threshold_pts[i + 1].cost - threshold_pts[i].cost) / dt
        disr_slope = abs(threshold_pts[i + 1].disruption - threshold_pts[i].disruption) / dt
        slopes.append(max(cost_slope, disr_slope))
    cost_range = max(c.cost for c in curve) - min(c.cost for c in curve)
    disr_range = max(c.disruption for c in curve) - min(c.disruption for c in curve)
    norm = max(cost_range + disr_range, 1e-6)
    knee = max(slopes) / norm if slopes else 0.0

    # 2. Miscalibration: WhenCostJustifiesDisruption(1.0) vs WhenEmpty
    t1 = next((c for c in threshold_pts if c.threshold == 1.0), None)
    miscal = 0.0
    if t1 and ref_empty.cost > 0:
        savings = (ref_empty.cost - t1.cost) / ref_empty.cost
        if savings < 0:
            miscal = abs(savings)

    # 3. Non-monotonicity
    non_mono = 0
    for i in range(len(threshold_pts) - 1):
        if threshold_pts[i + 1].threshold > threshold_pts[i].threshold:
            if (threshold_pts[i + 1].cost > threshold_pts[i].cost and
                    threshold_pts[i + 1].disruption > threshold_pts[i].disruption):
                non_mono += 1
    non_mono_frac = non_mono / max(len(threshold_pts) - 1, 1)

    # 4. Range magnitude
    ref_cost = max(ref_empty.cost, ref_underutilized.cost, 1e-6)
    ref_disr = max(ref_empty.disruption, ref_underutilized.disruption, 1e-6)
    range_score = (cost_range / ref_cost) + (disr_range / ref_disr)

    return (
        0.30 * knee
        + 0.25 * miscal
        + 0.25 * non_mono_frac
        + 0.20 * range_score
    )
