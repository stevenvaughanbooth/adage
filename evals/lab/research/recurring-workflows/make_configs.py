#!/usr/bin/env python3
"""Runner configs (evals/live/runner.py) for the recurring-workflow families, and the
pre-registration digest.

  python3 make_configs.py [--repeats 2] [--budget 11]

Per family: train = the first TRAIN_N variants (no hooks, 1 repeat: the "earlier sessions");
eval = the remaining variants x repeats x arms control / adage / agents-md, where the agents-md
slot carries the learned procedure card (rw_sandbox.py commits cards/<family>.md as CLAUDE.md).
Claude Haiku 4.5, 60 turns, confined, host auth, concurrency 2, `adage condition --demo`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
import build  # noqa: E402
import common  # noqa: E402

import families  # noqa: E402

WORK = Path("/tmp/adage-research/recurring-workflows")
CARDS = WORK / "cards"
REL = "evals/lab/research/recurring-workflows"


def config(fam: str, repeats: int, budget: float, arms: list[str], cards: Path, suffix: str = "") -> dict:
    f = families.FAMILIES[fam]
    r = common.repo(f["repo"])
    order = families.order(fam)
    train, test = order[: families.TRAIN_N], order[families.TRAIN_N:]
    prompts = {t: f["prompt"](families.parse_task(t)[1]) for t in order}
    return {
        "name": f"rw-{fam}{suffix}",
        "repo_key": common.repo_key(r),
        "workdir": "repo",
        "sandbox": ["python3", f"{{repo}}/{REL}/rw_sandbox.py", "{task}", "{dir}", "--arm", "{arm}",
                    "--context-dir", str(cards)],
        "score": ["python3", f"{{repo}}/{REL}/rw_score.py", "{task}", "{dir}"],
        "prompts": ["{id}"],
        "task_prompts": prompts,
        "train": {"tasks": train, "repeats": 1},
        "eval": {"tasks": test, "repeats": repeats, "arms": arms},
        "seed": build.SEED,
        "model": "claude-haiku-4-5-20251001",
        "max_turns": 60,
        "concurrency": 2,
        "budget_usd": budget,
        "host_auth": True,
        "confine": True,
        "learn_args": ["--demo"],
        "classify": {"lookup": [], "regen": [], "verify": build.VERIFY, "bash_edit": build.BASH_EDIT, "check_flags": []},
        "obligation": {"paths": None},
        "labels": {"lookup": "lookup", "regen": "regenerate"},
    }


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repeats", type=int, default=2)
    ap.add_argument("--budget", type=float, default=11.0)
    a = ap.parse_args(argv)
    out = HERE / "configs"
    out.mkdir(exist_ok=True)
    digest = hashlib.sha256()
    for fam in families.FAMILIES:
        cfg = config(fam, a.repeats, a.budget, ["control", "adage", "agents-md"], CARDS)
        p = out / f"{fam}.json"
        p.write_text(json.dumps(cfg, indent=1) + "\n")
        digest.update(p.read_bytes())
        print(f"{fam}: train {cfg['train']['tasks']}\n  test {cfg['eval']['tasks']} x{a.repeats} {cfg['eval']['arms']} -> {p}")
    digest.update((HERE / "families.py").read_bytes())
    print("digest", digest.hexdigest())
    return 0


if __name__ == "__main__":
    sys.exit(main())
