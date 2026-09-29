"""Driver. `python run.py split|loro|curve|rules|ksweep [--K 128]`; writes results/<regime>.json.

Regimes
- split: train = RepoBench `train` arm (all five repos pooled, 47 sessions); test = every held-out
  control session (208: Haiku 4.5 + Sonnet 5 on click/vue-core). Discovery weights, clusters and
  models see training sessions only.
- loro: for each repo R, train = every session (all arms) of the other four repos; test = R's control
  sessions. Nothing from R (not even its clusters) is used for fitting.
- curve: leave-task-out learning curve. Tasks in 5 folds; train = n sessions sampled from the other
  folds (all arms, all repos), test = control sessions of the held-out fold.
"""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

import numpy as np

from skip import (Sess, NextSym, crossfit_calibrator, deliver, discovery_weights, labels, make, oracle, outcome,
                  summarise, load)
from space import Space

HERE = Path(__file__).resolve().parent
OUT = HERE / "results"
MODELS = ["prior", "repo-prior", "ppm1-repo", "ppm3-repo", "ppm3", "ppm3-next5-repo", "logistic", "mlp",
          "prefix-tree"]
THETAS = [0.05, 0.1, 0.15, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]


def prep(train_raw, test_raw, K, seed=0):
    sp = Space(K=K, seed=seed).fit(train_raw)
    tr = [Sess(s, sp) for s in train_raw]
    te = [Sess(s, sp) for s in test_raw]
    W = discovery_weights(tr, sp.K)
    trd = [(s, labels(s, W, sp.K)) for s in tr]
    ted = [(s, labels(s, W, sp.K)) for s in te]
    return sp, W, trd, ted


def evaluate(models, sp, W, trd, ted, thetas=THETAS, folds=5, ci=True):
    K = sp.K
    res = {}
    for name in models:
        t0 = time.time()
        cal = crossfit_calibrator(name, K, trd, folds=folds)
        m = make(name, K).fit(trd)
        probs = []
        for s, lab in ted:
            sc = m.score(s)
            p = cal.predict(sc.ravel()).reshape(sc.shape) + 1e-6 * sc / max(sc.max(), 1e-12)  # tie-break
            probs.append((s, lab, p))
        reps = represent(sp, trd)
        curve = []
        for th in thetas:
            rows = [outcome(s, lab, W, deliver(s, p, th), rep=reps.get(s.repo, reps[None])) for s, lab, p in probs]
            if sum(r["n"] for r in rows) == 0:
                continue
            curve.append({"theta": th, **summarise(rows, ci=ci)})
        res[name] = {"curve": curve, "secs": round(time.time() - t0, 1)}
        for c in curve:
            print(f"  {name:12s} θ={c['theta']} n/s={c['deliveries_per_session']:.2f} prec={c['precision']} "
                  f"prec_inst={c['precision_inst']} prec_txt={c['precision_text']} taken={c['taken_rate']} net fsim={c['net_fsim']:.2f} relfail={c['net_relfail']:.2f} "
                  f"inst={c['net_inst']:.2f} ({c['net_inst_usd_pct']:.1f}%$) expl={c['net_explore']:.2f} inst_any={c['net_inst_any']:.2f} dist={c['net_dist']:.2f}", flush=True)
    return res


def represent(sp, trd):
    """Delivered text per cluster: its most frequent training member in the session's repo (else
    overall). Returns {repo|None: (K, d) matrix of the representatives' vectors}."""
    out = {}
    repos = sorted({s.repo for s, _ in trd})
    for R in repos + [None]:
        texts = {t for s, _ in trd if R is None or s.repo == R for t in s.texts}
        reps = []
        for c in range(sp.K):
            cand = [t for t, _ in sp.members[c].most_common() if t in texts] or sp.describe(c, 1) or [""]
            reps.append(cand[0])
        out[R] = sp.vec(reps)
    return out


def ceilings(W, ted):
    out = {"calls_per_session": round(float(np.mean([s.n for s, _ in ted])), 2)}
    tot = sum(s.usd for s, _ in ted)
    for b in (None, 1, 3):
        c = [oracle(s, lab, b) for s, lab in ted]
        out["unlimited" if b is None else f"budget{b}"] = {
            k: {"calls": round(float(np.mean([x[k][0] for x in c])), 3),
                "usd_pct": round(100 * sum(x[k][1] for x in c) / tot, 2)} for k in ("fsim", "relfail", "inst", "explore", "dist", "inst_any")}
    return out


def next_acc(sp, trd, ted):
    ns = NextSym(sp.K, 3).fit([s.c for s, _ in trd])
    ns1 = NextSym(sp.K, 1).fit([s.c for s, _ in trd])
    h3 = h1 = n = 0
    for s, _ in ted:
        for i in range(s.n):
            n += 1
            h3 += ns.predict(s.c, i).argmax() == s.c[i]
            h1 += ns1.predict(s.c, i).argmax() == s.c[i]
    return {"ppm3_top1": round(h3 / n, 3), "order1_top1": round(h1 / n, 3), "n": n}


def split_regime(S, K, models=MODELS):
    tr = [s for s in S if s["arm"] == "train"]
    te = [s for s in S if s["arm"] == "control"]
    sp, W, trd, ted = prep(tr, te, K)
    print(f"split K={sp.K} tightness={sp.tightness:.3f} train={len(trd)} test={len(ted)}", flush=True)
    out = {"K": sp.K, "tightness": sp.tightness, "train": len(trd), "test": len(ted),
           "next_cluster": next_acc(sp, trd, ted), "ceiling": ceilings(W, ted)}
    print("  next-cluster", out["next_cluster"], "\n  ceiling", out["ceiling"], flush=True)
    out["models"] = evaluate(models, sp, W, trd, ted)
    return out, (sp, W, trd, ted)


def loro_regime(S, K, models=MODELS):
    out = {}
    for R in ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]:
        tr = [s for s in S if s["repo"] != R]
        te = [s for s in S if s["repo"] == R and s["arm"] == "control"]
        sp, W, trd, ted = prep(tr, te, K)
        print(f"loro {R}: train={len(trd)} test={len(ted)}", flush=True)
        out[R] = {"train": len(trd), "test": len(ted), "ceiling": ceilings(W, ted),
                  "models": evaluate(models, sp, W, trd, ted, folds=3, ci=False)}
    return out


def eval_rows(models, sp, W, trd, ted, thetas=THETAS, folds=3):
    """{model: {theta: [outcome rows]}} (for pooling across test folds)."""
    K = sp.K
    reps = represent(sp, trd)
    out = {}
    for name in models:
        cal = crossfit_calibrator(name, K, trd, folds=folds)
        m = make(name, K).fit(trd)
        probs = []
        for s, lab in ted:
            sc = m.score(s)
            probs.append((s, lab, cal.predict(sc.ravel()).reshape(sc.shape) + 1e-6 * sc / max(sc.max(), 1e-12)))
        out[name] = {th: [outcome(s, lab, W, deliver(s, p, th), rep=reps.get(s.repo, reps[None]))
                          for s, lab, p in probs] for th in thetas}
    return out


def curve_regime(S, K, models, sizes=(25, 50, 100, 200, 400, 10000), nfold=5, seed=0):
    rng = np.random.default_rng(seed)
    tasks = sorted({s["task"] for s in S})
    rng.shuffle(tasks)
    fold = {t: k % nfold for k, t in enumerate(tasks)}
    res = {}
    for n in sizes:
        pooled = {m: {th: [] for th in THETAS} for m in models}
        ntrain = []
        for f in range(nfold):
            pool = [s for s in S if fold[s["task"]] != f]
            te = [s for s in S if fold[s["task"]] == f and s["arm"] == "control"]
            idx = rng.permutation(len(pool))[:n]
            tr = [pool[i] for i in sorted(idx)]
            ntrain.append(len(tr))
            sp, W, trd, ted = prep(tr, te, K)
            rows = eval_rows(models, sp, W, trd, ted)
            for m in models:
                for th in THETAS:
                    pooled[m][th] += rows[m][th]
        res[n] = {"train_sessions": int(np.mean(ntrain)), "models": {}}
        for m in models:
            curve = [{"theta": th, **summarise(pooled[m][th], ci=True)} for th in THETAS
                     if sum(r["n"] for r in pooled[m][th])]
            res[n]["models"][m] = curve
            ok = [c for c in curve if (c["precision"] or 0) >= 0.7 and c["deliveries_per_session"] * c["sessions"] >= 10]
            best = max(curve, key=lambda c: c["net_inst"]) if curve else None
            print(f"n={n} ({int(np.mean(ntrain))}) {m:14s} best-inst θ={best['theta']} prec={best['precision']} "
                  f"d/s={best['deliveries_per_session']:.2f} net_inst={best['net_inst']:.2f} "
                  f"CI={best.get('net_inst_ci')} | p>=0.7 points: "
                  f"{[(c['theta'], round(c['deliveries_per_session'], 2), round(c['net_inst'], 2)) for c in ok]}",
                  flush=True)
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("regime")
    ap.add_argument("--K", type=int, default=128)
    ap.add_argument("--models", default=",".join(MODELS))
    ap.add_argument("--sizes", default="25,50,100,200,400,10000")
    a = ap.parse_args()
    OUT.mkdir(exist_ok=True)
    S = load()
    models = a.models.split(",")
    if a.regime == "split":
        res, _ = split_regime(S, a.K, models)
    elif a.regime == "curve":
        res = curve_regime(S, a.K, models, sizes=[int(x) for x in a.sizes.split(",")])
    elif a.regime == "loro":
        res = loro_regime(S, a.K, models)
    else:
        raise SystemExit("unknown regime")
    (OUT / f"{a.regime}-K{a.K}.json").write_text(json.dumps(res, indent=1, default=float))


if __name__ == "__main__":
    main()
