#!/usr/bin/env python3
"""Create a RepoBench-Local sandbox: the repository at a task's base commit, ready to work in.

  python3 evals/repobench/sandbox.py <repo-id> <task-id> <dir> [--arm ARM] [--context FILE]

The sandbox is a fresh one-commit git repository holding the tree of the task's base commit (the
parent of the real fix), with `origin` set to the upstream URL, as a clone has it. It has no
history: nothing in it names the fix or the tests that check it. Then the repository's `setup`
steps give it the environment a contributor would have (a virtualenv, node_modules, the Go module
download). Setup runs here, outside the agent's view; the agent starts from the result.

`--context FILE` (the agents-md arm) adds FILE as CLAUDE.md (appended to one the repository already
has) in the base commit, as a team that keeps a context file would have it committed.
"""

from __future__ import annotations

import argparse
import io
import os
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path

import common

IDENT = ["-c", "user.name=adage-eval", "-c", "user.email=adage-eval@localhost", "-c", "commit.gpgsign=false"]


def extract(mirror: Path, rev: str, dest: Path) -> None:
    data = subprocess.run(["git", "-C", str(mirror), "archive", "--format=tar", rev], capture_output=True, check=True).stdout
    with tarfile.open(fileobj=io.BytesIO(data)) as tf:
        tf.extractall(dest, filter="tar")


def run_steps(steps, cwd: Path, timeout=600) -> None:
    env = dict(os.environ)
    for s in steps:
        cmd = s.format(cache=common.cache_dir())
        out = subprocess.run(["bash", "-c", cmd], cwd=cwd, capture_output=True, text=True, env=env, timeout=timeout)
        if out.returncode != 0:
            raise RuntimeError(f"setup step failed ({cmd[:80]}): {(out.stderr or out.stdout)[-600:]}")


def create(r: dict, base: str, dest: Path, context: str | None = None, setup: bool = True) -> Path:
    dest = Path(dest)
    if dest.exists():
        shutil.rmtree(dest)
    dest.mkdir(parents=True)
    extract(common.mirror(r), base, dest)
    if context:
        cm = dest / "CLAUDE.md"
        prior = cm.read_text() + "\n\n" if cm.exists() else ""
        cm.write_text(prior + context)
    q = ["git", "-C", str(dest)]
    subprocess.run(q + ["init", "-q", "-b", "main"], check=True)
    subprocess.run(q + ["remote", "add", "origin", r["url"]], check=True)
    subprocess.run(q + ["add", "-A", "-f", "."], check=True, capture_output=True)
    subprocess.run(q + IDENT + ["commit", "-q", "--no-verify", "-m", "Base"], check=True, capture_output=True)
    if setup:
        run_steps(r.get("setup", []), dest)
    return dest


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repo")
    ap.add_argument("task")
    ap.add_argument("dir")
    ap.add_argument("--arm", default="")
    ap.add_argument("--context", help="context file to commit as CLAUDE.md (agents-md arm)")
    a = ap.parse_args(argv)
    r = common.repo(a.repo)
    t = common.task(a.repo, a.task)
    ctx = None
    if a.context and a.arm == "agents-md":
        p = Path(a.context)
        ctx = p.read_text() if p.exists() else None
        if ctx is None:
            raise SystemExit(f"context file {p} is missing")
    create(r, t["base"], Path(a.dir), ctx)
    return 0


if __name__ == "__main__":
    sys.exit(main())
