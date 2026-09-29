#!/usr/bin/env python3
"""Validate every variant's hidden check, as RepoBench's validate.py does for mined fixes: on the
untouched base it must FAIL, with the gold patch it must PASS, in reasonable time. No tokens.

  python3 validate.py [--family F ...] [--only SUBSTR] [--out validation.json] [--work DIR]

Build caches go under /tmp/adage-showcase (Go build cache, one shared Cargo target directory), so the
footprint stays measurable and is removed with that directory. Rows are appended to the output as they
finish, so a partial run keeps what it validated; the summary counts only the latest row per task.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = Path("/tmp/adage-showcase")
os.environ.setdefault("REPOBENCH_CACHE", str(ROOT / "cache"))
os.environ.setdefault("GOCACHE", str(ROOT / "cache" / "go-build"))
os.environ.setdefault("CARGO_TARGET_DIR", str(ROOT / "cache" / "cargo-target"))
sys.path.insert(0, str(HERE))
import families  # noqa: E402
import sc_sandbox  # noqa: E402
import sc_score  # noqa: E402


def validate(tid: str, work: Path) -> dict:
    f, v = families.parse_task(tid)
    d = work / "repo"
    t0 = time.time()
    sc_sandbox.create(tid, d)
    base = sc_score.score(tid, d)
    sc_sandbox.create(tid, d)
    f["gold"](v, d)
    gold = sc_score.score(tid, d)
    row = {"task": tid, "family": f["id"], "split": "train" if tid in families.train_tasks(f["id"]) else "held-out",
           "ok": (not base["passed"]) and gold["passed"], "base_passed": base["passed"], "base_checks": base["checks"],
           "base_hidden_failed": len(base["hidden_failed"]), "gold_passed": gold["passed"], "gold_checks": gold["checks"],
           "gold_n_tests": gold["n_tests"], "gold_check_seconds": gold["elapsed_s"],
           "wall_seconds": round(time.time() - t0, 1)}
    if not gold["passed"]:
        row.update(gold_tail=gold["tail"], gold_failed=gold["hidden_failed"] + gold["suite_failed"])
    shutil.rmtree(d, ignore_errors=True)
    return row


def summarise(rows: list[dict]) -> dict:
    latest = {}
    for r in rows:
        latest[r["task"]] = r
    by = {}
    for fid in families.ORDER:
        rs = [latest[t] for t in families.order(fid) if t in latest]
        by[fid] = {"valid": sum(r["ok"] for r in rs), "of": len(families.order(fid)), "validated": len(rs),
                   "gold_check_seconds_max": max((r["gold_check_seconds"] for r in rs), default=None)}
    return {"families": by, "valid": sum(b["valid"] for b in by.values()),
            "of": sum(b["of"] for b in by.values()), "variants": [latest[t] for fid in families.ORDER
                                                                   for t in families.order(fid) if t in latest]}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--family", action="append")
    ap.add_argument("--only")
    ap.add_argument("--out", default=str(HERE / "validation.json"))
    ap.add_argument("--work", default=str(ROOT / "validate"))
    a = ap.parse_args(argv)
    out = Path(a.out)
    prior = json.loads(out.read_text())["variants"] if out.exists() else []
    rows = list(prior)
    for fid in a.family or families.ORDER:
        for tid in families.order(fid):
            if a.only and a.only not in tid:
                continue
            row = validate(tid, Path(a.work))
            rows.append(row)
            print(json.dumps(row), flush=True)
            s = summarise(rows)
            out.write_text(json.dumps(s, indent=1) + "\n")
    s = summarise(rows)
    out.write_text(json.dumps(s, indent=1) + "\n")
    print(json.dumps(s["families"], indent=1))
    print(f"{s['valid']} of {s['of']} variants valid")
    return 0 if s["valid"] == s["of"] else 1


if __name__ == "__main__":
    sys.exit(main())
