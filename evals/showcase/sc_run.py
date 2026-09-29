#!/usr/bin/env python3
"""The showcase's runner: the GO validation's driver (evals/launch/go-validation/gv_run.py, imported
unchanged), which is the unchanged live runner (evals/live/runner.py) plus training as a user, the
learner wait, the global spend cap, `--eval-repeats` and the guards.

  python3 sc_run.py <config.json> <phases...> [--execute] --host-auth --root R --adage BIN [--eval-repeats N]

One addition, for the Sonnet arm's configs (`go_validation.seed_home_from: <Haiku root>`):
  - the root's ADAGE_HOME (private/adage-home) is a copy of the Haiku root's, made once, before the first
    eval session: the index the product learned from that family's 4 Haiku training sessions. Its
    learning stays frozen in eval (ADAGE_AUTO_LEARN=0), as in every held-out session;
  - it refuses to start before that home has a learned index (index.json), and refuses the `train` and
    `learn` phases: the Sonnet arm never trains and the harness never conditions.

The global cap ($60, prereg R1) spans every showcase root: gv_run.py sums what they recorded before each
`--execute` and clamps this runner's cap to what remains.
"""

from __future__ import annotations

import importlib.util
import json
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
GV = HERE.parent / "launch" / "go-validation" / "gv_run.py"


def load_gv():
    spec = importlib.util.spec_from_file_location("gv_run", GV)
    mod = importlib.util.module_from_spec(spec)
    sys.modules["gv_run"] = mod
    spec.loader.exec_module(mod)
    return mod


def seed_home(src_root: Path, root: Path) -> dict:
    """Copy the Haiku root's learned ADAGE_HOME into this root, once."""
    src, dst = src_root / "private" / "adage-home", root / "private" / "adage-home"
    if dst.exists():
        return {"seeded": False, "why": "already seeded", "home": str(dst)}
    if not (src / "index.json").exists():
        raise SystemExit(f"{src} has no learned index yet: run the family's Haiku training first (prereg stage A)")
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copytree(src, dst, symlinks=True, ignore=shutil.ignore_patterns("*.lock"))
    rec = {"seeded": True, "from": str(src), "home": str(dst)}
    (root / "private" / "seeded-home.json").write_text(json.dumps(rec, indent=1) + "\n")
    return rec


def main(argv: list[str]) -> int:
    gv = load_gv()
    cpath = gv.config_path(argv)
    cfg = json.loads(Path(cpath).read_text())
    g = cfg.get("go_validation") or {}
    if g.get("seed_home_from"):
        phases = [a for a in argv if a in ("train", "learn", "eval", "report")]
        if {"train", "learn"} & set(phases):
            raise SystemExit("the Sonnet arm runs eval (and report) only, on the Haiku-learned index")
        if "--execute" in argv and "eval" in phases:
            print(f"home: {json.dumps(seed_home(Path(g['seed_home_from']), gv.root_of(cfg, argv)))}", flush=True)
    return gv.main(argv)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
