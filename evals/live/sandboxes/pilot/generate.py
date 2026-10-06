#!/usr/bin/env python3
"""Generate a fresh bounded pilot checkout, or print its external split/group registry."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from cases import ORIGIN, dump, manifest, task
from project import artifact
from safe_python import evaluate

HERE = Path(__file__).resolve().parent
FIXED_DATE = "2026-10-01T00:00:00+00:00"


def files(spec):
    source = spec["buggy"]
    initial = artifact(source, spec["visible"], evaluate(source, [x["input"] for x in spec["visible"]]))
    result = {
        ".gitignore": "__pycache__/\n*.pyc\n.adage/\n",
        "README.md": "# Client examples service\n\nThe Python helper in `src/solution.py` supplies JSON-only behavior. `generated/client-examples.json` is consumed by a static client; keep it current with the helper.\n\nAfter source edits, run `make generate`, then `make check`. The checks run visible acceptance examples and verify the generated artifact matches the current source. Python 3, Make, and Git are enough; no package installation or network service is needed.\n\nRead TASK.md for the issue. Preserve the existing verification tools and acceptance examples.\n",
        "TASK.md": f"# {spec['id']}: {spec['label']}\n\n{spec['description']}\n\nFix `solve(value)` in `src/solution.py` and keep the generated client examples current. The input and output must be JSON values. This bounded fixture supports pure Python built-ins and ordinary string/list/dict methods only: no imports, I/O, reflection, dynamic evaluation, decorators, type annotations, or extra top-level code. Keep the existing function signature and use the project commands in README.md.\n",
        "examples.json": dump(spec["visible"]),
        "src/solution.py": source,
        "generated/client-examples.json": dump(initial),
        "tools/project.py": (HERE / "project.py").read_text(encoding="utf-8"),
        "tools/safe_python.py": (HERE / "safe_python.py").read_text(encoding="utf-8"),
        "Makefile": ".PHONY: generate check\ngenerate:\n\tpython3 tools/project.py generate\ncheck:\n\tpython3 tools/project.py check\n",
    }
    if spec["kind"] == "documentation":
        result["TASK.md"] = f"# {spec['id']}: {spec['label']}\n\n{spec['description']}\n\nThis is a documentation correction. The helper and generated client examples already behave correctly for this request.\n"
        result["docs/guide.md"] = spec["doc_before"]
    return result


def generate(out, task_id):
    spec = task(task_id)
    out = Path(out)
    if out.exists() and (not out.is_dir() or any(out.iterdir())):
        raise ValueError(f"output must be an empty directory: {out}")
    out.mkdir(parents=True, exist_ok=True)
    for name, body in files(spec).items():
        path = out / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
               GIT_AUTHOR_NAME="Adage Pilot", GIT_AUTHOR_EMAIL="pilot@adage.invalid",
               GIT_COMMITTER_NAME="Adage Pilot", GIT_COMMITTER_EMAIL="pilot@adage.invalid",
               GIT_AUTHOR_DATE=FIXED_DATE, GIT_COMMITTER_DATE=FIXED_DATE)
    for args in (["init", "-q"], ["remote", "add", "origin", ORIGIN], ["add", "."],
                 ["commit", "-q", "-m", f"Pilot {task_id}"]):
        subprocess.run(["git", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false",
                        "-c", "core.hooksPath=/dev/null", *args], cwd=out, env=env,
                       capture_output=True, check=True, timeout=15)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out")
    parser.add_argument("--task")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()
    if args.list:
        print(dump(manifest()), end="")
    elif args.out and args.task:
        generate(args.out, args.task)
        print(json.dumps({"generated": args.task, "out": args.out}))
    else:
        parser.error("provide --out and --task, or --list")


if __name__ == "__main__":
    main()
