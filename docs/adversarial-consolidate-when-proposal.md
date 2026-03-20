# Adversarial Search for ConsolidateWhen Trend Analysis

**Date:** 2026-03-20
**Bead:** k8s-fsma

---

## 1. What "Adversarial" Means for Trend Comparison

Standard adversarial search finds scenarios where two strategies diverge most on
a scalar metric (e.g., cost_efficiency delta between MostAllocated vs
LeastAllocated). ConsolidateWhen is fundamentally different: the interesting
question is not "which scenario produces the worst single outcome?" but "which
scenario produces the most pathological cost-disruption tradeoff curve?"

A scenario is adversarial for ConsolidateWhen when:

1. **Sharp knee**: A small threshold change (e.g., 1.0 → 1.5) causes a
   disproportionate jump in disruption or cost. Operators tuning the threshold
   would hit a cliff they can't predict from neighboring values.

2. **Decision ratio miscalibration**: `WhenCostJustifiesDisruption` at
   threshold=1.0 performs *worse* than `WhenEmpty` — the cost-benefit
   calculation actively misleads, consolidating nodes that shouldn't be touched.

3. **Non-convex Pareto frontier**: No good middle ground exists. The
   cost-disruption curve has a concavity where increasing the threshold
   simultaneously increases both cost *and* disruption over some range.

4. **Extreme threshold sensitivity**: The outcome at threshold=0.5 vs
   threshold=1.0 differs by >50% on cost or disruption, meaning the feature
   is fragile and small configuration errors have outsized impact.

These are all *curve properties*, not point properties. The scoring function
must evaluate the shape of the relationship across multiple thresholds, not
just the delta between two variants.

---

## 2. Scoring Function Proposals

Each trial in the adversarial search runs a single scenario at N thresholds
(the "threshold sweep"). The scoring function evaluates the resulting
cost-disruption curve and returns a scalar that Optuna maximizes.

### Threshold Sweep Design

Each Optuna trial generates a workload/cluster configuration, then the evaluator
runs it at a fixed set of thresholds:

```
THRESHOLDS = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0]
```

Plus two reference variants:
- `WhenEmpty` (baseline — no cost-based consolidation)
- `WhenEmptyOrUnderutilized` (current default)

This produces 10 data points per trial. The scoring functions below operate on
these 10 points.

### 2A. Max Slope Sensitivity

Find the threshold interval with the steepest cost or disruption gradient.

```python
def max_slope_sensitivity(curve: list[ThresholdResult]) -> float:
    """Score = max |Δmetric / Δthreshold| across adjacent threshold pairs.

    Finds scenarios with sharp knees where a small threshold change
    causes a large outcome change.
    """
    max_slope = 0.0
    for i in range(len(curve) - 1):
        dt = curve[i + 1].threshold - curve[i].threshold
        if dt <= 0:
            continue
        # Normalize cost and disruption to [0,1] using curve range
        cost_slope = abs(curve[i + 1].cost - curve[i].cost) / dt
        disr_slope = abs(curve[i + 1].disruption - curve[i].disruption) / dt
        max_slope = max(max_slope, cost_slope, disr_slope)
    # Normalize by curve range to make cross-scenario comparable
    cost_range = max(c.cost for c in curve) - min(c.cost for c in curve)
    disr_range = max(c.disruption for c in curve) - min(c.disruption for c in curve)
    denom = max(cost_range + disr_range, 1e-6)
    return max_slope / denom
```

**Strengths**: Directly targets the "sharp knee" failure mode. Simple, stateless.
**Weaknesses**: Sensitive to threshold spacing. Misses non-convexity.

### 2B. Pareto Area Divergence

Measure the area between the scenario's cost-disruption curve and an ideal
reference (the convex hull of the best achievable points).

```python
def pareto_area_divergence(curve: list[ThresholdResult],
                           ref_empty: ThresholdResult,
                           ref_underutilized: ThresholdResult) -> float:
    """Score = area between the WhenCostJustifiesDisruption curve and
    the line connecting WhenEmpty and WhenEmptyOrUnderutilized.

    Large area means the threshold-based policy explores a wide
    cost-disruption tradeoff space. Non-convex regions (where the
    curve bulges above the reference line) indicate pathological
    behavior.
    """
    # Reference line: WhenEmpty (high cost, low disruption) to
    # WhenEmptyOrUnderutilized (low cost, high disruption)
    ref_points = sorted(
        [ref_empty, ref_underutilized],
        key=lambda p: p.cost,
    )
    # Threshold curve points sorted by cost
    pts = sorted(curve, key=lambda p: p.cost)

    # Compute area between curve and reference line using trapezoidal rule
    # Positive area = curve is above reference (non-convex / worse)
    # Negative area = curve is below reference (better tradeoff)
    total_area = 0.0
    non_convex_area = 0.0
    for i in range(len(pts) - 1):
        dx = pts[i + 1].cost - pts[i].cost
        avg_disruption = (pts[i].disruption + pts[i + 1].disruption) / 2
        # Interpolate reference line at this cost
        ref_disruption = _interpolate_ref(
            pts[i].cost, ref_points[0], ref_points[1],
        )
        delta = avg_disruption - ref_disruption
        total_area += abs(delta) * abs(dx)
        if delta > 0:
            non_convex_area += delta * abs(dx)

    # Weight non-convex area 2x (it's the pathological case)
    return total_area + non_convex_area
```

**Strengths**: Captures overall curve shape. Penalizes non-convexity (the "no
good middle ground" failure). Naturally compares against reference policies.
**Weaknesses**: Requires both reference variants. More complex to implement.

### 2C. Composite Trend Score (Recommended)

Combine multiple curve pathologies into a single score with interpretable
components.

```python
def composite_trend_score(curve: list[ThresholdResult],
                          ref_empty: ThresholdResult,
                          ref_underutilized: ThresholdResult) -> float:
    """Weighted combination of curve pathology indicators.

    Components:
    1. knee_sharpness: max normalized slope across adjacent thresholds
    2. miscalibration: how much worse WhenCostJustifiesDisruption(1.0)
       is vs WhenEmpty on cost (negative = miscalibrated)
    3. non_monotonicity: count of threshold intervals where increasing
       threshold increases BOTH cost and disruption
    4. range_magnitude: total spread of cost and disruption across
       the threshold sweep (larger = more sensitive)
    """
    # 1. Knee sharpness (reuse max_slope_sensitivity logic)
    slopes = []
    for i in range(len(curve) - 1):
        dt = curve[i + 1].threshold - curve[i].threshold
        if dt <= 0:
            continue
        cost_slope = abs(curve[i + 1].cost - curve[i].cost) / dt
        disr_slope = abs(curve[i + 1].disruption - curve[i].disruption) / dt
        slopes.append(max(cost_slope, disr_slope))
    cost_range = max(c.cost for c in curve) - min(c.cost for c in curve)
    disr_range = max(c.disruption for c in curve) - min(c.disruption for c in curve)
    norm = max(cost_range + disr_range, 1e-6)
    knee = max(slopes) / norm if slopes else 0.0

    # 2. Miscalibration: WhenCostJustifiesDisruption(1.0) vs WhenEmpty
    t1 = next((c for c in curve if c.threshold == 1.0), None)
    miscal = 0.0
    if t1 and ref_empty.cost > 0:
        # Positive = cost-justified is cheaper (good)
        # Negative = cost-justified is MORE expensive (miscalibrated)
        savings = (ref_empty.cost - t1.cost) / ref_empty.cost
        if savings < 0:
            miscal = abs(savings)  # penalize miscalibration

    # 3. Non-monotonicity: threshold↑ should mean disruption↓
    non_mono = 0
    for i in range(len(curve) - 1):
        if curve[i + 1].threshold > curve[i].threshold:
            cost_up = curve[i + 1].cost > curve[i].cost
            disr_up = curve[i + 1].disruption > curve[i].disruption
            if cost_up and disr_up:
                non_mono += 1
    non_mono_frac = non_mono / max(len(curve) - 1, 1)

    # 4. Range magnitude (clamped-relative to reference)
    ref_cost = max(ref_empty.cost, ref_underutilized.cost, 1e-6)
    ref_disr = max(ref_empty.disruption, ref_underutilized.disruption, 1e-6)
    range_score = (cost_range / ref_cost) + (disr_range / ref_disr)

    # Weighted combination
    return (
        0.30 * knee
        + 0.25 * miscal
        + 0.25 * non_mono_frac
        + 0.20 * range_score
    )
```

**Strengths**: Captures all four adversarial failure modes. Weights are
interpretable and tunable. Each component can be logged independently for
analysis. Stateless.
**Weaknesses**: Four tuning weights. Requires reference variants.

---

## 3. Recommended Search Space

The search varies workload/cluster parameters while the threshold sweep is
fixed. This separates "what scenario to test" (searched) from "how to evaluate
it" (fixed sweep).

### Searched Dimensions

| Dimension | Range | Rationale |
|-----------|-------|-----------|
| Node pool count | 1–3 | Multi-pool creates heterogeneous consolidation targets |
| Instance type mix | All types / single-fit | Homogeneous vs heterogeneous node costs |
| Max nodes per pool | 10–80 | Moderate scale (avoid OOM, keep eval fast) |
| Workload count | 2–5 | Enough diversity without combinatorial explosion |
| Workload archetypes | web_app, batch_job, saas_microservice, ml_training | Core archetypes that interact differently with consolidation |
| Pod priority mix | low/medium/high | Priority affects disruption cost calculation |
| PDB coverage | 0%/25%/50%/80% | PDBs constrain which nodes can be consolidated |
| Traffic pattern | diurnal/spike/steady | Diurnal creates consolidation windows; spike tests reactivity |
| Peak multiplier | 1.5–8.0 | Controls scale-down magnitude at trough |
| Scale-down timing | 6h/12h/18h | When consolidation opportunities appear |
| Node startup latency | 0s/15s/30s/60s | Affects consolidation timing vs new provisioning |
| Consolidate-after delay | 0s–600s | How long Karpenter waits before consolidating |
| Disruption budget | 1–10 nodes | Limits concurrent consolidation |

### Fixed Dimensions (per trial)

| Dimension | Value | Rationale |
|-----------|-------|-----------|
| Threshold sweep | [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0] | Fixed evaluation grid |
| Reference variants | WhenEmpty, WhenEmptyOrUnderutilized | Baselines for miscalibration scoring |
| Runs per variant | 20 | Fast screening; top-k re-evaluated at 100 |
| Seeds | [42] during search, [42, 100, 200] for re-eval | Progressive evaluation |
| Scheduling strategy | reverse_schedule | Fast path during search |
| Time mode | wall_clock | Avoids consolidation thrash from logical mode |

### Search Budget

- **Phase 1 (exploration)**: 100 trials with Optuna TPE
- **Phase 2 (re-evaluation)**: Top 30 re-run with 3 seeds × 100 runs
- **Phase 3 (diversity selection)**: `diverse_top_k` selects final 10

Total eval cost: ~100 × 10 variants × 20 runs + 30 × 10 × 100 = 50K sim ticks.
At ~1ms/tick, this is ~50 seconds wall clock.

---

## 4. Implementation Plan

### Step 1: Add `ThresholdResult` data structure and curve evaluation

New file: `python/kubesim/trend_scoring.py`

```python
@dataclass
class ThresholdResult:
    threshold: float  # or None for WhenEmpty/WhenEmptyOrUnderutilized
    policy: str
    cost: float       # total_cost_per_hour
    disruption: float # pods_evicted / total_pods
    availability: float
    node_count: float

def evaluate_threshold_sweep(scenario, thresholds, seeds):
    """Run scenario at each threshold + reference policies, return curve."""
```

Contains `max_slope_sensitivity`, `pareto_area_divergence`, and
`composite_trend_score` from §2.

### Step 2: New adversarial search script

New file: `scripts/find_adversarial_consolidate_when.py`

- Extends `OptunaAdversarialSearch` pattern from `adversarial.py`
- Each trial: build scenario → run threshold sweep → score curve
- Objective: maximize `composite_trend_score`
- Output: `scenarios/adversarial/consolidate-when/worst_case_*.yaml`

Key difference from `find_adversarial.py`: each trial evaluates 10 variants
(8 thresholds + 2 references) instead of 2. The `_build_scenario` method
generates the workload/cluster config; the evaluator handles the sweep.

### Step 3: Extend `OptunaAdversarialSearch` for multi-variant sweeps

Add a `sweep_evaluator` callback to `OptunaAdversarialSearch` that replaces
the two-variant comparison with an N-variant threshold sweep. This keeps the
Optuna trial → scenario → score pipeline intact while changing what "evaluate"
means.

### Step 4: Results analysis and visualization

Extend `report.py` or add a new analysis script that:
- Plots cost-disruption curves for top-k adversarial scenarios
- Highlights knee points, non-monotonic regions, and miscalibration
- Compares curve shapes across scenarios (overlay plots)

### Step 5: Validate with existing scenarios

Run the new scoring functions on the existing `scenarios/consolidate-when/`
files to verify they produce sensible rankings. The
`cost-disruption-tradeoff.yaml` scenario (10 threshold variants) is the
primary validation target.

---

## 5. Comparison with Existing Approach

| Aspect | Current (`find_adversarial.py`) | Proposed |
|--------|--------------------------------|----------|
| Comparison | 2 variants (A vs B) | 10 variants (8 thresholds + 2 refs) |
| Scoring | Scalar divergence (point) | Curve shape analysis (trend) |
| What's adversarial | Max delta between two strategies | Pathological curve properties |
| Search space | Workloads + cluster + strategy | Workloads + cluster (threshold is swept, not searched) |
| Eval cost per trial | 2 × seeds × runs | 10 × seeds × runs (5× more) |
| Output | Worst-case scenario YAML | Worst-case scenario YAML + curve data |

The 5× eval cost per trial is offset by the lower budget needed — curve
pathologies are rarer than point divergences, so Optuna's TPE converges faster
when the objective is more informative.

---

## 6. Open Questions

1. **Threshold grid density**: 8 points may miss narrow knees between grid
   points. An adaptive refinement step (bisect intervals with high slope)
   could catch these, at the cost of variable eval time per trial.

2. **Weight tuning for composite score**: The 0.30/0.25/0.25/0.20 weights in
   `composite_trend_score` are initial guesses. Running the scorer on the
   existing `consolidate-when/` scenarios and checking that known-interesting
   cases rank highly would validate or adjust these.

3. **Interaction with scale-invariant scoring**: The `clamped_relative_divergence`
   from `scale_invariant_scoring.py` normalizes per-objective deltas. The trend
   scoring functions normalize differently (by curve range). These should be
   kept separate — trend scoring is a different abstraction level.
