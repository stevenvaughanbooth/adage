#!/usr/bin/env python3
"""Analysis of the confirming experiment (prereg.json here). $0, offline.

  OMP_NUM_THREADS=1 nice -n 10 python3 analyze_confirm.py

live: card-by-hook vs control on go-flag, paired by (variant, repeat), $ / calls / turns ratio - 1
with a 90% bootstrap over variants, pass rates; the card-from-1-session arm vs the same variants'
controls; the hook's own log (what it decided, in both arms: detection recall on the held-out
prompts as run live, and delivery) ; uptake by role (instrument taxonomy of ../analyze.py).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import analyze  # noqa: E402

RUNS = Path("/tmp/adage-research/recurring-workflows/confirm/runs")


def hook_log(root: Path) -> dict:
    out = {}
    for d in sorted((root / "runs").glob("eval-*")):
        p = d / "home" / ".rw-card" / "log.jsonl"
        recs = [json.loads(x) for x in p.read_text().splitlines()] if p.exists() else []
        out[d.name] = recs
    return out


def main() -> int:
    analyze.RUNS = RUNS
    main_ss = analyze.load(["go-flag"])
    n1_ss = analyze.load(["go-flag-card1"])
    res = {"live": analyze.live(main_ss), "uptake": analyze.uptake(main_ss)}
    # card from 1 session vs the same variants' controls (both repeats)
    ctrl = {}
    for s in main_ss:
        if s.phase == "eval" and s.arm == "control":
            ctrl.setdefault(s.task, []).append(s)
    card4 = {}
    for s in main_ss:
        if s.phase == "eval" and s.arm == "agents-md":
            card4.setdefault(s.task, []).append(s)
    curve = {"n0_control_usd": round(float(np.mean([x.cost_usd for v in ctrl.values() for x in v])), 4)}
    for label, arm in (("n1_card_hook", {s.task: [s] for s in n1_ss if s.phase == "eval"}), ("n4_card_hook", card4)):
        ts = sorted(set(arm) & set(ctrl))
        if ts:
            curve[label] = {
                "usd": analyze.boot_ratio([(t, float(np.mean([x.cost_usd for x in arm[t]])),
                                            float(np.mean([x.cost_usd for x in ctrl[t]]))) for t in ts]),
                "calls": analyze.boot_ratio([(t, float(np.mean([x.n_calls for x in arm[t]])),
                                              float(np.mean([x.n_calls for x in ctrl[t]]))) for t in ts]),
                "sessions": sum(len(arm[t]) for t in ts), "passed": sum(x.passed for t in ts for x in arm[t])}
    res["live_curve"] = curve
    # the hook's decisions
    logs = {**hook_log(RUNS / "go-flag"), **{("n1:" + k): v for k, v in hook_log(RUNS / "go-flag-card1").items()}}
    dec = {"sessions": len(logs), "fired_at_prompt": 0, "fired_by_file": 0, "grey": 0, "none": 0, "errors": 0,
           "delivered_in_deliver_mode": 0, "scores": [], "hook_ms": []}
    for run, recs in logs.items():
        ups = [r for r in recs if r.get("event") == "UserPromptSubmit"]
        dec["errors"] += sum(r.get("event") == "error" for r in recs)
        if not ups:
            dec["none"] += 1
            continue
        u = ups[0]
        dec["scores"].append(u["score"])
        dec["hook_ms"].append(u.get("ms"))
        dec["fired_at_prompt"] += u["decision"] == "fire"
        dec["grey"] += u["decision"] == "grey"
        dec["none"] += u["decision"] == "none"
        dec["fired_by_file"] += any(r.get("decision") == "fire-file" for r in recs)
        dec["delivered_in_deliver_mode"] += u["mode"] == "deliver" and (u["decision"] == "fire" or any(
            r.get("decision") == "fire-file" for r in recs))
    dec["live_recall_on_held_out_prompts"] = [dec["fired_at_prompt"] + dec["fired_by_file"], dec["sessions"]]
    res["hook"] = dec
    out = HERE / "analysis.json"
    out.write_text(json.dumps(res, indent=1, default=str))
    print(json.dumps({k: v for k, v in res.items()}, indent=1, default=str)[:9000])
    return 0


if __name__ == "__main__":
    sys.exit(main())
