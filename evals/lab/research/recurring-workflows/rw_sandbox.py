#!/usr/bin/env python3
"""Sandbox for a recurring-workflow task (the live runner's `sandbox` command).

  python3 rw_sandbox.py <task-id> <dir> [--arm ARM] [--context-dir DIR]

The family's repository at its fixed base commit, built by RepoBench's `sandbox.create` (one-commit
repository, upstream `origin`, the project `.venv`). For the `agents-md` arm slot (used here for the
**procedure card** arm) the family's card `<context-dir>/<family>.md` is committed as CLAUDE.md.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
import common  # noqa: E402
import sandbox  # noqa: E402

import families  # noqa: E402


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("task")
    ap.add_argument("dir")
    ap.add_argument("--arm", default="")
    ap.add_argument("--context-dir", default="")
    a = ap.parse_args(argv)
    f, _v = families.parse_task(a.task)
    ctx = None
    if a.arm == "agents-md":
        fam = a.task.split("--")[0]
        p = Path(a.context_dir) / f"{fam}.md"
        if not p.exists():
            raise SystemExit(f"procedure card {p} is missing")
        ctx = p.read_text()
    sandbox.create(common.repo(f["repo"]), f["base"], Path(a.dir), ctx)
    return 0


if __name__ == "__main__":
    sys.exit(main())
