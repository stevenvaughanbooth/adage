#!/usr/bin/env python3
"""Validate every variant's hidden check, as RepoBench's validate.py does for mined fixes: it must
FAIL on the untouched base and PASS with the gold patch (implementation, export, docs, changelog,
a test), and the whole suite must run in reasonable time. No tokens.

  python3 validate_families.py [--family F] [--work DIR]
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
import common  # noqa: E402
import sandbox  # noqa: E402

import families  # noqa: E402
import rw_score  # noqa: E402


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--family", action="append")
    ap.add_argument("--only")
    ap.add_argument("--out")
    ap.add_argument("--work", default="/tmp/adage-research/recurring-workflows/validate")
    a = ap.parse_args(argv)
    work = Path(a.work)
    out = []
    for fam in a.family or list(families.FAMILIES):
        f = families.FAMILIES[fam]
        r = common.repo(f["repo"])
        for tid in families.order(fam):
            if a.only and a.only not in tid:
                continue
            _, v = families.parse_task(tid)
            d = work / "repo"
            sandbox.create(r, f["base"], d)
            base = rw_score.score(tid, d)
            sandbox.create(r, f["base"], d)
            f["gold"](v, d)
            gold = rw_score.score(tid, d)
            ok = (not base["passed"]) and gold["passed"]
            row = {"task": tid, "ok": ok, "base_hidden_failed": base["hidden_failed"], "base_suite_failed": base["suite_failed"],
                   "gold_passed": gold["passed"], "gold_failed": gold["hidden_failed"] + gold["suite_failed"],
                   "gold_seconds": gold["elapsed_s"], "n_tests": gold["n_tests"]}
            if not gold["passed"]:
                row["gold_tail"] = gold["tail"]
            out.append(row)
            print(json.dumps(row), flush=True)
            shutil.rmtree(d, ignore_errors=True)
    (Path(a.out) if a.out else HERE / "validation.json").write_text(json.dumps(out, indent=1) + "\n")
    print(f"{sum(r['ok'] for r in out)} of {len(out)} variants valid")
    return 0


if __name__ == "__main__":
    sys.exit(main())
