"""Small extras quoted in the README: acme (generated repo) pre-emptive vs hooks-off with task-cluster
CIs; lag from delivery to follow by hook; directive carry cost C(d)."""
import collections
import json
import pickle

import numpy as np

import data
from analysis import RNG, cluster_boot

rows, L = pickle.load(open("/tmp/adage-research/uptake-delivery/rows.pkl", "rb"))
S = data.load()
out = {}
acme = [s for s in S if s.repo == "skl-acme"]
for a, b in (("adage", "off"), ("agents-md", "off"), ("adage", "agents-md")):
    res = {}
    for key, f in (("calls", lambda s: len(s.calls)), ("usd", lambda s: s.cost), ("passed", lambda s: float(bool(s.passed)))):
        by = collections.defaultdict(lambda: ([], []))
        for s in acme:
            if s.arm in (a, b):
                by[s.task][0 if s.arm == a else 1].append(f(s))
        g = list(by.values())
        st = lambda gs: float(np.mean([x for q in gs for x in q[0]]) - np.mean([x for q in gs for x in q[1]]))
        base = np.mean([x for q in g for x in q[1]])
        res[key] = {"diff": round(st(g), 4), "ci90": cluster_boot(g, st), "base": round(float(base), 4)}
    out[f"acme_{a}_vs_{b}"] = res
lag = collections.defaultdict(list)
for r in rows:
    if r["treated"] and r["lag_learned"] is not None:
        lag[(r["hook"], r["kind"])].append(r["lag_learned"])
out["lag_to_follow_by_hook_kind"] = {f"{k[1]}@{k[0]}": {"n": len(v), "median": float(np.median(v)),
                                                        "p90": float(np.percentile(v, 90))} for k, v in sorted(lag.items())}
# carry cost of a directive: its tokens (~words*1.3) written once to cache, read by every later turn
cs = []
for r in rows:
    if r["treated"]:
        s = next(x for x in S if x.sid == r["sid"])
        tok = r["words"] * 1.3 + 40  # + the <adage-reflex> wrapper and provenance line
        p = data.PRICES[s.model]
        cs.append((tok * p[2] + tok * max(0, len(s.calls) - r["pos"]) * p[3]) / 1e6)
out["C_directive_usd_mean"] = round(float(np.mean(cs)), 6)
out["C_directive_usd_p90"] = round(float(np.percentile(cs, 90)), 6)
json.dump(out, open("results/extras.json", "w"), indent=1)
print(json.dumps(out, indent=1))

# per-repository (the leave-one-repo-out view: nothing here is fitted on the target repository
# except the judge threshold, which is fitted leave-one-repo-out)
from analysis import diff_by_task, mean_ci
per = {}
for repo in sorted({x["repo"] for x in L}):
    Lr = [x for x in L if x["repo"] == repo]
    d, ci, nt, nc = diff_by_task(Lr, "calls")
    Lc = [x for x in Lr if not x["treated"]]
    per[repo] = {"posthoc_realised_diff_calls": d, "ci90": ci, "n_t": nt, "n_c": nc,
                 "preemptive_offered_calls": round(float(np.mean([x["preempt_saved_calls"] for x in Lc])), 2) if Lc else None,
                 "preemptive_offered_ci90": mean_ci(Lc, "preempt_saved_calls") if len(Lc) > 1 else None,
                 "p_follow": round(float(np.mean([x["follow"] for x in Lr if x["treated"]])), 3),
                 "p_self": round(float(np.mean([x["follow"] for x in Lc])), 3) if Lc else None}
out["per_repo_dead_end"] = per
json.dump(out, open("results/extras.json", "w"), indent=1)
print(json.dumps(per, indent=1))
