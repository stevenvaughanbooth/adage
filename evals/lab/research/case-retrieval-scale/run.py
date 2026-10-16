"""The experiment: learning curve over memory size, cross-repo memory, ablations, the whole-session
summary variant, oracle ceiling, latency.

Test set: every held-out control-arm session (v1 + v2, all 7 slices). Memory for a test session:
sessions of OTHER tasks (leave-task-out; the same task on another model slice is the same task),
any arm. Thresholds (agreement θa, value θv) are chosen leave-one-repository-out: for each base
repository, the grid point with the most net calls saved on the OTHER repositories' test sessions
among those with pooled precision >= 0.7 and >= 10 directives (abstain if none), then applied to it.

    nice -n 10 env OMP_NUM_THREADS=1 python3 run.py            # -> result_raw.json
"""

from __future__ import annotations

import json
import pickle
import random
import sys
import time
from collections import defaultdict
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from casebank import CACHE, Memory, Params, build_corpus  # noqa: E402
from score import Scorer, aggregate, dtokens  # noqa: E402

SIZES = [5, 10, 20, 50, 100, "all"]
SEEDS = {5: 3, 10: 3, 20: 3, 50: 2, 100: 1, "all": 1}
GRID = {
    "next": [(a, 0.0) for a in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8)],
    "skip": [(a, v) for a in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8) for v in (0, 2, 4, 8)],
    "avoid": [(a, v) for a in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6) for v in (0.3, 0.5, 0.7)],
    "prior": [(a, v) for a in (0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7) for v in (0, 2, 4, 8)],
    "labprior": [(a, 0.0) for a in (0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7)],
    "summary": [(a, 0.0) for a in (0.2, 0.4, 0.6, 0.8, 1.0)],
}
TARGETS = ("next", "skip", "avoid")


def corpus():
    p = CACHE / "corpus.pkl"
    if p.exists():
        return pickle.loads(p.read_bytes())
    C = build_corpus()
    p.write_bytes(pickle.dumps(C))
    return C


def lab_prior(C, M: Memory, sc: Scorer):
    """The embedding lab's recommended reflex (baseline to beat; uses the lab's hand-written
    usefulness label on the MEMORY sessions): the shell command most often useful as a directive at
    the start of a memory session; confidence = its useful rate over memory sessions."""
    cnt = {}
    for s in M.idx:
        for k in {int(k) for k, b in zip(C.kid[s], C.bash[s]) if b}:
            if sc.run_outcome(s, 0, k)[0] == "useful":
                cnt[k] = cnt.get(k, 0) + 1
    if not cnt:
        return None
    k = max(cnt, key=cnt.get)
    return (k, cnt[k] / len(M.idx), 99.0, cnt[k]) if cnt[k] >= 2 else None


def candidates(C, M: Memory, tests: list[int], sc: Scorer) -> dict:
    out = {}
    pr = M.prior(("skip",))
    lp = lab_prior(C, M, sc)
    for s in tests:
        q = M.query(s, TARGETS)
        q["prior"] = pr["skip"]
        q["labprior"] = lp
        q["summary"] = M.summary(s)
        out[s] = q
    return out


def directives(C, s, cand, target, th):
    a0, v0 = th
    its = C.sit_i[s]
    if target in ("prior", "labprior"):
        b = cand[target]
        return [(0, b[0], "run", dtokens(C, b[0]))] if b and b[1] >= a0 and b[2] >= v0 else []
    if target == "summary":
        lines = [c for c in cand["summary"] if c[1] >= a0 - 1e-9]
        tok = 30 + sum(len(C.keys[c[0]]) // 4 + 8 for c in lines)
        return [(0, c[0], "run", tok if j == 0 else 0) for j, c in enumerate(lines)]
    kind = "avoid" if target == "avoid" else "run"
    out, seen = [], set()
    for j, b in enumerate(cand[target]):
        if b is None or b[1] < a0 or b[2] < v0 or b[0] in seen:
            continue
        seen.add(b[0])
        out.append((its[j], b[0], kind, dtokens(C, b[0])))
    return out


def score_grid(C, sc: Scorer, cands: dict, target: str) -> dict:
    """{th: {s: stats}}"""
    res = {}
    for th in GRID[target]:
        res[th] = {s: sc.session(s, directives(C, s, cands[s], target, th)) for s in cands}
    return res


def loro(C, grid_res: dict, bases: dict, prec=0.0, min_issued=10):
    """Leave-one-repo-out threshold choice: the grid point with the most harm-adjusted net calls on
    the OTHER repositories (abstain when none is positive). Returns ({s: stats}, {base: th})."""
    chosen, rows = {}, {}
    allb = sorted(set(bases.values()))
    for b in allb:
        best, bth = None, None
        for th, per in grid_res.items():
            other = [st for s, st in per.items() if bases[s] != b]
            iss = sum(x["issued"] for x in other)
            use = sum(x["useful"] for x in other)
            if iss < min_issued or use / iss < prec:
                continue
            net = sum(x["net_harm"] for x in other)
            if net > 0 and (best is None or net > best):
                best, bth = net, th
        chosen[b] = bth
        for s, st in (next(iter(grid_res.values()))).items():
            if bases[s] != b:
                continue
            if bth is None:
                z = dict(st)
                for k in ("issued", "useful", "strict", "redundant", "wrong", "harmful", "unused", "calls_saved",
                          "disc_saved", "usd_saved", "net_calls", "net_usd", "cost_usd", "cost_calls", "harm_calls",
                          "net_harm", "net_usd_harm", "loose_saved"):
                    z[k] = 0
                rows[s] = z
            else:
                rows[s] = grid_res[bth][s]
    return rows, chosen


def oracle(C, s):
    """Hindsight ceiling of this lever: every failed non-edit call that some run directive (a key
    that first occurs after it and then succeeds) or avoid directive (its own key fails at its first
    occurrence) could have removed."""
    kk, su = C.kid[s], C.succ[s]
    steps = C.sessions[s].steps
    n = len(kk)
    first = {}
    for t in range(n):
        if kk[t] >= 0 and kk[t] not in first:
            first[kk[t]] = t
    works = {k for k in first if su[first[k]:][kk[first[k]:] == k].any()}
    late = sorted(first[k] for k in works)
    last = max(late) if late else -1
    loose = [j for j, st in enumerate(steps) if (st.failed or st.action.explore) and not st.action.edit and j < last]
    saved = set()
    for j, st in enumerate(steps):
        if not st.failed or st.action.edit or not st.action.directive_eligible:
            continue
        if any(t > j for t in late) or (kk[j] >= 0 and C.bash[s][j]):
            saved.add(j)
    loose = set(loose) | saved
    return {"calls": len(saved), "usd": sum(steps[j].usd for j in saved), "loose": len(loose),
            "loose_usd": sum(steps[j].usd for j in loose), "failed_all": sum(x.failed for x in steps),
            "n": n, "usd_total": C.sessions[s].usd_total}


def main():
    t0 = time.time()
    C = corpus()
    S = C.sessions
    tests = [i for i, s in enumerate(S) if s.arm == "control"]
    bases = {i: S[i].base for i in tests}
    by_task = defaultdict(list)
    for i in tests:
        by_task[S[i].task].append(i)
    sc = Scorer(C)
    out = {"n_sessions": len(S), "n_tests": len(tests), "configs": {}}

    def run_config(name, mem_fn, prm=Params(), seeds=1, targets=("next", "skip", "avoid", "prior", "labprior", "summary")):
        t = time.time()
        per_seed = []
        nb, nmem, nland = [], [], []
        for seed in range(seeds):
            cands = {}
            for task, ts in by_task.items():
                idx = mem_fn(task, seed)
                if idx is None or len(idx) < 2:
                    continue
                M = Memory(C, idx, prm)
                nb.append(M.nbytes())
                nmem.append(len(idx))
                nland.append(M.n_landmarks)
                cands.update(candidates(C, M, ts, sc))
            if not cands:
                return None
            res = {}
            for tg in targets:
                g = score_grid(C, sc, cands, tg)
                rows, chosen = loro(C, g, {s: bases[s] for s in cands})
                res[tg] = (rows, chosen)
            # combined: skip + avoid at their own chosen thresholds
            comb = {}
            for s in cands:
                b = bases[s]
                d = []
                for tg in ("skip", "avoid"):
                    th = res[tg][1][b]
                    if th is not None:
                        d += directives(C, s, cands[s], tg, th)
                comb[s] = sc.session(s, d)
            res["skip+avoid"] = (comb, None)
            per_seed.append(res)
        # pool seeds: average per-session stats across seeds, then aggregate
        agg = {}
        for tg in per_seed[0]:
            sess = sorted(per_seed[0][tg][0])
            rows = []
            for s in sess:
                rs = [p[tg][0][s] for p in per_seed if s in p[tg][0]]
                rows.append({k: float(np.mean([r[k] for r in rs])) for k in rs[0]})
            a = aggregate(rows)
            a["thresholds"] = {b: per_seed[0][tg][1][b] for b in per_seed[0][tg][1]} if per_seed[0][tg][1] else None
            a["by_base"] = {b: aggregate([r for s, r in zip(sess, rows) if bases[s] == b])
                            for b in sorted({bases[s] for s in sess})}
            a["per_session"] = {S[s].sid: r["net_harm"] for s, r in zip(sess, rows)}
            agg[tg] = a
        agg["memory"] = {"sessions_mean": float(np.mean(nmem)), "bytes_mean": float(np.mean(nb)),
                         "landmarks_mean": float(np.mean(nland))}
        out["configs"][name] = agg
        print(f"{name}: {time.time() - t:.0f}s  mem {np.mean(nmem):.0f}  "
              + "  ".join(f"{tg} {agg[tg]['net_harm'][0]:+.2f} ({agg[tg]['net_calls'][0]:+.2f}) "
                          f"p={agg[tg]['precision'][0] or 0:.2f}" for tg in agg
                          if tg != "memory"), flush=True)
        json.dump(out, open(HERE / "result_raw.json", "w"), indent=1, default=str)
        return agg

    pools_same = {task: [i for i, s in enumerate(S) if s.base == S[by_task[task][0]].base and s.task != task]
                  for task in by_task}
    pools_other = {task: [i for i, s in enumerate(S) if s.base != S[by_task[task][0]].base] for task in by_task}

    def same(N):
        def f(task, seed):
            p = pools_same[task]
            if N == "all":
                return p
            if N > len(p):
                return None
            return random.Random(f"{task}-{N}-{seed}").sample(p, N)
        return f

    only = sys.argv[1:] or None
    # 1. learning curve, same repository
    for N in SIZES:
        if only and "curve" not in only:
            break
        run_config(f"same-{N}", same(N), seeds=SEEDS[N])
    # 2. cross-repository memory
    if not only or "cross" in only:
        run_config("other-all", lambda task, seed: pools_other[task])
        for N in (5, 20, "all"):
            def f(task, seed, N=N):
                a = same(N)(task, seed)
                return None if a is None else a + pools_other[task]
            run_config(f"same-{N}+other-all", f, seeds=SEEDS[N])
    # 3. ablations at same-all
    if not only or "ablate" in only:
        for name, prm in (("no-outcome", Params(outcome=False)), ("no-prompt", Params(w=(0, .5, .5))),
                          ("no-output", Params(w=(.5, .5, 0))), ("history-only", Params(w=(0, 1, 0))),
                          ("k50", Params(k=50)), ("run-only", Params(run_only=True))):
            run_config(f"same-all/{name}", same("all"), prm=prm, targets=("next", "skip", "avoid", "summary"))
    # 4. oracle ceiling
    orc = [oracle(C, s) for s in tests]
    out["oracle"] = {
        "calls_per_session": float(np.mean([o["calls"] for o in orc])),
        "failed_per_session": float(np.mean([o["failed_all"] for o in orc])),
        "calls_per_session_total": float(np.mean([o["n"] for o in orc])),
        "pct_calls": float(sum(o["calls"] for o in orc) / sum(o["n"] for o in orc)),
        "pct_usd": float(sum(o["usd"] for o in orc) / sum(o["usd_total"] for o in orc)),
        "loose_calls_per_session": float(np.mean([o["loose"] for o in orc])),
        "loose_pct_calls": float(sum(o["loose"] for o in orc) / sum(o["n"] for o in orc)),
        "loose_pct_usd": float(sum(o["loose_usd"] for o in orc) / sum(o["usd_total"] for o in orc)),
    }
    # 5. latency at the largest memories
    lat = {}
    for name, fn in (("same-all", same("all")), ("same-all+other-all", lambda t, s: pools_same[t] + pools_other[t])):
        task = next(iter(by_task))
        M = Memory(C, fn(task, 0), Params())
        s = by_task[task][0]
        ts = []
        for _ in range(3):
            t = time.perf_counter()
            M.query(s)
            ts.append((time.perf_counter() - t) * 1000 / len(C.sit_i[s]))
        tb = time.perf_counter()
        Memory(C, fn(task, 0), Params())
        lat[name] = {"situations": int(len(M.rows)), "sessions": int(len(M.idx)), "bytes_i16": M.nbytes(),
                     "query_ms_per_decision_point_py": float(np.median(ts)),
                     "build_s_py": time.perf_counter() - tb}
    import os
    os.environ["ADAGE_LAB_CACHE"] = str(CACHE)
    sys.path.insert(0, str(HERE.parent.parent))
    import embed
    m = embed.load("potion-8M")
    txt = ["run python -m pytest tests/test_x.py [failed] | read src/a.py"] * 50
    m.encode(txt[:2])
    tt = []
    for x in txt:
        t = time.perf_counter()
        m.encode([x])
        tt.append((time.perf_counter() - t) * 1000)
    lat["embed_ms_median"] = float(np.median(tt))
    out["latency"] = lat
    out["elapsed_s"] = time.time() - t0
    json.dump(out, open(HERE / "result_raw.json", "w"), indent=1, default=str)
    print("done", out["oracle"], lat, time.time() - t0)


if __name__ == "__main__":
    main()
