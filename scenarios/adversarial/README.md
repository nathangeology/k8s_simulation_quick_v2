# Adversarial Scenarios

Worst-case scenarios discovered by the adversarial finder where MostAllocated vs
LeastAllocated scheduling strategies diverge most in cost.

## Structure

- `scenarios/adversarial/*.yaml` — Clean input YAMLs (no results data, no scores)
- `results/adversarial/manifest.json` — Scores, direction, category, per-variant metrics
- `results/adversarial/summary.md` — Human-readable summary of findings

## Categories

Scenarios are categorized by direction of cost divergence:

- **adversarial_to_most**: MostAllocated costs more (positive cost delta)
- **adversarial_to_least**: LeastAllocated costs more (negative cost delta)
- **both_degrade**: Both strategies degrade similarly

## Generation

```bash
python scripts/find_adversarial.py
# or
python -m kubesim.run_adversarial --budget 1000 --top-k 5
```
