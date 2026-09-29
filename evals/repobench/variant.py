#!/usr/bin/env python3
"""A model slice of a finished RepoBench repository: the same held-out tasks, the same learned
index and the same AGENTS.md, worked by another model.

  python3 evals/repobench/variant.py <repo-id> --model claude-sonnet-5 --suffix sonnet5
        [--repeats 1] [--budget 25] [--runs DIR]

Writes configs/<repo-id>-<suffix>.json (held-out phase only) and seeds <runs>/<repo-id>-<suffix>/
with a copy of the Haiku root's private/adage-home (the index learned from the Haiku training
traces): a person who switches models keeps what Adage learned. Then run
`runner.py configs/<repo-id>-<suffix>.json eval report --execute --root <that root>`.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

import common


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("--model", required=True)
    ap.add_argument("--suffix", required=True)
    ap.add_argument("--repeats", type=int, default=1)
    ap.add_argument("--budget", type=float, default=25.0)
    ap.add_argument("--arms", default="control,adage,agents-md")
    ap.add_argument("--runs", default=str(common.cache_dir() / "runs"))
    a = ap.parse_args(argv)
    src = json.loads((common.HERE / "configs" / f"{a.repo}.json").read_text())
    cfg = dict(src)
    cfg.update(name=f"{src['name']}-{a.suffix}", model=a.model, budget_usd=a.budget)
    cfg["eval"] = {**src["eval"], "repeats": a.repeats, "arms": a.arms.split(",")}
    out = common.HERE / "configs" / f"{a.repo}-{a.suffix}.json"
    out.write_text(json.dumps(cfg, indent=1) + "\n")
    root = Path(a.runs) / f"{a.repo}-{a.suffix}"
    home = Path(a.runs) / a.repo / "private" / "adage-home"
    dest = root / "private" / "adage-home"
    if not dest.exists():
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(home, dest, ignore=shutil.ignore_patterns("sessions", "cache"))
    print(f"{cfg['name']}: {len(cfg['eval']['tasks'])} tasks x {a.repeats} x {len(cfg['eval']['arms'])} arms "
          f"({a.model}) -> {out}; index copied to {dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
