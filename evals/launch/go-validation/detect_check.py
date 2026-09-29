#!/usr/bin/env python3
"""Gate and false-fire check through the product's own hook ($0, no model calls).

  python3 detect_check.py --home <root>/private/adage-home --adage BIN [--out gate.json]

After the training sessions, the index in ADAGE_HOME is what the product learned by itself. This
feeds prompts to the INSTALLED hook exactly as Claude Code does (`adage hook claude <Event>` with the
event's JSON on stdin, in a checkout whose `origin` is unjs/ufo), on a throwaway copy of ADAGE_HOME
(so nothing here reaches the live sessions' spool), with the treated arm and learning off, as the
held-out `adage` sessions run:

  held out   the 8 held-out family prompts, verbatim as the live sessions get them
  negatives  8 same-repository prompts that are not the family (family_ufo.negatives())

For each prompt: SessionStart, UserPromptSubmit (card at the prompt?), then, for the events the
binary's `hooks-config` registers, a PostToolBatch with an Edit of each file the task edits (the
grey-zone file signal: card at the first edit of one of its files?). A card is recognised by its
route id (`card-…`) in the session's spool, and by its fixed first line in the hook's output.

Pre-registered gates (prereg.json): proceed to the held-out sessions only if the product formed a
card for github.com/unjs/ufo (R2) and the hook delivers it for at least R3_min_held_out_probe (4) of the
8 held-out prompts (R3). `delivered_tasks` fixes the pre-treatment subset of the secondary G2b. False fires on the negatives are a GO criterion (G4), not a gate.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "evals" / "live"))
import family_ufo as fam  # noqa: E402
import gv_sandbox  # noqa: E402
import runner  # noqa: E402

CARD_HEAD = "How changes of this kind were done here before"
MIN_HELD_OUT = int(json.loads((HERE / "prereg.json").read_text())["criteria"]["R3_min_held_out_probe"])


def wilson_upper(k: int, n: int, z: float = 1.6449) -> float:
    if n == 0:
        return 1.0
    p = k / n
    den = 1 + z * z / n
    c = p + z * z / (2 * n)
    r = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return round((c + r) / den, 4)


def cards_in(home: Path, repo_key: str) -> list[dict]:
    """Card routes in the compiled index (and their track files), for this repository."""
    out = []
    idx = home / "index.json"
    if idx.exists():
        data = json.loads(idx.read_text())
        for r in data.get("routes", []):
            text = json.dumps(r)
            rid = r.get("id") or (r.get("route") or {}).get("id") or ""
            if (str(rid).startswith("card-") or '"card"' in text.lower()) and repo_key.split("/")[-1] in text:
                out.append({"id": rid, "bytes": len(text)})
    if not out:  # fall back to the tracks directory
        for p in (home / "tracks").rglob("*"):
            if p.is_file() and "card-" in p.read_text(errors="replace"):
                out.append({"id": "(track file) " + str(p.relative_to(home)), "bytes": p.stat().st_size})
    return out


class Probe:
    def __init__(self, adage: Path, home: Path, work: Path, make_checkout=None):
        """`make_checkout(dir)` builds the checkout the hook runs in (default: ufo at the family's base)."""
        self.adage, self.work = adage, work
        self.home = work / "adage-home"
        shutil.copytree(home, self.home, symlinks=True)
        self.user = work / "user"
        self.user.mkdir()
        self.cwd = work / "repo"
        (make_checkout or (lambda d: gv_sandbox.create(fam.order()[0], d, setup=False)))(self.cwd)
        hooks = json.loads(subprocess.run([str(adage), "hooks-config", str(adage)], capture_output=True, text=True,
                                          check=True).stdout)["hooks"]
        self.events = sorted(hooks)

    def env(self) -> dict:
        e = {k: os.environ[k] for k in ("PATH", "LANG", "TZ") if k in os.environ}
        e.update(runner.home_dirs(self.user))
        e.update(ADAGE_HOME=str(self.home), ADAGE_ARM="treated", ADAGE_AUTO_LEARN="0")
        return e

    def hook(self, event: str, payload: dict) -> str:
        if event not in self.events:
            return ""
        out = subprocess.run([str(self.adage), "hook", "claude", event], input=json.dumps(payload), capture_output=True,
                             text=True, env=self.env(), cwd=self.cwd, timeout=60)
        return out.stdout

    def session(self, prompt: str, edits: list[str]) -> dict:
        sid = str(uuid.uuid4())
        base = {"session_id": sid, "transcript_path": str(self.user / f"{sid}.jsonl"), "cwd": str(self.cwd)}
        self.hook("SessionStart", {**base, "hook_event_name": "SessionStart", "source": "startup"})
        out = self.hook("UserPromptSubmit", {**base, "hook_event_name": "UserPromptSubmit", "prompt": prompt})
        at_prompt = CARD_HEAD in out
        at_edit = None
        for i, rel in enumerate(edits):
            call = {"tool_name": "Edit", "tool_use_id": f"toolu_probe_{i}",
                    "tool_input": {"file_path": str(self.cwd / rel), "old_string": "x", "new_string": "y"},
                    "tool_response": {"filePath": str(self.cwd / rel), "success": True}}
            o = self.hook("PostToolBatch", {**base, "hook_event_name": "PostToolBatch", "tool_calls": [call]})
            if CARD_HEAD in o and at_edit is None:
                at_edit = rel
        spool = runner.spool_directives(self.home, sid)
        routes = sorted({d["route"] for d in spool if str(d.get("route") or "").startswith("card-")
                         and d["kind"] == "directives"})
        return {"session": sid, "at_prompt": at_prompt, "at_edit": at_edit, "card_routes_in_spool": routes,
                "delivered": bool(at_prompt or at_edit or routes),
                "other_directives": sorted({d["route"] for d in spool if d["kind"] == "directives"
                                            and not str(d.get("route") or "").startswith("card-")})}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--home", required=True, help="the family root's private/adage-home")
    ap.add_argument("--adage", required=True)
    ap.add_argument("--out", default="")
    ap.add_argument("--repo-key", default="github.com/unjs/ufo")
    a = ap.parse_args(argv)
    home, adage = Path(a.home).resolve(), Path(a.adage).resolve()
    cards = cards_in(home, a.repo_key)
    res = {"home": str(home), "adage": str(adage), "cards": cards, "held_out": [], "negatives": []}
    with tempfile.TemporaryDirectory(prefix="gv-probe-") as t:
        p = Probe(adage, home, Path(t))
        res["hook_events"] = p.events
        res["file_signal_probed_via"] = "PostToolBatch" if "PostToolBatch" in p.events else None
        for tid in fam.held_out_tasks():
            r = p.session(fam.prompt(fam.variant(tid)), ["src/utils.ts", "test/utilities.test.ts", "README.md"])
            res["held_out"].append({"task": tid, **r})
        for n in fam.negatives():
            r = p.session(n["prompt"], n["edits"])
            res["negatives"].append({"id": n["id"], "source": n["source"], **r})
    k = sum(r["delivered"] for r in res["held_out"])
    ff = sum(r["delivered"] for r in res["negatives"])
    res["summary"] = {
        "cards": len(cards),
        "held_out_delivered": [k, len(res["held_out"])],
        "held_out_at_prompt": sum(r["at_prompt"] for r in res["held_out"]),
        "held_out_at_edit": sum(bool(r["at_edit"]) for r in res["held_out"]),
        "delivered_tasks": [r["task"] for r in res["held_out"] if r["delivered"]],
        "false_fires": [ff, len(res["negatives"])],
        "false_fire_rate_wilson90_upper": wilson_upper(ff, len(res["negatives"])),
        "gate_R2_card_formed": bool(cards),
        "gate_R3_detection": k >= MIN_HELD_OUT,
        "proceed_to_eval": bool(cards) and k >= MIN_HELD_OUT,
    }
    text = json.dumps(res, indent=1)
    if a.out:
        Path(a.out).write_text(text + "\n")
    print(json.dumps(res["summary"], indent=1))
    return 0 if res["summary"]["proceed_to_eval"] else 3


if __name__ == "__main__":
    sys.exit(main())
