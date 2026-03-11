"""CLI entry point: kubesim translate scenario.yaml --output manifests/"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from kubesim.validation.translator import translate_scenario


def main(argv: list[str] | None = None) -> int:
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


if __name__ == "__main__":
    sys.exit(main())
