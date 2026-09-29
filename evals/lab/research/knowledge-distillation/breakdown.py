"""Measurement instrument (hand-written, declared): what kind of call each method claims to obviate.
Classes by the call's tool name and whether it failed: failed call | Read | Grep/Glob | other Bash."""
import pickle, json
from collections import Counter
import kd_load, os
RES = "results/strict" if os.environ.get("KD_STRICT") == "1" else "results"
S = {s.sid: s for s in kd_load.load()}
P = pickle.load(open(kd_load.CACHE / ("per-strict.pkl" if os.environ.get("KD_STRICT") == "1" else "per.pkl"), "rb"))
ctrl = {}
for s in S.values():
    if s.arm == "control":
        ctrl.setdefault(s.repo, []).append(s)
def cls(c):
    if c.error: return "failed"
    if c.tool == "Read": return "Read"
    if c.tool in ("Grep", "Glob"): return "Grep/Glob"
    if c.tool == "Bash": return "Bash-ok"
    return c.tool
out = {}
import sys
keys = sys.argv[1:] or ["card|(0, 3000)", "card|(1, 400)", "agents-md|()", "oracle-train|()", "ctx-pre|(0, 0.7)"]
for key in keys:
    cnt = Counter(); n = 0
    for repo, ss in ctrl.items():
        rows = P["per"][f"{repo}|{key}"]
        for s, r in zip(ss, rows):
            n += 1
            for k in r["obv"]:
                cnt[cls(s.calls[k])] += 1
    out[key] = {k: round(v / n, 3) for k, v in cnt.items()}
    print(key, out[key])
json.dump(out, open(RES + "/breakdown.json", "w"), indent=1)
