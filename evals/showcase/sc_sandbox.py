#!/usr/bin/env python3
"""Sandbox for a showcase task (the live runner's `sandbox` command).

  python3 sc_sandbox.py <task-id> <dir> [--arm ARM]

The family's repository at its fixed base commit, built by RepoBench's `sandbox.create`: a one-commit
repository with the upstream `origin` (so every task of a family shares Adage's repository key, as a
user's checkouts do) and the repository's `setup` steps (a `.venv`, `go mod download`, `cargo fetch`).
Nothing is committed for any arm: the product under test delivers what it delivers through its hooks.
Mirrors and package caches live under `$REPOBENCH_CACHE` (default /tmp/adage-showcase/cache), which the
runner's confinement hides from every agent session.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CACHE = "/tmp/adage-showcase/cache"
os.environ.setdefault("REPOBENCH_CACHE", CACHE)
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "repobench"))
import common  # noqa: E402
import sandbox  # noqa: E402

import families  # noqa: E402


def create(tid: str, d: Path, setup: bool = True) -> Path:
    f, _v = families.parse_task(tid)  # an unknown task fails here, before anything is built
    r = common.repo(f["repo"])
    common.ensure_mirror(r)
    return sandbox.create(r, f["base"], Path(d), None, setup=setup)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("task")
    ap.add_argument("dir")
    ap.add_argument("--arm", default="")
    a = ap.parse_args(argv)
    create(a.task, Path(a.dir))
    return 0


if __name__ == "__main__":
    sys.exit(main())
