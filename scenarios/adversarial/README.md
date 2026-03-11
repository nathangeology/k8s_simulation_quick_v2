# Adversarial Scenarios

Worst-case scenarios discovered by the adversarial finder where MostAllocated vs
LeastAllocated scheduling strategies diverge most in cost.

## Output Structure

- `scenarios/adversarial/*.yaml` — Clean input YAMLs (no results data, no scores)
- `results/adversarial/manifest.json` — Scores, direction, category, per-variant metrics
- `results/adversarial/summary.md` — Human-readable summary of findings

## Directional Categories

Scenarios are categorized by signed delta (`most_cost - least_cost`):

- **adversarial_to_most** — MostAllocated costs more (positive delta)
- **adversarial_to_least** — LeastAllocated costs more (negative delta)
- **both_degrade** — Equal cost (zero delta)

Top-K scenarios are kept from each category separately.

## Generation

```bash
python -m kubesim run-adversarial --budget 1000 --top-k 10
```

Or via the standalone script:

```bash
python scripts/find_adversarial.py
```
