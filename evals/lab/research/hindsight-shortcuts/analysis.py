#!/usr/bin/env python3
"""Where the hindsight waste is, by shortcut kind, and how much of it a library of past shortcuts
covers (perfect matcher) as memory grows. Writes breakdown.json. Uses experiment.py's setup.

  OMP_NUM_THREADS=1 nice -n 10 python3 analysis.py
"""

from __future__ import annotations

import json
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

import costs
from experiment import REPOS, Ctx, boot

HERE = Path(__file__).resolve().parent


def by_kind(s, hinst, keep=lambda h: True):
    out = {}
    allD = set()
    for fam in ("cmd", "file", "fact"):
        D = set()
        for h in hinst:
            if h.fam == fam and keep(h):
                D |= set(h.D)
        allD |= D
        out[fam] = D
    out["all"] = allD
    return out


def main():
    ctx = Ctx()
    held = [s for s in ctx.held if "sonnet" not in s.root and s.arm == "control"]
    rows = []
    for s in held:
        lab = ctx.labs[s.sid]
        hi = ctx.hinst[s.sid]
        bk = by_kind(s, hi)
        r = {"task": s.task, "calls": len(s.calls), "cost": costs.session_cost(s),
             "waste": len(lab.waste), "waste_usd": costs.saved(s, lab.waste)["usd"],
             "failed": sum(c.is_error for c in s.calls)}
        for k, D in bk.items():
            r[f"oracle_{k}"] = len(D)
            r[f"oracle_{k}_usd"] = costs.saved(s, D)["usd"]
        rows.append(r)
    out = {"sessions": len(rows)}
    for k in ["waste", "failed"] + [f"oracle_{x}" for x in ("cmd", "file", "fact", "all")]:
        out[k] = boot(rows, k)
        out[k + "_pct_calls"] = boot(rows, k, ratio_to="calls")
    for k in ["waste_usd"] + [f"oracle_{x}_usd" for x in ("cmd", "file", "fact", "all")]:
        out[k + "_pct_cost"] = boot(rows, k, ratio_to="cost")
    out["cost_per_session"] = boot(rows, "cost")
    out["calls_per_session"] = boot(rows, "calls")

    # library ceiling by kind, memory = training split, and grown memory (leave-task-out, all arms)
    mems = {r: ctx.memory([s for s in ctx.train if s.repo == r]) for r in REPOS}
    extra = [s for s in ctx.all if "sonnet" not in s.root and s.phase == "eval" and s.arm in ("adage", "agents-md")]
    grown = {}
    lib_rows = defaultdict(list)
    recur = Counter()
    for s in held:
        hi = ctx.hinst[s.sid]
        for name in ("train", "grown"):
            if name == "train":
                mem = mems[s.repo]
            else:
                k = (s.repo, s.task)
                if k not in grown:
                    grown[k] = ctx.memory([x for x in ctx.train if x.repo == s.repo] +
                                          [x for x in extra if x.repo == s.repo and x.task != s.task])
                mem = grown[k]
            cls = [c.cid for c in mem.lib.classes if mem.support(c.cid, s.task) >= 2]
            inlib = [h for h in hi if any(mem.lib.matches(c, h) for c in cls)]
            bk = by_kind(s, inlib)
            r = {"task": s.task, "calls": len(s.calls), "cost": costs.session_cost(s)}
            for k, D in bk.items():
                r[k] = len(D)
                r[k + "_usd"] = costs.saved(s, D)["usd"]
            lib_rows[name].append(r)
            if name == "train":
                for h in inlib:
                    c = next(c for c in cls if mem.lib.matches(c, h))
                    recur[(s.repo, mem.lib.classes[c].rep.kind, mem.lib.classes[c].rep.text[:90])] += len(h.D)
    out["library"] = {}
    for name, rs in lib_rows.items():
        o = {}
        for k in ("cmd", "file", "fact", "all"):
            o[k] = boot(rs, k)
            o[k + "_pct_cost"] = boot(rs, k + "_usd", ratio_to="cost")
        out["library"][name] = o
    out["grown_memory_sessions"] = {f"{k[0]}/{k[1]}": len(m.sessions) for k, m in grown.items()}
    out["top_library_classes"] = [{"repo": k[0], "kind": k[1], "text": k[2], "calls_in_D": v}
                                  for k, v in recur.most_common(25)]
    (HERE / "breakdown.json").write_text(json.dumps(out, indent=1, default=str))
    print(json.dumps({k: v for k, v in out.items() if k not in ("top_library_classes", "grown_memory_sessions")},
                     indent=1, default=str)[:6000])
    for t in out["top_library_classes"][:15]:
        print(t)


if __name__ == "__main__":
    main()
