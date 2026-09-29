#!/usr/bin/env python3
"""RepoBench v2: rerun the held-out phase of a finished RepoBench repository on the same task
definitions and chronological split, reusing its archived training sessions.

  python3 evals/repobench/rerun.py <repo-id> [--max-turns 60] [--repeats 2] [--arms control,adage]
        [--eval-budget 16] [--thresholds default|demo] [--model M --suffix S]
        [--from DIR] [--runs DIR]

Writes configs/v2/<repo-id>[-S].json and seeds <runs>/<repo-id>[-S]/ with the v1 root's training
runs (state.json entries) and their transcripts, so `runner.py <config> learn eval report --execute
--root <root>` learns from the same traces and evaluates the same held-out tasks. `--thresholds
default` (the default) conditions as a user's automatic learner does (`learn.thresholds =
default`); v1 used `--demo`. The budget is the training spend already in state.json plus
`--eval-budget`.
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
    ap.add_argument("--max-turns", type=int, default=60)
    ap.add_argument("--repeats", type=int, default=2)
    ap.add_argument("--arms", default="control,adage")
    ap.add_argument("--eval-budget", type=float, default=16.0)
    ap.add_argument("--thresholds", choices=("default", "demo"), default="default")
    ap.add_argument("--model")
    ap.add_argument("--suffix")
    ap.add_argument("--concurrency", type=int, default=3)
    ap.add_argument("--from", dest="src", default=str(common.cache_dir() / "runs"))
    ap.add_argument("--runs", default="/tmp/adage-rb2/runs")
    a = ap.parse_args(argv)
    cfg = json.loads((common.HERE / "configs" / f"{a.repo}.json").read_text())
    name = a.repo + (f"-{a.suffix}" if a.suffix else "")
    old = Path(a.src) / a.repo
    root = Path(a.runs) / name
    state = json.loads((old / "state.json").read_text())
    train = {k: v for k, v in state["runs"].items() if v.get("phase") == "train"}
    train_cost = sum((r.get("metrics") or {}).get("cost_usd") or 0.0 for r in train.values())
    cfg.update(name=f"repobench2-{name}", max_turns=a.max_turns, concurrency=a.concurrency,
               budget_usd=round(train_cost + a.eval_budget, 2),
               learn_args=["--demo"] if a.thresholds == "demo" else [])
    if a.model:
        cfg["model"] = a.model
    cfg["eval"] = {**cfg["eval"], "repeats": a.repeats, "arms": a.arms.split(",")}
    out = common.HERE / "configs" / "v2" / f"{name}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(cfg, indent=1) + "\n")
    tdir = root / "private" / "transcripts"
    if not (root / "state.json").exists():
        tdir.mkdir(parents=True, exist_ok=True)
        n = 0
        for r in train.values():
            sid = (r.get("metrics") or {}).get("session_id")
            for p in (old / "private" / "transcripts").glob(f"*/{sid}.jsonl"):
                (tdir / p.parent.name).mkdir(exist_ok=True)
                shutil.copy(p, tdir / p.parent.name / p.name)
                n += 1
        seed = {"config": cfg["name"], "runs": train, "rounds": {}, "seed": state.get("seed"),
                "v1_root": str(old), "train_cost_usd": round(train_cost, 4)}
        (root / "state.json").write_text(json.dumps(seed))
        print(f"{name}: seeded {len(train)} training runs ({n} transcripts, ${train_cost:.2f}) from {old}")
    print(f"{cfg['name']}: {len(cfg['eval']['tasks'])} tasks x {a.repeats} x {cfg['eval']['arms']} "
          f"({cfg['model']}, max_turns {a.max_turns}, thresholds {a.thresholds}, budget ${cfg['budget_usd']}) -> {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
