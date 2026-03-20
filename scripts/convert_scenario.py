#!/usr/bin/env python3
"""CLI entry point for deterministic scenario conversion.

Usage:
    python scripts/convert_scenario.py --format k8s-import --input <path> --output <path>
"""

import argparse
import sys
from pathlib import Path

# Allow running from repo root
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "python"))

from kubesim.converter.k8s_import import K8sImportAdapter
from kubesim.converter.renderer import render_study_yaml

ADAPTERS = {
    "k8s-import": K8sImportAdapter,
}


def main() -> None:
    parser = argparse.ArgumentParser(description="Convert external scenario formats to native study YAML")
    parser.add_argument("--format", required=True, choices=sorted(ADAPTERS), help="Input format")
    parser.add_argument("--input", required=True, type=Path, help="Path to input (directory or file)")
    parser.add_argument("--output", required=True, type=Path, help="Output YAML path")
    parser.add_argument("--name", help="Override scenario name (default: derived from input)")
    args = parser.parse_args()

    adapter = ADAPTERS[args.format]()
    ir = adapter.convert(args.input)

    if args.name:
        ir.name = args.name

    yaml_str = render_study_yaml(ir)
    args.output.write_text(yaml_str)
    print(f"Converted {len(ir.workloads)} workloads → {args.output}")


if __name__ == "__main__":
    main()
