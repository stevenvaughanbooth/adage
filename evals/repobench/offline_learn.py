#!/usr/bin/env python3
"""Would more history have helped? Condition each repository on every trace the run produced.

  python3 evals/repobench/offline_learn.py [--runs DIR] [--curves DIR] [--out FILE]

The headline learns from the training sessions only (10 per repository). This spends nothing: it
runs `adage condition --demo` (and `--no-instincts`, so only learned reflexes count) in a scratch
ADAGE_HOME over every archived transcript of a repository: training, every held-out arm (Adage
leaves out the sessions its own directives steered) and both learning-curve sequences. Prints and
writes, per repository: traces read, used and steered; reflex candidates, compiled, routes kept;
the routes; and the most common reasons candidates were dropped.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

import common

ADAGE = common.REPO / "target" / "release" / "adage"


def transcripts(rid: str, runs: Path, curves: Path) -> list[str]:
    out = sorted(str(p) for p in (runs / rid / "private" / "transcripts").glob("*/*.jsonl"))
    out += sorted(str(p) for p in (curves / rid).glob("*/private/transcripts/*/*.jsonl"))
    return out


def learn(rid: str, files: list[str]) -> dict:
    home = Path(tempfile.mkdtemp(prefix=f"rb-offline-{rid}-", dir=str(common.cache_dir())))
    try:
        env = dict(os.environ, ADAGE_HOME=str(home), HOME=str(home))
        env.pop("CLAUDECODE", None)
        out = subprocess.run([str(ADAGE), "condition", "--demo", "--json", "--no-instincts", "--repo",
                              common.repo_key(common.repo(rid)), "--transcripts", *files],
                             env=env, capture_output=True, text=True, timeout=1800)
        if out.returncode != 0:
            return {"error": out.stderr[-500:]}
        rep = json.loads(out.stdout)
        v = next(iter(rep["repos"].values()), {})
        eng = v.get("engine") or {}
        reasons = collections.Counter((d.get("reason") or "").split(" (")[0].split('"')[0].strip() or d.get("reason")
                                      for d in eng.get("dropped", []))
        return {"traces": len(files), "read": rep.get("sessions_read"), "used": rep.get("sessions_used"),
                "steered": rep.get("sessions_steered"), "candidates": eng.get("reflexes"),
                "compiled": eng.get("compiled"), "routes": eng.get("routes"),
                "kept": [r for t in v.get("tracks", []) for r in t.get("routes", [])],
                "top_drop_reasons": reasons.most_common(5)}
    finally:
        shutil.rmtree(home, ignore_errors=True)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--runs", default=str(common.cache_dir() / "runs"))
    ap.add_argument("--curves", default=str(common.cache_dir() / "curves"))
    ap.add_argument("--out")
    ap.add_argument("repos", nargs="*")
    a = ap.parse_args(argv)
    res = {}
    for rid in a.repos or sorted(common.load_repos()):
        files = transcripts(rid, Path(a.runs), Path(a.curves))
        if not files:
            continue
        res[rid] = learn(rid, files)
        r = res[rid]
        print(f"{rid}: {r.get('traces')} traces (used {r.get('used')}, steered {r.get('steered')}); candidates "
              f"{r.get('candidates')}, compiled {r.get('compiled')}, routes {r.get('routes')}: {r.get('kept')}")
    if a.out:
        Path(a.out).write_text(json.dumps(res, indent=1) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
