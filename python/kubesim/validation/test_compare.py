"""Synthetic-data smoke test for sigma scoring and DTW in compare.py.

Creates fake Tier 1 distribution + Tier 2 point estimates and verifies
that the fidelity scorecard rates them correctly.

Run: python -m kubesim.validation.test_compare
"""

from __future__ import annotations

import math
import random
import sys

import polars as pl

from kubesim.validation.compare import (
    Fidelity,
    compare_sigma,
    compute_dtw,
    compute_sigma_scores,
    build_scorecard,
    dtw_distance,
)

METRICS = ["total_cost_per_hour", "node_count", "running_pods", "pending_pods"]


def _make_tier1(n: int = 200, seed: int = 42) -> pl.DataFrame:
    """Fake Tier 1: N seed runs with known distribution."""
    rng = random.Random(seed)
    rows = {m: [] for m in METRICS}
    rows["node_count_over_time"] = []
    for _ in range(n):
        rows["total_cost_per_hour"].append(rng.gauss(50.0, 3.0))
        rows["node_count"].append(rng.gauss(10.0, 1.0))
        rows["running_pods"].append(rng.gauss(30.0, 2.0))
        rows["pending_pods"].append(rng.gauss(1.0, 0.5))
        # Time series: ramp up then plateau
        ts = [rng.gauss(2 + min(i, 8), 0.3) for i in range(12)]
        rows["node_count_over_time"].append(ts)
    return pl.DataFrame(rows)


def _make_tier2_close(seed: int = 99) -> pl.DataFrame:
    """Tier 2 observation within 1σ of Tier 1 means."""
    rng = random.Random(seed)
    return pl.DataFrame({
        "total_cost_per_hour": [50.0 + rng.gauss(0, 0.5)],
        "node_count": [10.0 + rng.gauss(0, 0.3)],
        "running_pods": [30.0 + rng.gauss(0, 0.5)],
        "pending_pods": [1.0 + rng.gauss(0, 0.1)],
        "node_count_over_time": [[2 + min(i, 8) + rng.gauss(0, 0.2) for i in range(12)]],
    })


def _make_tier2_far() -> pl.DataFrame:
    """Tier 2 observation >2σ away — should trigger RED."""
    return pl.DataFrame({
        "total_cost_per_hour": [70.0],  # ~6.7σ from mean=50, std=3
        "node_count": [15.0],           # ~5σ from mean=10, std=1
        "running_pods": [30.0],         # on target
        "pending_pods": [1.0],          # on target
        "node_count_over_time": [[10.0] * 12],  # flat high — very different shape
    })


def test_dtw_identical() -> None:
    a = [1.0, 2.0, 3.0, 4.0]
    assert dtw_distance(a, a) == 0.0, "DTW of identical series should be 0"


def test_dtw_shifted() -> None:
    a = [0.0, 1.0, 2.0, 3.0]
    b = [1.0, 2.0, 3.0, 4.0]
    d = dtw_distance(a, b)
    assert d > 0, "DTW of shifted series should be > 0"
    assert d < 10, "DTW of slightly shifted series should be small"


def test_sigma_green() -> None:
    tier1 = _make_tier1()
    tier2 = _make_tier2_close()
    scores = compute_sigma_scores(tier1, tier2, METRICS)
    for s in scores:
        assert s.fidelity in (Fidelity.GREEN, Fidelity.YELLOW), (
            f"{s.metric}: expected GREEN/YELLOW, got {s.fidelity} (z={s.z:.2f})"
        )


def test_sigma_red() -> None:
    tier1 = _make_tier1()
    tier2 = _make_tier2_far()
    scores = compute_sigma_scores(tier1, tier2, METRICS)
    red_metrics = {s.metric for s in scores if s.fidelity == Fidelity.RED}
    assert "total_cost_per_hour" in red_metrics, "cost should be RED when 6σ away"
    assert "node_count" in red_metrics, "node_count should be RED when 5σ away"


def test_scorecard_overall() -> None:
    tier1 = _make_tier1()
    tier2_close = _make_tier2_close()
    sc = build_scorecard(compute_sigma_scores(tier1, tier2_close, METRICS))
    assert sc.overall in (Fidelity.GREEN, Fidelity.YELLOW)

    tier2_far = _make_tier2_far()
    sc_bad = build_scorecard(compute_sigma_scores(tier1, tier2_far, METRICS))
    assert sc_bad.overall == Fidelity.RED


def test_compare_sigma_e2e() -> None:
    tier1 = _make_tier1()
    tier2 = _make_tier2_close()
    scorecard = compare_sigma(tier1, tier2, METRICS)
    assert scorecard.overall in (Fidelity.GREEN, Fidelity.YELLOW)
    # Verify JSON and markdown generation don't crash
    j = scorecard.to_json()
    assert "overall_fidelity" in j
    md = scorecard.to_markdown()
    assert "Fidelity Scorecard" in md


def test_dtw_result_fidelity() -> None:
    tier1 = _make_tier1()
    tier2_close = _make_tier2_close()
    sc = compare_sigma(tier1, tier2_close, METRICS, ts_column="node_count_over_time")
    assert len(sc.dtw_results) == 1
    assert sc.dtw_results[0].fidelity in (Fidelity.GREEN, Fidelity.YELLOW)

    tier2_far = _make_tier2_far()
    sc_bad = compare_sigma(tier1, tier2_far, METRICS, ts_column="node_count_over_time")
    assert len(sc_bad.dtw_results) == 1


def test_per_metric_tolerance() -> None:
    """scheduling_latency with 2σ tolerance should be GREEN even at z=1.5."""
    tier1 = pl.DataFrame({"scheduling_latency": [100.0 + i * 0.1 for i in range(100)]})
    std = tier1["scheduling_latency"].std()
    mean = tier1["scheduling_latency"].mean()
    # Observed at 1.5σ above mean
    obs_val = mean + 1.5 * std
    tier2 = pl.DataFrame({"scheduling_latency": [obs_val]})
    scores = compute_sigma_scores(tier1, tier2, ["scheduling_latency"])
    assert len(scores) == 1
    assert scores[0].fidelity == Fidelity.GREEN, (
        f"scheduling_latency at z=1.5 should be GREEN with 2σ tolerance, got {scores[0].fidelity}"
    )


def main() -> None:
    tests = [
        test_dtw_identical,
        test_dtw_shifted,
        test_sigma_green,
        test_sigma_red,
        test_scorecard_overall,
        test_compare_sigma_e2e,
        test_dtw_result_fidelity,
        test_per_metric_tolerance,
    ]
    failed = 0
    for t in tests:
        try:
            t()
            print(f"  ✓ {t.__name__}")
        except Exception as e:
            print(f"  ✗ {t.__name__}: {e}")
            failed += 1
    print(f"\n{'All tests passed!' if not failed else f'{failed} test(s) FAILED'}")
    sys.exit(failed)


if __name__ == "__main__":
    main()
