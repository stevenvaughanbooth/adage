#!/usr/bin/env python3
"""Sandbox for a GO-validation family task (the live runner's `sandbox` command).

  python3 gv_sandbox.py <task-id> <dir> [--arm ARM]

unjs/ufo at the family's fixed base commit, built by RepoBench's `sandbox.create`: a one-commit
repository with the upstream `origin` (so every task shares Adage's repository key
`github.com/unjs/ufo`, as a user's checkouts do) and `pnpm install` from the local store, as a
contributor following the README would have it. Nothing is committed for any arm: the product under
test delivers everything it delivers through its hooks. The mirror and the pnpm store live under
`$REPOBENCH_CACHE` (default here: /tmp/adage-launch/go-validation/cache), which the runner's
confinement hides from every agent session.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CACHE = "/tmp/adage-launch/go-validation/cache"
os.environ.setdefault("REPOBENCH_CACHE", CACHE)
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1] / "repobench"))
import common  # noqa: E402
import sandbox  # noqa: E402

import family_ufo as fam  # noqa: E402


def ensure_mirror() -> Path:
    """The blobless ufo mirror (clone it on first use, as RepoBench does)."""
    return common.ensure_mirror(common.repo(fam.FAMILY["repo"]))


def create(tid: str, d: Path, setup: bool = True) -> Path:
    fam.variant(tid)  # an unknown task fails here, before anything is built
    ensure_mirror()
    return sandbox.create(common.repo(fam.FAMILY["repo"]), fam.UFO_BASE, Path(d), None, setup=setup)


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
