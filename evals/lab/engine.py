"""The current Rust engine on the same split: build an `adage eval` dataset per repository from the
RepoBench transcripts, then replay (`adage eval <ds> --split chrono --thresholds default|demo`) and
mine (`adage eval mine`). The dataset (raw transcripts) goes to a scratch dir, never the repo.

  python3 evals/lab/engine.py [--out DIR] [--adage target/release/adage]
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import traces

SKIP_DIRS = {".git", ".venv", "node_modules", "__pycache__", ".pytest_cache", ".tox", "dist", "build"}


def sandbox_files(repo_dir: Path) -> list[str]:
    out = []
    for root, dirs, files in os.walk(repo_dir):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            out.append(os.path.relpath(os.path.join(root, f), repo_dir))
    return sorted(out)


def build(repo: str, sessions: list[traces.Session], out: Path, test_arms=("control",)) -> Path:
    ds = out / repo
    shutil.rmtree(ds, ignore_errors=True)
    (ds / "sessions").mkdir(parents=True)
    metas, train, test = [], [], []
    for s in sessions:
        f = f"sessions/{s.run}-{s.sid[:8]}.jsonl"
        shutil.copy(s.source, ds / f)
        metas.append({"file": f, "id": s.sid, "run": s.run, "phase": s.phase, "arm": s.arm, "task": s.task,
                      "passed": s.passed, "round": 1})
        if s.phase == "train":
            train.append(s.sid)
        elif s.arm in test_arms:
            test.append(s.sid)
    # The repository's files, from any archived checkout of one of its runs.
    src = next((Path(s.source).parent for s in sessions), None)
    files = []
    for base in traces.BASES:
        for d in sorted((base / repo / "runs").glob("*/repo"))[:1]:
            files = sandbox_files(d)
        if files:
            break
    (ds / "sandbox.txt").write_text("\n".join(files) + "\n")
    man = {"name": f"lab-{repo}", "repo": repo, "learn": "default", "sandbox": "sandbox.txt", "sessions": metas,
           "splits": {"chrono": {"train": train, "test": test,
                                 "note": "RepoBench chronological split: training traces vs held-out control arm"}},
           "default_split": "chrono"}
    (ds / "manifest.json").write_text(json.dumps(man, indent=1))
    return ds


def replay(adage: Path, ds: Path, thresholds: str) -> dict:
    r = subprocess.run([str(adage), "eval", str(ds), "--split", "chrono", "--thresholds", thresholds, "--json"],
                       capture_output=True, text=True, timeout=3600, env=dict(os.environ, ADAGE_HOME=str(ds / ".home")))
    if r.returncode != 0:
        return {"error": r.stderr[-2000:]}
    return json.loads(r.stdout)


def mine(adage: Path, ds: Path) -> dict:
    r = subprocess.run([str(adage), "eval", "mine", str(ds), "--split", "chrono", "--json"],
                       capture_output=True, text=True, timeout=3600, env=dict(os.environ, ADAGE_HOME=str(ds / ".home")))
    if r.returncode != 0:
        return {"error": r.stderr[-2000:]}
    return json.loads(r.stdout)


def summary(res: dict) -> dict:
    """What the lab reports (the full replay/mine output is large and names every trace step)."""
    out = {}
    for repo, v in res.items():
        o = {}
        for th in ("default", "demo"):
            rep = v.get(th) or {}
            dirs = [d for s in rep.get("sessions", []) for d in s.get("directives", [])]
            o[th] = {"error": rep.get("error"), "routes": len(rep.get("routes", [])),
                     "train_sessions": rep.get("train_sessions"), "scored_sessions": rep.get("scored_sessions"),
                     "directives": len(dirs), "correct": sum(d["correct"] for d in dirs)}
        m = v.get("mine") or {}
        b = (m.get("bundles") or [{}])[0]
        ho = b.get("held_out") or {}
        o["mine"] = {"top1_engine": m.get("top1_engine"), "top1_baseline": m.get("top1_baseline"),
                     "top1_order2": m.get("top1_order2"), "positions": ho.get("positions"),
                     "held_out_sessions": ho.get("sessions"),
                     "reflexes_mined_in_sample": len((b.get("learned") or {}).get("reflexes", [])),
                     "reflex_shapes_in_sample": [r.get("shape") for r in (b.get("learned") or {}).get("reflexes", [])],
                     "ece": m.get("ece"), "recall": m.get("recall")}
        out[repo] = o
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="/tmp/adage-lab-datasets")
    ap.add_argument("--adage", default=str(traces.LAB.parents[1] / "target" / "release" / "adage"))
    ap.add_argument("--results", default=str(traces.LAB / "results" / "engine.json"))
    a = ap.parse_args(argv)
    S = traces.load()
    res = {}
    for repo in traces.REPOS:
        ds = build(repo, S[repo], Path(a.out))
        res[repo] = {}
        for th in ("default", "demo"):
            rep = replay(Path(a.adage), ds, th)
            res[repo][th] = rep
            if "error" in rep:
                print(repo, th, "ERROR", rep["error"][-400:])
                continue
            dirs = [d for s in rep.get("sessions", []) for d in s.get("directives", [])]
            print(f"{repo} [{th}]: routes {len(rep.get('routes', []))}, test sessions {rep.get('scored_sessions')}, "
                  f"directives {len(dirs)}, correct {sum(d['correct'] for d in dirs)}, "
                  f"turns saved {sum(d['turns_saved'] for d in dirs)}")
        res[repo]["mine"] = mine(Path(a.adage), ds)
    Path(a.results).parent.mkdir(parents=True, exist_ok=True)
    Path(a.results).write_text(json.dumps(summary(res), indent=1, default=str))
    return 0


if __name__ == "__main__":
    sys.exit(main())
