#!/usr/bin/env python3
"""Write the GO validation's runner configs (no tokens):

  configs/ufo-util.json          the family: 4 training sessions (the user's history, product
                                 installed, learning by itself), then 8 held-out variants x 2
                                 repeats x {control, adage}, paired by (variant, repeat)
  configs/click-regression.json  the RepoBench regression slice: the 6 held-out click tasks of
                                 click-a3slice, 1 repeat x {control, adage}, conditioned on the same
                                 10 archived training sessions (no new training), without outcomes

  python3 make_configs.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(HERE))
import family_ufo as fam  # noqa: E402

RUNS = "/tmp/adage-launch/go-validation/runs"
FAMILY_ROOT = f"{RUNS}/ufo-util"
REGRESSION_ROOT = f"{RUNS}/click-regression"
CLICK_HISTORY = "/tmp/adage-repobench/runs/click"
MODEL = "claude-haiku-4-5-20251001"
SEED = 20260929
GLOBAL_CAP = 15.0
FAMILY_CAP = 9.5
REGRESSION_EVAL_CAP = 4.0

VERIFY = ["pytest", "vitest", "go test", "make test", "npm test", "npm run test", "pnpm test", "pnpm run test",
          "pnpm vitest", "tox", "nox", "node --test", "unittest", "go vet", "eslint", "prettier -c", "pnpm lint",
          "pnpm run lint"]
BASH_EDIT = ["sed -i", "cat >", "cat <<", "tee ", "write_text(", "patch -p", "git apply", "perl -pi"]


def family_config() -> dict:
    return {
        "name": "go-validation-ufo-util",
        "description": "GO validation of the shipped product on a fresh recurring family (unjs/ufo: add a URL "
                       "utility). Training = the user's history with the product installed and learning by "
                       "itself; held out = control (hooks, directives withheld) vs adage (the full product). "
                       "See prereg.json.",
        "repo_key": "github.com/unjs/ufo",
        "workdir": "repo",
        "sandbox": ["python3", "{repo}/evals/launch/go-validation/gv_sandbox.py", "{task}", "{dir}", "--arm", "{arm}"],
        "score": ["python3", "{repo}/evals/launch/go-validation/gv_score.py", "{task}", "{dir}"],
        "prompts": ["{id}"],
        "task_prompts": {t: fam.prompt(fam.variant(t)) for t in fam.order()},
        "train": {"tasks": fam.train_tasks(), "repeats": 1},
        "eval": {"tasks": fam.held_out_tasks(), "repeats": 2, "arms": ["control", "adage"]},
        "seed": SEED,
        "model": MODEL,
        "max_turns": 60,
        "concurrency": 2,
        "budget_usd": FAMILY_CAP,
        "host_auth": True,
        "confine": True,
        "learn_args": [],
        "classify": {"lookup": [], "regen": ["automd", "pnpm build", "pnpm run build"], "verify": VERIFY,
                     "bash_edit": BASH_EDIT, "check_flags": []},
        # Instrument only: was the README regenerated (automd) after the last edit of the sources?
        "obligation": {"paths": "src/*.ts"},
        "labels": {"lookup": "lookup", "regen": "automd"},
        "go_validation": {
            "train_as_user": True,
            "learner_wait_s": 900,
            "global_cap_usd": GLOBAL_CAP,
            "spend_roots": [FAMILY_ROOT, REGRESSION_ROOT],
        },
    }


def regression_config() -> dict:
    src = json.loads((REPO / "evals/repobench/configs/v2/click-a3slice.json").read_text())
    state = json.loads((Path(CLICK_HISTORY) / "state.json").read_text())
    train_cost = sum((r.get("metrics") or {}).get("cost_usd") or 0.0 for r in state["runs"].values()
                     if r.get("phase") == "train" and r["task"] in src["train"]["tasks"])
    cfg = dict(src)
    cfg.update({
        "name": "go-validation-click-regression",
        "description": "Regression slice of the GO validation: click-a3slice's 6 held-out one-off bug fixes, "
                       "control vs adage (the product under test), conditioned without outcomes on the same 10 "
                       "archived training sessions. Checks that uplift gating and procedure cards do no harm on "
                       "one-off work. See prereg.json.",
        "model": MODEL,
        "seed": SEED,
        "max_turns": 40,
        "concurrency": 2,
        "eval": {**src["eval"], "repeats": 1, "arms": ["control", "adage"]},
        "budget_usd": round(train_cost + REGRESSION_EVAL_CAP, 2),
        "host_auth": True,
        "confine": True,
        "learn_args": [],
        "go_validation": {
            "learn_outcomes": False,
            "seed_train_from": CLICK_HISTORY,
            "min_remaining_usd": 3.0,
            "global_cap_usd": GLOBAL_CAP,
            "spend_roots": [FAMILY_ROOT, REGRESSION_ROOT],
        },
    })
    cfg.pop("split_digest", None)
    cfg["based_on"] = {"config": "evals/repobench/configs/v2/click-a3slice.json",
                       "split_digest": src.get("split_digest"), "seeded_train_cost_usd": round(train_cost, 4)}
    return cfg


def main() -> int:
    out = HERE / "configs"
    out.mkdir(exist_ok=True)
    for name, cfg in (("ufo-util", family_config()), ("click-regression", regression_config())):
        p = out / f"{name}.json"
        p.write_text(json.dumps(cfg, indent=1) + "\n")
        print(f"{p.relative_to(REPO)}: train {len(cfg['train']['tasks'])}, eval {len(cfg['eval']['tasks'])} x "
              f"{cfg['eval']['repeats']} x {cfg['eval']['arms']}, cap ${cfg['budget_usd']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
