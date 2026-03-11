"""CLI entry points for kubesim validation commands."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from kubesim.validation.translator import translate_scenario


def translate_main(argv: list[str] | None = None) -> int:
    """kubesim translate scenario.yaml --output manifests/"""
    parser = argparse.ArgumentParser(
        prog="kubesim translate",
        description="Convert KubeSim scenario YAML to K8s manifests",
    )
    parser.add_argument("scenario", type=Path, help="Path to scenario YAML file")
    parser.add_argument("--output", "-o", type=Path, default=Path("manifests"),
                        help="Output directory for manifests (default: manifests/)")
    parser.add_argument("--variant", "-v", type=str, default=None,
                        help="Apply variant-specific config (e.g. deletion cost, PDB)")

    args = parser.parse_args(argv)

    if not args.scenario.exists():
        print(f"Error: scenario file not found: {args.scenario}", file=sys.stderr)
        return 1

    written = translate_scenario(args.scenario, args.output, args.variant)
    print(f"Generated {len(written)} manifests in {args.output}/")
    for p in written:
        print(f"  {p}")
    return 0


def validate_kwok_main(argv: list[str] | None = None) -> int:
    """kubesim validate-kwok manifests/ --output results.parquet"""
    from kubesim.validation.kwok import run_kwok_validation

    parser = argparse.ArgumentParser(
        prog="kubesim validate-kwok",
        description="Run translated scenarios on KWOK/KIND and collect metrics",
    )
    parser.add_argument("manifests", type=Path, help="Directory of K8s manifest YAML files")
    parser.add_argument("--output", "-o", type=Path, default=Path("results.parquet"),
                        help="Output file (default: results.parquet)")
    parser.add_argument("--variant", "-v", type=str, default="kwok",
                        help="Variant name for results (default: kwok)")
    parser.add_argument("--nodes", "-n", type=int, default=10,
                        help="Number of fake KWOK nodes (default: 10)")
    parser.add_argument("--settle", type=int, default=30,
                        help="Seconds to wait for pods to settle (default: 30)")
    parser.add_argument("--no-cleanup", action="store_true",
                        help="Don't delete the KIND cluster after run")

    args = parser.parse_args(argv)

    if not args.manifests.is_dir():
        print(f"Error: manifests directory not found: {args.manifests}", file=sys.stderr)
        return 1

    run_kwok_validation(
        manifests_dir=args.manifests,
        output=args.output,
        variant=args.variant,
        node_count=args.nodes,
        settle_seconds=args.settle,
        cleanup=not args.no_cleanup,
    )
    return 0


# Keep backward compat: old entry point
main = translate_main

if __name__ == "__main__":
    sys.exit(main())
