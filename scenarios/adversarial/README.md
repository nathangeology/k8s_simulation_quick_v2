# Adversarial Scenarios

Worst-case scenarios discovered by `find_adversarial.py` where MostAllocated vs
LeastAllocated scheduling strategies diverge most in cost.

## Generation

```bash
python scripts/find_adversarial.py
```

## Output Structure

- `scenarios/adversarial/*.yaml` — Clean input YAMLs (no results data, no scores)
- `results/adversarial/manifest.json` — Scores, direction, category, per-variant metrics
- `results/adversarial/summary.md` — Human-readable summary of findings

## Categories

Scenarios are ranked separately by direction:

- **adversarial_to_most** — MostAllocated costs more (positive signed delta)
- **adversarial_to_least** — LeastAllocated costs more (negative signed delta)
- **both_degrade** — Both strategies degrade (mixed signals)

Top-k scenarios are kept from each category.

## Purpose

These scenarios serve as inputs for follow-up property-based testing, ensuring
the simulator handles extreme configurations correctly and that scheduling
strategy comparisons remain valid under adversarial conditions.
