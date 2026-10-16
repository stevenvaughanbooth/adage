#!/usr/bin/env python3
"""Validate mined candidates into RepoBench-Local tasks.

  python3 evals/repobench/validate.py <repo-id> <candidates.json> [--max N] [--jobs J] [--work DIR]

For each candidate (newest first) it builds the sandbox at the base commit (sandbox.py), writes
the fix commit's test files into it, and runs the scorer's hidden command twice:

  1. at the base: it must FAIL (the tests check the fix);
  2. with the real fix applied (every non-test file the fix commit changed): it must PASS, in
     under --max-seconds (default 60).

A candidate whose fix changes a dependency manifest or lockfile is refused: the sandbox's
environment was built from the base's. Tasks that pass are merged into tasks/<repo-id>.json,
keeping prompts already written; the verdicts, timings and failures of all candidates go to
<work>/<repo-id>.validation.json. Nothing here spends tokens.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import common
import sandbox
import score

MANIFESTS = ("package.json", "pnpm-lock.yaml", "package-lock.json", "go.mod", "go.sum", "pyproject.toml", "uv.lock",
             "setup.py", "setup.cfg", "tox.ini", "requirements")


def task_id(rid: str, sha: str) -> str:
    return f"{rid}-{sha[:8]}"


def apply_fix(r: dict, c: dict, d: Path) -> list[str]:
    """Write the fix commit's version of every non-test file it changed; returns those paths."""
    m = common.mirror(r)
    names = common.git(m, "diff", "--name-only", c["parent"], c["sha"]).split()
    gold = [p for p in names if not common.is_test(r, p)]
    for p in gold:
        blob = subprocess.run(["git", "-C", str(m), "show", f"{c['sha']}:{p}"], capture_output=True)
        dest = d / p
        if blob.returncode != 0:
            dest.unlink(missing_ok=True)
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(blob.stdout)
    return gold


def check(r: dict, c: dict, work: Path, max_seconds: float) -> dict:
    tid = task_id(r["id"], c["sha"])
    m = common.mirror(r)
    names = common.git(m, "diff", "--name-only", c["parent"], c["sha"]).split()
    tests = [p for p in names if common.is_test(r, p)]
    t = {"id": tid, "sha": c["sha"], "base": c["parent"], "date": c["date"], "subject": c["subject"],
         "src_files": c["src_files"], "test_files": tests}
    verdict = {"id": tid, "sha": c["sha"], "subject": c["subject"]}
    bad = [p for p in names if p.split("/")[-1] in MANIFESTS or p.startswith("requirements")]
    if bad:
        return {**verdict, "ok": False, "reason": f"changes manifests {bad}"}
    if not any(score.runnable(r["lang"], p) for p in tests):
        return {**verdict, "ok": False, "reason": "no runnable test module among the fix's test files"}
    d = work / tid / "repo"
    try:
        sandbox.create(r, c["parent"], d)
        score.write_hidden(r, t, d)
        base = score.run_hidden(r, t, d)
        if base["passed"]:
            return {**verdict, "ok": False, "reason": "hidden tests pass at the base", "base": base}
        gold = apply_fix(r, c, d)
        fixed = score.run_hidden(r, t, d)
        if not fixed["passed"]:
            return {**verdict, "ok": False, "reason": "hidden tests fail with the real fix", "base": base, "fixed": fixed}
        if fixed["elapsed_s"] > max_seconds:
            return {**verdict, "ok": False, "reason": f"hidden run takes {fixed['elapsed_s']}s", "fixed": fixed}
        t.update(gold_files=gold, hidden_command=fixed["command"], base_exit=base["exit_code"],
                 base_tail=base["tail"][-1500:], fixed_seconds=fixed["elapsed_s"], n_tests=fixed["n_tests"])
        return {**verdict, "ok": True, "task": t}
    except Exception as e:  # noqa: BLE001 - one broken candidate must not stop the rest
        return {**verdict, "ok": False, "reason": f"error: {e}"[:600]}
    finally:
        shutil.rmtree(work / tid, ignore_errors=True)


def merge(rid: str, tasks: list[dict]) -> Path:
    p = common.TASKS_DIR / f"{rid}.json"
    old = json.loads(p.read_text()) if p.exists() else {"repo": rid, "tasks": []}
    keep = {t["id"]: t for t in old["tasks"]}
    for t in tasks:
        prev = keep.get(t["id"], {})
        keep[t["id"]] = {**t, **{k: prev[k] for k in ("prompt", "split", "prompt_source") if k in prev}}
    r = common.repo(rid)
    out = {"repo": rid, "url": r["url"], "repo_key": common.repo_key(r),
           "tasks": sorted(keep.values(), key=lambda t: (t["date"], t["sha"]))}
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(out, indent=1) + "\n")
    return p


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("candidates")
    ap.add_argument("--max", type=int, default=40, help="candidates to try (newest first)")
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--max-seconds", type=float, default=60.0)
    ap.add_argument("--work", default=str(common.cache_dir() / "validate"))
    ap.add_argument("--only", nargs="*", help="only these commit SHAs (prefixes)")
    a = ap.parse_args(argv)
    r = common.repo(a.repo)
    cands = json.loads(Path(a.candidates).read_text())["candidates"]
    if a.only:
        cands = [c for c in cands if any(c["sha"].startswith(x) for x in a.only)]
    cands = cands[: a.max]
    work = Path(a.work)
    work.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(a.jobs) as ex:
        results = []
        for v in ex.map(lambda c: check(r, c, work, a.max_seconds), cands):
            print(f"  {v['id']}: {'OK' if v['ok'] else 'no: ' + v['reason'][:160]}  {v['subject'][:70]}", flush=True)
            results.append(v)
    (work / f"{a.repo}.validation.json").write_text(json.dumps(results, indent=1))
    good = [v["task"] for v in results if v["ok"]]
    p = merge(a.repo, good) if good else None
    print(f"{a.repo}: {len(good)} of {len(results)} candidates valid" + (f" -> {p}" if p else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
