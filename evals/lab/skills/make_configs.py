#!/usr/bin/env python3
"""Write the runner configs (evals/live/runner.py) for the skill lab's two live skills.

Phases are run separately: `train` (skill-steered exploration, the skill as CLAUDE.md, no hooks),
then compile.py writes private/adage-home (the compiled skill's reflexes, nothing learned), then
`eval report` with arms off (neither), agents-md (skill in context) and adage (compiled reflexes).
"""

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
LAB = "{repo}/evals/lab/skills"


def acme():
    base = json.loads((REPO / "evals/live/configs/acme-billing.json").read_text())
    base.update({
        "name": "skill-lab-acme",
        "description": "Skill lab: acme-work-item SKILL.md in context vs compiled into reflexes vs neither",
        "sandbox": ["python3", f"{LAB}/sandbox.py", "acme", f"{LAB}/skills/acme-work-item/SKILL.md", "{dir}", "{task}",
                    "{arm}"],
        "train": {"tasks": ["W-1041", "W-1042", "W-1043", "W-1044", "W-1045", "W-1046"], "repeats": 1},
        "eval": {"tasks": ["W-2041", "W-2042", "W-2043", "W-2044", "W-2045", "W-2046"], "repeats": 2,
                 "arms": ["off", "agents-md", "adage"]},
        "concurrency": 3, "budget_usd": 5.0, "host_auth": True, "confine": True, "seed": 20260929,
        "learn_args": [],
    })
    return base


def click():
    base = json.loads((REPO / "evals/repobench/configs/v2/click.json").read_text())
    base.update({
        "name": "skill-lab-click",
        "description": "Skill lab: click-bugfix SKILL.md in context vs compiled into reflexes (neither: RepoBench v2 control)",
        "sandbox": ["python3", f"{LAB}/sandbox.py", "click", f"{LAB}/skills/click-bugfix/SKILL.md", "{dir}", "{task}",
                    "{arm}"],
        "train": {"tasks": ["click-0be2bd38", "click-8533c966", "click-27aaed3f"], "repeats": 1},
        "eval": {"tasks": ["click-0cd28d65", "click-2468b709", "click-3bb230dc", "click-c3535905"], "repeats": 2,
                 "arms": ["agents-md", "adage"]},
        "concurrency": 3, "budget_usd": 7.0, "seed": 20260929, "learn_args": [],
    })
    return base


if __name__ == "__main__":
    for name, cfg in (("acme", acme()), ("click", click())):
        (HERE / "configs" / f"{name}.json").write_text(json.dumps(cfg, indent=1) + "\n")
        print("wrote", HERE / "configs" / f"{name}.json")
