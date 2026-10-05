#!/usr/bin/env python3
"""Replay manifests for `adage eval replay-savings`, from RepoBench runner roots.

  python3 evals/replay/manifest.py <runner-root>... --out DIR [--cochange] [--mirrors DIR]

For each runner root (`evals/live/runner.py --root`, e.g. /tmp/adage-rb2/runs/click) writes
`DIR/<name>.json`: the repository key, the checkout instincts bind against (the root's
`private/bind-sandbox/repo`, a fresh sandbox with its `.venv`/`node_modules`), the index that was
delivered live (`private/adage-home/index.json`), the training transcripts, and every finished
held-out session (id, task, arm, transcript, passed, cost).

`--cochange` adds the co-change oracle's table (`cochange()`): per held-out task, per source file,
the test files that changed with it most often in the repository's history before the task's base
commit (commits touching fewer than 40 files; the architecture review's §3.3 method). History
comes from the RepoBench mirrors (`--mirrors`, default $REPOBENCH_CACHE/mirrors) or, when a
repository has none, a blobless clone into that directory.

Reads only; spends nothing.
"""

from __future__ import annotations

import argparse
import collections
import fnmatch
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RB = HERE.parent / "repobench"


def repos() -> dict[str, dict]:
    return {r["id"]: r for r in json.loads((RB / "repos.json").read_text())["repos"]}


def repo_id(root: Path, state: dict) -> str:
    """The RepoBench repository a root ran: its name without a model suffix (`click-sonnet5`)."""
    name = root.name
    known = repos()
    for rid in sorted(known, key=len, reverse=True):
        if name == rid or name.startswith(rid + "-"):
            return rid
    raise SystemExit(f"{root}: not a RepoBench repository root ({name})")


def transcript_of(root: Path, rid: str, run: dict) -> Path | None:
    """The run's transcript: `transcripts/<…-runs-<rid>-repo>/<session_id>.jsonl` (a rerun leaves
    the discarded session's file beside it, so the session id decides)."""
    dirs = [d for d in (root / "transcripts").glob(f"*-{rid}-repo") if d.is_dir()]
    if not dirs:
        return None
    sid = (run.get("metrics") or {}).get("session_id")
    files = sorted(dirs[0].glob("*.jsonl"))
    for f in files:
        if sid and f.stem == sid:
            return f
    return files[0] if len(files) == 1 else None


def build(root: Path, cochange: bool = False, mirrors: Path | None = None) -> dict:
    state = json.loads((root / "state.json").read_text())
    rid = repo_id(root, state)
    r = repos()[rid]
    key = normalise_origin(r["url"])
    sessions, train = [], []
    for run_id, run in sorted(state["runs"].items()):
        if run.get("status") not in ("passed", "failed"):
            continue
        t = transcript_of(root, run_id, run)
        if t is None:
            continue
        if run.get("phase") == "train":
            train.append(str(t))
            continue
        m = run.get("metrics") or {}
        sessions.append({"id": run_id, "task": run.get("task"), "arm": run.get("arm"), "transcript": str(t),
                         "passed": m.get("passed"), "cost_usd": m.get("cost_usd"),
                         "stopped": m.get("result_subtype") == "success"})
    # A model slice (`click-sonnet5`) ran on the same repository: the base root's checkout.
    # (RepoBench v1 roots kept it as `instincts-sandbox`.)
    candidates = [root / "private" / d / "repo" for d in ("bind-sandbox", "instincts-sandbox")]
    candidates += [root.parent / rid / "private" / d / "repo" for d in ("bind-sandbox", "instincts-sandbox")]
    checkout = next((c for c in candidates if c.is_dir()), candidates[0])
    out = {"name": root.name, "repo_key": key, "root": str(checkout),
           "index": str(root / "private" / "adage-home" / "index.json"), "train": train, "sessions": sessions}
    if cochange:
        tasks = sorted({s["task"] for s in sessions})
        out["cochange"] = {"k": 3, "tasks": cochange_table(r, tasks, mirrors)}
    return out


def normalise_origin(url: str) -> str:
    u = url.rstrip("/")
    if u.endswith(".git"):
        u = u[:-4]
    return u.split("://")[-1].lower()


def matches(path: str, globs) -> bool:
    return any(fnmatch.fnmatch(path, g) for g in globs)


def is_test(r: dict, path: str) -> bool:
    return matches(path, r["test_globs"])


def is_source(r: dict, path: str) -> bool:
    return not is_test(r, path) and matches(path, r["src_globs"])


def mirror(r: dict, mirrors: Path | None) -> Path:
    d = mirrors or Path(os.environ.get("REPOBENCH_CACHE") or "/tmp/adage-repobench") / "mirrors"
    m = d / r["url"].rstrip("/").split("github.com/")[-1].replace("/", "__")
    if not (m / "HEAD").exists() and not (m / ".git").exists():
        m.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "clone", "-q", "--filter=blob:none", "--no-checkout", r["url"], str(m)], check=True)
    return m


def git(m: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(m), *args], capture_output=True, text=True, check=True).stdout


def rank(commits: list[set[str]], r: dict, present: set[str]) -> dict[str, list[str]]:
    """Source file → test files, by how many commits (of < 40 files) changed both; ties by path.
    Only test files that exist at the base commit."""
    co: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
    for c in commits:
        if len(c) >= 40:
            continue
        srcs = [f for f in c if is_source(r, f)]
        tests = [f for f in c if is_test(r, f) and f in present]
        for s in srcs:
            for t in tests:
                co[s][t] += 1
    return {s: [t for t, _ in sorted(cnt.items(), key=lambda kv: (-kv[1], kv[0]))][:10] for s, cnt in sorted(co.items())}


def cochange_table(r: dict, task_ids: list[str], mirrors: Path | None) -> dict:
    tasks = {t["id"]: t for t in json.loads((RB / "tasks" / f"{r['id']}.json").read_text())["tasks"]}
    m = mirror(r, mirrors)
    out = {}
    for tid in task_ids:
        base = tasks[tid]["base"]
        log = git(m, "log", "--name-only", "--format=format:@@", "-n", "3000", base)
        commits = [set(x.split()) for x in log.split("@@") if x.strip()]
        present = set(git(m, "ls-tree", "-r", "--name-only", base).split())
        out[tid] = rank(commits, r, present)
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("roots", nargs="+", type=Path)
    ap.add_argument("--out", type=Path, required=True)
    ap.add_argument("--cochange", action="store_true")
    ap.add_argument("--mirrors", type=Path)
    a = ap.parse_args(argv)
    a.out.mkdir(parents=True, exist_ok=True)
    for root in a.roots:
        man = build(root, a.cochange, a.mirrors)
        p = a.out / f"{root.name}.json"
        p.write_text(json.dumps(man, indent=1) + "\n")
        arms = collections.Counter(s["arm"] for s in man["sessions"])
        print(f"{p}: {len(man['sessions'])} sessions {dict(arms)}, {len(man['train'])} train"
              + (f", co-change for {len(man['cochange']['tasks'])} tasks" if a.cochange else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
