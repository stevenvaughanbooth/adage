#!/usr/bin/env python3
"""Select tasks, split them chronologically, and write the live-runner config of each repository.

  python3 evals/repobench/build.py [<repo-id> ...] [--n 20] [--train-frac 0.5] [--buffer 2]

Per repository: the latest `--n` validated tasks that have a prompt, split with the external
protocol's chronological split (evals/external/protocol.py: oldest `--train-frac` train, the next
`--buffer` discarded, the rest test; ordered by the fix's committer date). The split and its
SHA-256 digest (the pre-registration) go to splits/<repo-id>.json, and a config for
evals/live/runner.py to configs/<repo-id>.json: confined (bubblewrap), host auth, the pinned tool
set, one prompt per task, arms control / adage / agents-md / instincts, seeded arm order.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import common

sys.path.insert(0, str(common.REPO / "evals" / "external"))
import protocol  # noqa: E402

SPLITS = common.HERE / "splits"
CONFIGS = common.HERE / "configs"
CONTEXT = common.HERE / "context"
SEED = 20260928

VERIFY = ["pytest", "vitest", "go test", "make test", "npm test", "npm run test", "pnpm test", "pnpm run test",
          "pnpm vitest", "tox", "nox", "node --test", "unittest", "go vet"]
BASH_EDIT = ["sed -i", "cat >", "cat <<", "tee ", "write_text(", "patch -p", "git apply", "perl -pi"]


def select(rid: str, n: int) -> list[dict]:
    tasks = [t for t in common.load_tasks(rid)["tasks"] if t.get("prompt")]
    tasks.sort(key=lambda t: (t["date"], t["sha"]))
    return tasks[-n:]


def split(rid: str, tasks: list[dict], train_frac: float, buffer: int) -> dict:
    r = common.repo(rid)
    metas = [protocol.TaskMeta(task_id=t["id"], repo=rid, base_commit=t["sha"], gold_files=t.get("gold_files") or [],
                               test_files=t["test_files"]) for t in tasks]
    dates = {t["sha"]: t["date"] for t in tasks}
    sp = protocol.chronological_split(metas, lambda _repo, sha: dates[sha], train_frac=train_frac, buffer=buffer,
                                      min_test=5)
    if rid not in sp["repos"]:
        raise SystemExit(f"{rid}: too few tasks for a split: {sp['excluded']}")
    return {**sp, "digest": protocol.split_digest(sp), "repo_key": common.repo_key(r)}


def config(rid: str, sp: dict, tasks: list[dict], arms: list[str], train_repeats: int, eval_repeats: int,
           model: str, max_turns: int, budget: float) -> dict:
    r = common.repo(rid)
    part = sp["repos"][rid]
    prompts = {t["id"]: t["prompt"] for t in tasks}
    return {
        "name": f"repobench-{rid}",
        "repo_key": common.repo_key(r),
        "workdir": "repo",
        "sandbox": ["python3", "{repo}/evals/repobench/sandbox.py", rid, "{task}", "{dir}", "--arm", "{arm}",
                    "--context", f"{{repo}}/evals/repobench/context/{rid}.md"],
        "score": ["python3", "{repo}/evals/repobench/score.py", rid, "{task}", "{dir}"],
        "prompts": ["Fix {id}."],
        "task_prompts": {k: prompts[k] for k in part["train"] + part["test"]},
        "train": {"tasks": part["train"], "repeats": train_repeats},
        "eval": {"tasks": part["test"], "repeats": eval_repeats, "arms": arms},
        "seed": SEED,
        "model": model,
        "max_turns": max_turns,
        "concurrency": 3,
        "budget_usd": budget,
        "host_auth": True,
        "confine": True,
        "learn_args": ["--demo"],
        "classify": {"lookup": [], "regen": ["make generate", "go generate", "vitest -u", "--update-snapshots"],
                     "verify": VERIFY, "bash_edit": BASH_EDIT, "check_flags": []},
        "obligation": {"paths": None},
        "labels": {"lookup": "lookup", "regen": "regenerate"},
        "split_digest": sp["digest"],
    }


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("repos", nargs="*")
    ap.add_argument("--n", type=int, default=20)
    ap.add_argument("--train-frac", type=float, default=0.5)
    ap.add_argument("--buffer", type=int, default=2)
    ap.add_argument("--arms", default="control,adage,agents-md,instincts")
    ap.add_argument("--train-repeats", type=int, default=1)
    ap.add_argument("--eval-repeats", type=int, default=2)
    ap.add_argument("--model", default="claude-haiku-4-5-20251001")
    ap.add_argument("--max-turns", type=int, default=30)
    ap.add_argument("--budget", type=float, default=16.0)
    a = ap.parse_args(argv)
    SPLITS.mkdir(exist_ok=True)
    CONFIGS.mkdir(exist_ok=True)
    for rid in a.repos or sorted(p.stem for p in common.TASKS_DIR.glob("*.json")):
        tasks = select(rid, a.n)
        sp = split(rid, tasks, a.train_frac, a.buffer)
        (SPLITS / f"{rid}.json").write_text(json.dumps(sp, indent=1) + "\n")
        cfg = config(rid, sp, tasks, a.arms.split(","), a.train_repeats, a.eval_repeats, a.model, a.max_turns,
                     a.budget)
        (CONFIGS / f"{rid}.json").write_text(json.dumps(cfg, indent=1) + "\n")
        part = sp["repos"][rid]
        print(f"{rid}: {len(tasks)} tasks -> train {len(part['train'])}, buffer {len(part['buffer'])}, "
              f"test {len(part['test'])}; cutoff {part['cutoff'][:10]}, first test {part['first_test'][:10]}; "
              f"digest {sp['digest'][:12]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
