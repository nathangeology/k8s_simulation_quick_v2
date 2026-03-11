# Adversarial Scenarios

Worst-case scenarios discovered by `AdversarialFinder` where MostAllocated vs
LeastAllocated scheduling strategies diverge most in cost.

## Generation

```bash
python -m kubesim run-adversarial --budget 1000 --top-k 10
```

## Purpose

These scenarios serve as inputs for follow-up property-based testing, ensuring
the simulator handles extreme configurations correctly and that scheduling
strategy comparisons remain valid under adversarial conditions.
