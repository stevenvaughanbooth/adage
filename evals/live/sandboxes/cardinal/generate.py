#!/usr/bin/env python3
"""Generate one fresh, deterministic git checkout; never modifies an existing nonempty path."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from cases import ORIGIN, files, task, tasks


def generate(out, task_id):
    spec = task(task_id)
    out = Path(out)
    if out.exists() and (not out.is_dir() or any(out.iterdir())):
        raise ValueError(f"output is not an empty directory: {out}")
    out.mkdir(parents=True, exist_ok=True)
    for name, body in files(spec).items():
        path = out / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
               GIT_AUTHOR_NAME="Cardinal Fixture", GIT_AUTHOR_EMAIL="fixture@adage.invalid",
               GIT_COMMITTER_NAME="Cardinal Fixture", GIT_COMMITTER_EMAIL="fixture@adage.invalid",
               GIT_AUTHOR_DATE="2026-10-01T00:00:00+00:00", GIT_COMMITTER_DATE="2026-10-01T00:00:00+00:00")
    for args in (["init", "-q"], ["remote", "add", "origin", ORIGIN], ["add", "."],
                 ["commit", "-q", "-m", f"Cardinal {task_id}"]):
        subprocess.run(["git", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", *args],
                       cwd=out, env=env, capture_output=True, check=True)
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out")
    p.add_argument("--task")
    p.add_argument("--list", action="store_true", help="print the external split/group manifest")
    args = p.parse_args()
    if args.list:
        print(json.dumps(tasks(), indent=2))
    elif args.out and args.task:
        generate(args.out, args.task)
        print(json.dumps({"generated": args.task, "out": args.out}))
    else:
        p.error("provide --out and --task, or --list")


if __name__ == "__main__":
    main()
