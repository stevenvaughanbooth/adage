"""Diagnostics: the full threshold grid, pooled (in-sample over test sessions: NOT a result)."""
import sys, time
from collections import defaultdict
import numpy as np
import run
from casebank import Memory, Params
from score import Scorer

def grid(N="all", prm=Params(), targets=("next", "skip", "avoid", "prior", "labprior", "summary"), other=False, seed=0):
    C = run.corpus(); S = C.sessions
    tests = [i for i, s in enumerate(S) if s.arm == "control"]
    by_task = defaultdict(list)
    for i in tests: by_task[S[i].task].append(i)
    cands = {}
    import random
    for task, ts in by_task.items():
        b = S[ts[0]].base
        p = [i for i, s in enumerate(S) if s.base == b and s.task != task]
        if N != "all":
            if N > len(p): continue
            p = random.Random(f"{task}-{N}-{seed}").sample(p, N)
        if other: p += [i for i, s in enumerate(S) if s.base != b]
        M = Memory(C, p, prm)
        cands.update(run.candidates(C, M, ts, Scorer(C)))
    sc = Scorer(C)
    for tg in targets:
        g = run.score_grid(C, sc, cands, tg)
        print("==", tg)
        for th, per in g.items():
            v = list(per.values())
            iss = sum(x["issued"] for x in v); use = sum(x["useful"] for x in v)
            if iss == 0: continue
            print(th, "iss/s %.2f prec %.2f strict %.2f wrong %.2f saved %.2f disc %.2f loose %.2f net %.2f harmnet %.2f" % (
                iss/len(v), use/iss, sum(x["strict"] for x in v)/iss, sum(x["wrong"]+x["harmful"] for x in v)/iss,
                np.mean([x["calls_saved"] for x in v]), np.mean([x["disc_saved"] for x in v]), np.mean([x["loose_saved"] for x in v]), np.mean([x["net_calls"] for x in v]), np.mean([x["net_harm"] for x in v])))
    return C, cands

if __name__ == "__main__":
    N = sys.argv[1] if len(sys.argv) > 1 else "all"
    grid("all" if N == "all" else int(N))
