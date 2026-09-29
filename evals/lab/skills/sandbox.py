#!/usr/bin/env python3
"""The runner's sandbox command for the skill lab: the underlying sandbox, plus the skill for skill arms.

  python3 sandbox.py acme  <SKILL.md> <dir> <task> <arm>
  python3 sandbox.py click <SKILL.md> <dir> <task> <arm>

`train` (exploration: skill-steered, no hooks) and `agents-md` (the held-out skill-in-context arm)
get the SKILL.md committed as CLAUDE.md, amended into the sandbox's single commit, so the model
reads the whole skill every turn (the upper bound of skill adherence, and of its token cost).
`off` (neither) and `adage` (compiled reflexes only) get no skill text at all.

The acme sandbox's single commit is made deterministic (fixed dates and identity), so every acme
sandbox of an arm without the skill has the same root commit and so the same repository key
(`acme-billing@<root commit>`): the key a pack `adage train` compiles against one sandbox applies
to (run_compiled.py).
"""

import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
SKILL_ARMS = ("train", "agents-md")
IDENT = ["-c", "user.name=adage-eval", "-c", "user.email=adage-eval@localhost", "-c", "commit.gpgsign=false"]
FIXED = "2026-09-01T00:00:00+0000"


def amend(d):
    """Re-make the sandbox's single commit with fixed dates and identity (a deterministic root)."""
    env = dict(os.environ, GIT_AUTHOR_DATE=FIXED, GIT_COMMITTER_DATE=FIXED)
    subprocess.run(["git", *IDENT, "commit", "-q", "--amend", "--no-edit", "--reset-author"], cwd=d, check=True,
                   capture_output=True, env=env)


def main(argv):
    kind, skill, d, task, arm = argv
    if kind == "acme":
        subprocess.run(["python3", str(REPO / "demo/sandbox/generate.py"), d], check=True, capture_output=True)
        amend(d)
    elif kind == "click":
        subprocess.run(["python3", str(REPO / "evals/repobench/sandbox.py"), "click", task, d, "--arm", "off"],
                       check=True, capture_output=True)
    else:
        raise SystemExit(f"unknown sandbox {kind}")
    if arm in SKILL_ARMS:
        cm = Path(d) / "CLAUDE.md"
        prior = cm.read_text() + "\n\n" if cm.exists() else ""
        cm.write_text(prior + Path(skill).read_text())
        subprocess.run(["git", *IDENT, "add", "CLAUDE.md"], cwd=d, check=True, capture_output=True)
        amend(d)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
