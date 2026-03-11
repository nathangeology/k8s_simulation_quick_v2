"""CLI entry point: kubesim validate-eks manifests/ --cluster my-cluster --output results.parquet"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from kubesim.validation.eks import EksRunner, export_results


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="kubesim validate-eks",
        description="Run translated scenarios on a real EKS cluster and collect metrics",
    )
    parser.add_argument("manifests", type=Path, help="Directory of K8s manifest YAML files")
    parser.add_argument("--cluster", required=True, help="EKS cluster name (used as kubectl context)")
    parser.add_argument("--output", "-o", type=Path, default=Path("results.parquet"),
                        help="Output parquet file (default: results.parquet)")
    parser.add_argument("--namespace", "-n", type=str, default="kubesim",
                        help="Kubernetes namespace (default: kubesim)")
    parser.add_argument("--variant", "-v", type=str, default="",
                        help="Variant label for the result row")
    parser.add_argument("--context", type=str, default=None,
                        help="kubectl context override (default: uses --cluster as context)")
    parser.add_argument("--timeout", type=int, default=300,
                        help="Convergence timeout in seconds (default: 300)")
    parser.add_argument("--prometheus-url", type=str, default=None,
                        help="Prometheus endpoint for additional metrics")

    args = parser.parse_args(argv)

    if not args.manifests.is_dir():
        print(f"Error: manifests directory not found: {args.manifests}", file=sys.stderr)
        return 1

    context = args.context or f"arn:aws:eks:us-east-1:000000000000:cluster/{args.cluster}"

    runner = EksRunner(
        cluster=args.cluster,
        namespace=args.namespace,
        context=context,
        timeout_s=args.timeout,
        prometheus_url=args.prometheus_url,
    )

    print(f"Running EKS validation on cluster '{args.cluster}' ...")
    result = runner.run(args.manifests, variant=args.variant)

    out = export_results([result], args.output)
    print(f"Results written to {out}")
    print(f"  nodes={result.node_count} pods={result.pod_count} "
          f"running={result.running_pods} pending={result.pending_pods}")
    print(f"  cost/hr=${result.total_cost_per_hour:.4f} "
          f"disruptions={result.disruption_events} "
          f"elapsed={result.final_time}ms")

    if result.scheduling_latencies_ms:
        avg = sum(result.scheduling_latencies_ms) / len(result.scheduling_latencies_ms)
        p99_idx = int(len(result.scheduling_latencies_ms) * 0.99)
        sorted_lat = sorted(result.scheduling_latencies_ms)
        p99 = sorted_lat[min(p99_idx, len(sorted_lat) - 1)]
        print(f"  scheduling latency: avg={avg:.0f}ms p99={p99:.0f}ms")

    return 0


if __name__ == "__main__":
    sys.exit(main())
