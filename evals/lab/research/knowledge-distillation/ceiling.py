"""Oracle ceilings of the lever on held-out control sessions (its size, before any learning).

(a) every discovery event obviated (all information the session had to find out, incl. task-specific);
(b) durable oracle: events whose needed atoms were also discovered in sessions of >= 2 *other tasks*
    of the same repository (train + held-out control, leave-this-session-out): a perfect card of recurring
    knowledge, no budget;
(c) train-learnable: needed atoms all discovered somewhere in the repository's training sessions.
Savings in calls and in $ (cumulative-context model, see common.dollars), as fractions of the session.
"""
import json
from collections import defaultdict, Counter
import numpy as np
import kd_load, facts, kd_embed
from common import dollars, events_for
from stats import boot

S = kd_load.load()
ctrl = [s for s in S if s.arm == "control"]
train = [s for s in S if s.arm == "train"]
E = events_for(S)

out = {}
tab = defaultdict(list)
for repo in sorted({s.repo for s in S}):
    pool = [s for s in S if s.repo == repo and s.arm in ("train", "control")]
    per = {}
    tasks_of = defaultdict(set)  # atom -> tasks whose sessions discovered it
    for s in pool:
        A = set()
        for e in E[s.sid]:
            A |= e.need
        per[s.sid] = A
        for a in A:
            tasks_of[a].add(s.task)
    trainA = set().union(*[per[s.sid] for s in pool if s.arm == "train"])
    for s in [x for x in ctrl if x.repo == repo]:
        ev = E[s.sid]
        allc = set(k for e in ev for k in e.calls)
        dur = set(k for e in ev if all(len(tasks_of[a] - {s.task}) >= 2 for a in e.need) for k in e.calls)
        lrn = set(k for e in ev if e.need <= trainA for k in e.calls)
        ch = set(k for e in ev if e.kind == "chain" for k in e.calls)
        durch = set(k for e in ev if e.kind == "chain" and all(len(tasks_of[a] - {s.task}) >= 2 for a in e.need) for k in e.calls)
        n, c = len(s.calls), s.cost
        tab[repo].append(dict(task=s.task, n=n, cost=c, all=len(allc), all_d=dollars(s, allc), dur=len(dur),
                              dur_d=dollars(s, dur), lrn=len(lrn), lrn_d=dollars(s, lrn), chain=len(ch),
                              durch=len(durch), durch_d=dollars(s, durch)))
allrows = [r for v in tab.values() for r in v]
for repo, rows in list(tab.items()) + [("all", allrows)]:
    d = {}
    for k in ("all", "dur", "lrn", "chain", "durch"):
        m, ci = boot([r[k] for r in rows], [r["task"] for r in rows])
        d[k] = (m, ci)
    fr = {k: sum(r[k + "_d"] for r in rows) / sum(r["cost"] for r in rows) for k in ("all", "dur", "lrn", "durch")}
    frc = {k: sum(r[k] for r in rows) / sum(r["n"] for r in rows) for k in ("all", "dur", "lrn", "chain", "durch")}
    out[repo] = dict(n=len(rows), calls_per_session=np.mean([r["n"] for r in rows]), usd_per_session=np.mean([r["cost"] for r in rows]),
                     calls=d, frac_calls=frc, frac_usd=fr)
    print(f"{repo:12s} n={len(rows):3d} calls/s={out[repo]['calls_per_session']:.1f} $/s={out[repo]['usd_per_session']:.3f}")
    for k in d:
        print(f"    {k:6s} {d[k][0]:5.2f} [{d[k][1][0]:.2f},{d[k][1][1]:.2f}] calls  frac calls {frc[k]:.1%}  frac $ {fr.get(k, float('nan')):.1%}")
json.dump(out, open("results/ceiling.json", "w"), indent=1, default=float)
