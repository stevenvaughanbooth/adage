#!/usr/bin/env python3
"""Run the held-out phase and the report of a skill-lab config with evals/live/runner.py.

  python3 run_heldout.py <config.json> <root> [--execute] [--adage BIN]

Before it, compile.py must have written <root>/private/adage-home (the compiled skill's reflexes):
the runner's learn phase is skipped, so the `adage` arm gets the compiled skill and nothing else.
"""

import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def main(argv):
    cfg, root, rest = argv[0], argv[1], argv[2:]
    adage = rest[rest.index("--adage") + 1] if "--adage" in rest else str(REPO / "target/debug/adage")
    if not (Path(root) / "private" / "adage-home" / "index.json").exists():
        raise SystemExit(f"{root}/private/adage-home/index.json is missing: run compile.py --home first")
    cmd = ["python3", str(REPO / "evals/live/runner.py"), cfg, "eval", "report", "--root", root, "--adage", adage]
    if "--execute" in rest:
        cmd.append("--execute")
    return subprocess.run(cmd, env=os.environ).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
