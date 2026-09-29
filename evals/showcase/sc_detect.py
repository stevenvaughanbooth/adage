#!/usr/bin/env python3
"""Gate and false-fire probe through the product's own hook, per family ($0, no model calls).

  python3 sc_detect.py --family F --home <root>/private/adage-home --adage BIN [--out <root>/gate.json]

The GO validation's probe (evals/launch/go-validation/detect_check.py `Probe`, imported unchanged) on a
throwaway copy of the family's learned ADAGE_HOME, in a checkout of the family's repository (origin set,
so the repository key is the users'), treated arm, learning off, as the held-out `adage` sessions run:

  held out   the family's 8 held-out prompts, verbatim as the live sessions get them
  negatives  its 6 same-repository negative prompts (families.negatives())

Each prompt: SessionStart, UserPromptSubmit (card at the prompt?), then a PostToolBatch with an Edit of
each file such a task edits (card at the first edit?). A card is recognised by its route id in the spool
and by its first line in the hook's output.

Prereg (prereg.json): the gate is REPORTED, not a stop (the showcase measures the product as shipped,
misses included). `delivered_tasks` fixes, before any held-out session, the pre-treatment subset of the
delivered-subset secondary. The negatives give the false-fire secondary.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import sc_sandbox  # noqa: E402,F401  (sets REPOBENCH_CACHE before the GO modules do)
import families as F  # noqa: E402


def load_dc():
    if "gv_detect_check" in sys.modules:
        return sys.modules["gv_detect_check"]
    gv = HERE.parent / "launch" / "go-validation"
    saved = list(sys.path)
    sys.path.insert(0, str(gv))
    try:
        spec = importlib.util.spec_from_file_location("gv_detect_check", gv / "detect_check.py")
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
    finally:  # detect_check adds its own directory (with its own analyze.py) to sys.path: undo it
        sys.path[:] = saved
    sys.modules["gv_detect_check"] = mod
    return mod


def probe(fid: str, home: Path, adage: Path) -> dict:
    dc = load_dc()
    f = F.FAMILIES[fid]
    cards = dc.cards_in(home, f["repo_key"])
    res = {"family": fid, "home": str(home), "adage": str(adage), "cards": cards, "held_out": [], "negatives": []}
    with tempfile.TemporaryDirectory(prefix="sc-probe-") as t:
        p = dc.Probe(adage, home, Path(t), lambda d: sc_sandbox.create(F.order(fid)[0], d, setup=False))
        res["hook_events"] = p.events
        for tid in F.held_out_tasks(fid):
            res["held_out"].append({"task": tid, **p.session(F.prompt(tid), F.probe_edits(tid))})
        for n in F.negatives(fid):
            res["negatives"].append({"id": n["id"], "source": n["source"], **p.session(n["prompt"], n["edits"])})
    k = sum(r["delivered"] for r in res["held_out"])
    ff = sum(r["delivered"] for r in res["negatives"])
    res["summary"] = {
        "cards": len(cards), "card_formed": bool(cards),
        "held_out_delivered": [k, len(res["held_out"])],
        "held_out_at_prompt": sum(r["at_prompt"] for r in res["held_out"]),
        "held_out_at_edit": sum(bool(r["at_edit"]) for r in res["held_out"]),
        "delivered_tasks": [r["task"] for r in res["held_out"] if r["delivered"]],
        "false_fires": [ff, len(res["negatives"])],
        "false_fire_ids": [r["id"] for r in res["negatives"] if r["delivered"]],
        "false_fires_at_prompt": sum(bool(r["at_prompt"]) for r in res["negatives"]),
        "false_fires_at_edit_only": sum(bool(r["delivered"] and not r["at_prompt"]) for r in res["negatives"]),
        "false_fire_rate_wilson90_upper": dc.wilson_upper(ff, len(res["negatives"])),
    }
    return res


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--family", required=True, choices=F.ORDER)
    ap.add_argument("--home", required=True)
    ap.add_argument("--adage", required=True)
    ap.add_argument("--out", default="")
    a = ap.parse_args(argv)
    res = probe(a.family, Path(a.home).resolve(), Path(a.adage).resolve())
    if a.out:
        Path(a.out).write_text(json.dumps(res, indent=1) + "\n")
    print(json.dumps(res["summary"], indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
