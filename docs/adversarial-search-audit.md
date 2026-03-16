# Adversarial Search Audit

**Date:** 2026-03-14
**Bead:** k8s-ed6

---

## 1. Current State Assessment

### 1.1 Scripts Inventory

| Script | Search Algorithm | Parameters Searched | Objective | Budget | Seeds |
|--------|-----------------|-------------------|-----------|--------|-------|
| `scripts/find_adversarial.py` | Hypothesis (random PBT) | Node pools, workloads, instance types, traffic | Multi-objective: cost_efficiency, availability, scheduling_failure_rate, entropy_deviation | 500 (×2 strategies) | [42, 100, 200] |
| `scripts/find_adversarial_deletion_cost.py` | Hypothesis (random PBT) | Same space, 5-way variant comparison | Max pairwise divergence across availability, cost_efficiency, disruption_rate | 500 (×2 strategies) | [42, 100, 200] |
| `scripts/find_adversarial_karpenter_version.py` | `AdversarialFinder` class (Hypothesis PBT) | Same space, Karpenter v0.35 vs v1.x | Multi-objective: cost_efficiency, availability, consolidation_waste | 500 (×2 strategies) | [42, 100, 200] |
| `python/kubesim/run_adversarial.py` | `AdversarialFinder` class | Configurable via CLI | Cost divergence between variant pair | 1000 | [42, 123, 7] |

### 1.2 Supporting Modules

| Module | Role |
|--------|------|
| `adversarial.py` | `AdversarialFinder` class — wraps Hypothesis with top-k tracking, feature importance, shrinking |
| `strategies.py` | Hypothesis composite strategies: `cluster_scenario()`, `chaos_scenario()`, 9 workload archetypes, node pool variants |
| `objectives.py` | 6 objective functions + `multi_objective` combinator + `pareto_violation` |
| `scenario_templates.py` | 6 hand-crafted stress templates (bin packing, consolidation cascade, spot storms, etc.) |
| `report.py` | A/B comparison reports with Mann-Whitney U, bootstrap CI, Polars + plotly |

### 1.3 Search Space Coverage

**Dimensions currently searched:**

| Dimension | Range | Notes |
|-----------|-------|-------|
| Node pools | 1–3 pools | Instance types sampled from 20 types |
| Max nodes per pool | 11–200 | |
| Workload count | 2–8 | |
| Workload types | 9 archetypes | web_app, ml_training, batch_job, saas_microservice + 5 edge cases |
| CPU requests | 50m–32000m | Uniform or normal distribution |
| Memory requests | 64Mi–131072Mi | Uniform or normal distribution |
| GPU requests | {1, 2, 4, 8} | Only on ml_training |
| Traffic patterns | 4 types | diurnal, spike, steady, diurnal_with_spike |
| Consolidation policy | 2 values | WhenEmpty, WhenUnderutilized |
| Scale-down patterns | 3 types | cliff, staggered, oscillating (chaos only) |
| Topology spread | max_skew 1–5 | Optional per workload |
| PDB | min_available 1/2/25%/50%/80% | Optional per workload |
| Batch duration | 1m–24h | Exponential or lognormal |

**Dimensions NOT searched:**

| Missing Dimension | Impact |
|-------------------|--------|
| Karpenter disruption budgets | HIGH — real clusters use `spec.disruption.budgets` to limit concurrent disruptions |
| Karpenter batch idle duration | HIGH — `spec.disruption.consolidateAfter` controls consolidation timing |
| Scheduling strategy (FullScan/HintBased/ReverseSchedule/etc.) | MEDIUM — 5 strategies exist in the engine but adversarial search doesn't vary them |
| Node startup latency | MEDIUM — real clusters have 30s–5min node startup; search uses 0 |
| Spot interruption rate | HIGH — `spot_interruption_storm` template exists but isn't used in search |
| Multi-AZ topology | MEDIUM — zone distribution affects topology spread constraints |
| Pod priority/preemption | MEDIUM — priority classes exist but preemption cascades aren't explored |
| HPA scaling parameters | LOW — HPA target/metric varies but scaling lag isn't modeled |

### 1.4 Generated Scenario Analysis

**Top-level `scenarios/adversarial/` (10 scenarios):**
- 8 of 10 are minimal: `name: '000'`, `runs: 1`, single pool, single web_app workload
- Hypothesis shrinking converged to the simplest possible divergent config
- These are useful as minimal reproducing examples but don't represent real-world adversarial conditions
- Only 2 scenarios (worst_case_03, worst_case_08) have complex multi-workload configs

**`scenarios/adversarial/scheduling/` (19 scenarios):**
- More diverse — multi-pool, mixed workloads, varying run counts
- But still show convergence: many share the same pool structure with minor workload variations
- Run counts range from 1 to 7291 — the `runs` field is part of the generated scenario, not the search budget

**`scenarios/adversarial/deletion-cost/` (10 scenarios):**
- Chaos-mode scenarios with extreme replica counts (447–866 replicas)
- Good at finding deletion ordering divergence
- Narrow focus: mostly single-pool + extreme-replica patterns

**`scenarios/adversarial/karpenter-version/` (10 scenarios):**
- Multi-pool, multi-workload, realistic-looking configs
- Best diversity of the three subdirectories
- Divergence scores are low (max 0.0452) — suggests the two Karpenter versions behave similarly under random scenarios

### 1.5 What Works

1. **Hypothesis integration is solid.** The `@given` + `@settings` pattern with `derandomize=True` gives reproducible searches. The composite strategies in `strategies.py` are well-designed and composable.

2. **Multi-objective framework.** The objectives module provides clean, composable functions. The `pareto_violation` function enables dominated-solution detection.

3. **Feature importance tracking.** `AdversarialFinder` can track which scenario features correlate with high divergence — this is the seed of a smarter search.

4. **Scenario templates.** The 6 hand-crafted templates in `scenario_templates.py` capture known trouble patterns. These could seed guided search.

5. **Report pipeline.** The `report.py` module produces statistically rigorous A/B comparisons with bootstrap CIs and Mann-Whitney U tests.

### 1.6 What Doesn't Work

1. **Search is unguided random.** Hypothesis is a property-based testing tool, not an optimization engine. It generates random inputs and shrinks failures. There's no feedback loop — the search doesn't learn from high-scoring scenarios to generate better ones.

2. **Convergence to trivial configs.** Hypothesis shrinking minimizes inputs, so the "best" adversarial scenarios are often the simplest ones (1 pool, 1 workload, runs=1). These are minimal reproducing examples, not realistic worst cases.

3. **Run count is a searched parameter.** The `runs` field (1–10000) is part of the generated scenario. High run counts (9000+) don't mean the search ran 9000 evaluations — they mean the generated scenario specifies 9000 simulation runs per evaluation. This is wasteful: each `evaluate()` call runs `batch_run(config, [42, 100, 200])` which runs the scenario 3× (one per seed), and each scenario internally specifies its own `runs` count. A scenario with `runs: 9422` evaluated with 3 seeds = 28,266 simulation ticks per candidate.

4. **No progressive evaluation.** Every candidate gets the same evaluation cost regardless of promise. A scenario that clearly has zero divergence after 3 seeds still runs all seeds.

5. **Chaos mode is too narrow.** It always uses single-instance pools + edge-case workloads. Real adversarial conditions often involve subtle interactions between normal-looking configs.

---

## 2. Proposed Improvements (Ranked by Impact)

### 2.1 Replace Random Search with Bayesian Optimization [HIGH IMPACT]

**Problem:** Hypothesis generates random scenarios with no feedback. Budget of 500 evaluations explores a tiny fraction of the space.

**Proposal:** Use Optuna (Tree-structured Parzen Estimator) or similar to guide the search toward high-divergence regions.

```
Approach:
1. Define search space as Optuna parameters (node count, workload mix, etc.)
2. Objective: maximize combined_divergence (or multi-objective via Optuna's NSGA-II)
3. Each trial: build scenario dict → batch_run → compute divergence
4. Optuna learns which parameter regions produce high divergence
```

**Estimated improvement:** 3–10× better scenarios per evaluation budget. Bayesian optimization typically finds better optima than random search in 100–500 trials for 10–30 dimensional spaces.

**Effort:** Medium. The scenario construction logic in `strategies.py` can be reused — just replace the Hypothesis `@given` wrapper with Optuna trial parameter sampling.

### 2.2 Fix Run Count Explosion [HIGH IMPACT, LOW EFFORT]

**Problem:** The `runs` field is sampled from `st.integers(1, 10000)`, making some evaluations 10,000× more expensive than others with no benefit to the search.

**Proposal:** Fix `runs` to a small constant (e.g., 50–100) during adversarial search. The search needs fast, approximate divergence estimates — not statistically precise results. Reserve high run counts for the final report phase on top-k scenarios.

**Estimated improvement:** 10–100× faster search. Currently a single evaluation with `runs: 9422` and 3 seeds takes ~28K simulation ticks. With `runs: 50` it takes ~150.

### 2.3 Progressive Evaluation (Early Stopping) [HIGH IMPACT, MEDIUM EFFORT]

**Problem:** Every candidate scenario gets full evaluation (3 seeds × N runs) regardless of promise.

**Proposal:** Two-phase evaluation:
1. **Quick screen:** Run with 1 seed, `runs: 10`. If divergence < threshold, discard.
2. **Full evaluation:** Run with 3 seeds, `runs: 100`. Only for candidates that pass screening.

**Estimated improvement:** 5–20× more candidates evaluated per wall-clock hour. Most random scenarios have near-zero divergence and can be discarded cheaply.

### 2.4 Expand Search Space: Karpenter Configuration [HIGH IMPACT, MEDIUM EFFORT]

**Problem:** The search varies workloads and node pools but not Karpenter's own configuration knobs, which are the primary source of real-world cost divergence.

**Proposal:** Add these dimensions to the search space:

| Parameter | Range | Why |
|-----------|-------|-----|
| `consolidateAfter` | 0s–30m | Controls how quickly Karpenter consolidates underutilized nodes |
| `disruption.budgets[].nodes` | 1–10 or "10%" | Limits concurrent disruptions |
| `disruption.budgets[].schedule` | cron expressions | Time-windowed disruption |
| `expireAfter` | 1h–720h | Node TTL before forced replacement |
| `consolidation.policy` | WhenEmpty/WhenUnderutilized | Already searched, but not in combination with above |

### 2.5 Evolutionary/Genetic Crossover [MEDIUM IMPACT, MEDIUM EFFORT]

**Problem:** Each scenario is generated independently. Good scenarios can't "breed" to produce better offspring.

**Proposal:** After initial random search, take top-k scenarios and apply:
1. **Crossover:** Swap workload lists or node pool configs between two high-scoring scenarios
2. **Mutation:** Perturb numeric parameters (±10–20%) of high-scoring scenarios
3. **Selection:** Keep top-k from combined pool, repeat for N generations

This is complementary to Bayesian optimization — can be used as a refinement phase.

### 2.6 Scenario Diversity Enforcement [MEDIUM IMPACT, LOW EFFORT]

**Problem:** Top-k scenarios converge to similar patterns (e.g., all use single-pool + extreme replicas for deletion cost).

**Proposal:** Use novelty search or diversity-aware selection:
1. Define a feature vector per scenario (num_pools, num_workloads, has_gpu, has_pdb, etc.) — `_extract_features()` already does this
2. When selecting top-k, enforce minimum distance between selected scenarios in feature space
3. This ensures the adversarial suite covers different failure modes, not just the single highest-scoring pattern

### 2.7 Multi-Objective Pareto Search [MEDIUM IMPACT, MEDIUM EFFORT]

**Problem:** Current search maximizes a single combined divergence score. This misses scenarios that are extreme on one objective but average on others.

**Proposal:** Use NSGA-II (available in Optuna) for true multi-objective optimization:
- Objectives: cost_efficiency divergence, availability divergence, scheduling_failure_rate divergence
- Output: Pareto frontier of non-dominated scenarios
- Each point on the frontier represents a different type of adversarial condition

### 2.8 Leverage ReverseSchedule for Faster Evaluation [MEDIUM IMPACT, LOW EFFORT]

**Problem:** Each evaluation runs the full simulation with `FullScan` scheduling (O(pods × nodes) per scheduling cycle).

**Proposal:** Use `ReverseSchedule` strategy during adversarial search. It's faster (O(1) per NodeReady event) and produces equivalent results for divergence detection. Reserve `FullScan` for final validation of top-k scenarios.

**Estimated improvement:** 2–5× faster per evaluation, depending on cluster size.

### 2.9 Seed Adversarial Search with Known Trouble Patterns [LOW IMPACT, LOW EFFORT]

**Problem:** Random search starts from scratch every time. The 6 templates in `scenario_templates.py` encode known trouble patterns but aren't used to seed the search.

**Proposal:** Initialize the search population with parameterized versions of the templates:
- `bin_packing_stress` with varying instance types and pod sizes
- `consolidation_cascade` with varying scale-down percentages
- `spot_interruption_storm` with varying spot fractions

This gives the optimizer a warm start in known-interesting regions.

### 2.10 Add Missing Real-World Patterns [LOW IMPACT, MEDIUM EFFORT]

**Problem:** Several important real-world patterns are absent from the search space.

| Pattern | Description |
|---------|-------------|
| Bursty traffic | Sudden 10–100× traffic spikes (Black Friday, viral events) |
| Correlated failures | Multiple nodes failing simultaneously (AZ outage) |
| Rolling deployments | Gradual pod replacement during deploys |
| Cluster autoscaler interaction | CA and Karpenter competing for node provisioning |
| Resource limit vs request mismatch | Pods with limits >> requests causing OOM kills |
| DaemonSet overhead | Per-node pods consuming resources before workload scheduling |

---

## 3. Efficiency Analysis: Run Counts

The `runs` field in generated scenarios controls how many simulation ticks each scenario executes. Current distribution across generated adversarial scenarios:

| Runs | Count | % of scenarios |
|------|-------|---------------|
| 1 | 24 | 49% |
| 50–100 | 2 | 4% |
| 1000–5000 | 10 | 20% |
| 5000–10000 | 13 | 27% |

**Recommendation:** For adversarial search, fix `runs` to 50. For statistical validation of top-k results, use 1000+ runs with the `report.py` pipeline. The search phase needs fast approximate divergence, not precise estimates.

**Statistical significance:** With 3 seeds × 50 runs = 150 samples per variant, a Mann-Whitney U test can detect effect sizes of ~0.3 standard deviations at p < 0.05. This is sufficient for screening; the report phase with 1000+ runs provides publication-quality statistics.

---

## 4. Concrete Next Steps

### Phase 1: Quick Wins (1–2 sessions)
1. **Fix run count:** Cap `runs` at 50–100 in `cluster_scenario()` and `chaos_scenario()` strategies during search
2. **Add progressive evaluation:** Quick screen with 1 seed before full 3-seed evaluation
3. **Use ReverseSchedule:** Set `scheduling_strategy: ReverseSchedule` during search evaluations

### Phase 2: Guided Search (2–3 sessions)
4. **Integrate Optuna:** Replace Hypothesis `@given` with Optuna TPE sampler in `AdversarialFinder`
5. **Add Karpenter config dimensions:** `consolidateAfter`, disruption budgets, `expireAfter`
6. **Diversity-aware selection:** Enforce feature-space distance in top-k selection

### Phase 3: Advanced (3–5 sessions)
7. **NSGA-II multi-objective:** Pareto frontier search via Optuna
8. **Evolutionary refinement:** Crossover/mutation phase on top-k scenarios
9. **Template seeding:** Warm-start search with parameterized trouble templates
10. **Missing patterns:** Add bursty traffic, correlated failures, rolling deployments to search space

---

## 5. Summary

The adversarial search infrastructure is well-architected — the objective functions, strategy composition, feature tracking, and report pipeline are solid foundations. The main weakness is the search algorithm itself: Hypothesis property-based testing is designed for finding bugs via random generation + shrinking, not for optimization. Replacing it with Bayesian optimization (Optuna) while keeping the existing scenario construction and evaluation pipeline would yield the largest improvement. Combined with fixing the run count explosion and adding progressive evaluation, the search could find 10–50× better adversarial scenarios within the same wall-clock budget.
