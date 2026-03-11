"""python -m kubesim CLI dispatcher."""

from __future__ import annotations

import sys


def main() -> int:
    if len(sys.argv) < 2:
        print("Usage: kubesim <command> [args...]")
        print("Commands: translate, run, train")
        return 1

    cmd = sys.argv[1]

    if cmd == "translate":
        from kubesim.validation.cli import main as translate_main
        return translate_main(sys.argv[2:])
    else:
        print(f"Unknown command: {cmd}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
