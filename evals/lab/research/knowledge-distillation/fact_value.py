"""Held-out value of each fact on the LORO-chosen card: marginal calls obviated per held-out session
(card minus card-without-this-fact), with a task-clustered 90% bootstrap, and the share of held-out
sessions it helped (Wilson 90%)."""
import json
import numpy as np
import kd_load, os, facts, learner
RES = "results/strict" if os.environ.get("KD_STRICT") == "1" else "results"
from common import events_for, dollars
from stats import boot, wilson

S = kd_load.load(); E = events_for(S)
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
out = {}
for r in REPOS:
    mem = [s for s in S if s.repo == r and s.arm == "train"]
    ctrl = [s for s in S if s.repo == r and s.arm == "control"]
    sel = learner.select(mem, E, learner.candidates(mem, E), m=0, budget=3000)
    full = facts.atoms(learner.card_text(sel))
    def obv(s, A):
        ks = set()
        for e in E[s.sid]:
            if facts.covers(A, e.need): ks |= set(e.calls)
        return ks
    base = {s.sid: obv(s, full) for s in ctrl}
    rows = []
    for i, it in enumerate(sel):
        A = facts.atoms(learner.card_text(sel[:i] + sel[i + 1:]))
        d = [len(base[s.sid]) - len(obv(s, A)) for s in ctrl]
        m, ci = boot(d, [s.task for s in ctrl])
        k = sum(1 for x in d if x > 0)
        rows.append(dict(fact=it.text, kind=it.kind, train_tasks=len(it.tasks), train_gain=round(it.gain, 3),
                         heldout_calls=round(m, 3), ci90=[round(ci[0], 3), round(ci[1], 3)],
                         sessions_helped=[k, len(ctrl)], wilson90=[round(x, 3) for x in wilson(k, len(ctrl))],
                         tokens=round(it.tokens)))
    rows.sort(key=lambda x: -x["heldout_calls"])
    out[r] = rows
    print(r, len(sel), "facts; >0 held-out:", sum(1 for x in rows if x["heldout_calls"] > 0))
    for x in rows[:5]:
        print("   ", x["heldout_calls"], x["ci90"], x["sessions_helped"], x["kind"], x["fact"][:150])
json.dump(out, open(RES + "/fact_value.json", "w"), indent=1)
