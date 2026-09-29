#!/usr/bin/env python3
"""The embedding lab: case-based next-action retrieval vs n-gram baselines (vs the Rust engine,
which `engine.py` replays on the same split).

  ADAGE_LAB_MODELS=<dir> python3 evals/lab/lab.py [--quick] [--out evals/lab/results/lab.json]

Per repository: memory = the chronological split's training traces; test = the held-out
control-arm sessions (RepoBench v1 + v2, Claude Haiku 4.5). Every parameter and the directive
threshold are tuned on attrs, click and marshmallow (pooled) and applied unchanged to urfave-cli
(Go) and vuejs/core (TypeScript), which play no part in tuning.

Also reported: a "grown memory" variant (memory = training traces + every other task's held-out
sessions, all arms, leave-task-out) to see whether the method scales with history, and a
cross-repository variant for the held-out repos (memory = the other four repositories' training
traces only).
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

import numpy as np

import embed
import metrics
import traces
from predict import KNN, NGram, Params
from situations import situations

TUNE = ["attrs", "click", "marshmallow"]
HELD = ["urfave-cli", "vue-core"]
THETAS = [round(float(x), 2) for x in np.arange(0.05, 1.0, 0.05)]
TARGETS = (0.7, 0.5)


def sits_of(sessions):
    return [s for x in sessions for s in situations(x)]


def pool(curves: list[list[dict]]) -> list[dict]:
    out = []
    for j, th in enumerate(THETAS):
        rows = [c[j] for c in curves]
        n = sum(r["issued"] for r in rows)
        u = sum(r["useful"] for r in rows)
        ses = sum(r["sessions"] for r in rows)
        per = lambda k: sum(r[k] * r["sessions"] for r in rows) / max(ses, 1)
        us = sum(round((r["precision_strict"] or 0) * r["issued"]) for r in rows)
        out.append({"theta": th, "issued": n, "useful": u, "sessions": ses, "precision": u / n if n else None,
                    "precision_strict": us / n if n else None,
                    "saved_per_session": per("saved_per_session"),
                    "saved_discovery_per_session": per("saved_discovery_per_session"),
                    "saved_fail_per_session": per("saved_fail_per_session"),
                    "recall": float(np.mean([r["recall"] for r in rows]))})
    return out


def at(curve, theta):
    return next(c for c in curve if abs(c["theta"] - theta) < 1e-9)


def slim(c):
    if not c:
        return None
    return {k: (round(float(v), 4) if isinstance(v, (float, np.floating)) else v) for k, v in c.items()
            if k != "examples"}


def summarise(name, curves, accs) -> dict:
    pc = pool(curves)
    row = {"name": name, "acc": {lv: float(np.mean([a[lv] for a in accs])) for lv in ("key", "template", "full")}}
    for t in TARGETS:
        op = metrics.operating_point(pc, t)
        row[f"op{t}"] = slim(op)
    valid = [c for c in pc if c["issued"] >= 10 and c["precision"] is not None]
    row["max_precision"] = slim(max(valid, key=lambda c: c["precision"])) if valid else None
    return row


def score_key(row):
    """Rank a directive configuration: discovery turns saved at P>=0.7, then at P>=0.5, then max
    precision."""
    o7, o5, mx = row["op0.7"], row["op0.5"], row["max_precision"]
    k = "saved_discovery_per_session"
    return (o7[k] if o7 else -1, o5[k] if o5 else -1, mx["precision"] if mx else 0)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="a smaller grid")
    ap.add_argument("--out", default=str(traces.LAB / "results" / "lab.json"))
    a = ap.parse_args(argv)
    t0 = time.time()
    S = traces.load()
    data = {}
    for repo in traces.REPOS:
        train = [s for s in S[repo] if s.phase == "train"]
        test = [s for s in S[repo] if s.phase == "eval" and s.arm == "control"]
        data[repo] = {"train": train, "test": test, "tr": sits_of(train), "te": sits_of(test)}
        print(f"{repo}: train {len(train)} sessions / {len(data[repo]['tr'])} situations; "
              f"test {len(test)} / {len(data[repo]['te'])}", flush=True)

    models = ["tfidf"] + embed.available()
    print("models:", models, flush=True)
    emb = {m: embed.Cached(m, embed.load(m)) for m in models if m != "tfidf"}

    def embedder(m, train_sits):
        if m != "tfidf":
            return emb[m]
        t = embed.Tfidf()
        t.fit([s.prompt_text for s in train_sits] + [s.hist_text for s in train_sits])
        return t

    bases = {}

    def knn_preds(p, repo, memory_sits=None, test_sits=None):
        if memory_sits is None and test_sits is None:
            # The default split: one index and one set of similarity matrices per (repo, model),
            # re-weighted for every configuration.
            if (repo, p.model) not in bases:
                d = data[repo]
                full = Params(model=p.model, w_prompt=1, w_hist=1, w_struct=1)
                knn = KNN(d["tr"], full, embedder(p.model, d["tr"]))
                bases[(repo, p.model)] = (knn, knn.components(d["te"]))
            knn, comp = bases[(repo, p.model)]
            knn.p = p
            return knn.run(data[repo]["te"], comp)
        ms = memory_sits if memory_sits is not None else data[repo]["tr"]
        ts = test_sits if test_sits is not None else data[repo]["te"]
        return KNN(ms, p, embedder(p.model, ms)).run(ts)

    res = {"split": {r: {"train_sessions": len(data[r]["train"]), "test_sessions": len(data[r]["test"]),
                         "train_situations": len(data[r]["tr"]), "test_situations": len(data[r]["te"])}
                     for r in traces.REPOS},
           "tuning": [], "per_repo": {}, "grown": {}, "cross_repo": {}}

    # ------------------------------------------------------------ baselines (tuned like everything else)
    baselines = {}
    for order in (0, 1, 2):
        for target in ("next", "useful"):
            for run_only in (False, True):
                name = f"order-{order}/{target}" + ("/run" if run_only else "")
                baselines[name] = lambda tr, o=order, t=target, r=run_only: NGram(tr, o, target=t, run_only=r)
    for name, mk in baselines.items():
        curves, accs = [], []
        for repo in TUNE:
            d = data[repo]
            pr = mk(d["tr"]).run(d["te"])
            curves.append(metrics.sweep(d["te"], pr, THETAS))
            accs.append(metrics.accuracy(d["te"], pr))
        row = summarise(name, curves, accs)
        row["kind"] = "baseline"
        res["tuning"].append(row)

    # ------------------------------------------------------------ kNN grid (tuning repos only)
    weights = [(0.5, 0.5, 0.0), (0.3, 0.4, 0.3), (0.2, 0.3, 0.5), (0.0, 0.5, 0.5), (0.5, 0.0, 0.5),
               (0.6, 0.2, 0.2), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)]
    ks = [10, 20, 40] if not a.quick else [20]
    grid = []
    for m in models:
        for w in weights:
            if w[0] == 0 and w[1] == 0 and m != models[0]:
                continue  # structure-only does not use the text model
            for k in ks:
                for target in ("next", "useful"):
                    for run_only in (False, True):
                        grid.append(Params(model=m, w_prompt=w[0], w_hist=w[1], w_struct=w[2], k=k,
                                           target=target, run_only=run_only))
    print(f"kNN grid: {len(grid)} configurations x {len(TUNE)} repos", flush=True)
    for gi, p in enumerate(grid):
        curves, accs = [], []
        for repo in TUNE:
            d = data[repo]
            pr = knn_preds(p, repo)
            curves.append(metrics.sweep(d["te"], pr, THETAS))
            accs.append(metrics.accuracy(d["te"], pr))
        label = p.model if (p.w_prompt or p.w_hist) else "struct-only"
        row = summarise(f"knn:{label}", curves, accs)
        row.update(kind="knn", model=label, params=dict(p.__dict__))
        res["tuning"].append(row)
        if gi % 25 == 0:
            print(f"  [{gi}/{len(grid)}] {label} w={p.w_prompt, p.w_hist, p.w_struct} k={p.k} {p.target} "
                  f"run_only={p.run_only}: acc {row['acc']['template']:.3f} op0.7 {row['op0.7']} "
                  f"max P {row['max_precision'] and row['max_precision']['precision']}", flush=True)
    for m in emb.values():
        m.save()

    # ------------------------------------------------------------ selection
    knn_rows = [r for r in res["tuning"] if r["kind"] == "knn"]
    best_acc, best_dir = {}, {}
    for r in knn_rows:
        m = r["model"]
        if m not in best_acc or r["acc"]["template"] > best_acc[m]["acc"]["template"]:
            best_acc[m] = r
        if m not in best_dir or score_key(r) > score_key(best_dir[m]):
            best_dir[m] = r
    base_rows = {r["name"]: r for r in res["tuning"] if r["kind"] == "baseline"}
    best_base_dir = max(base_rows.values(), key=score_key)
    chosen = max(best_dir.values(), key=score_key)
    res["best_acc"] = {m: {"params": r["params"], "acc": r["acc"]} for m, r in best_acc.items()}
    res["best_dir"] = {m: {k: r[k] for k in ("params", "op0.7", "op0.5", "max_precision", "acc")}
                       for m, r in best_dir.items()}
    res["best_baseline_dir"] = {k: best_base_dir[k] for k in ("name", "op0.7", "op0.5", "max_precision")}
    res["chosen"] = {k: chosen[k] for k in ("params", "op0.7", "op0.5", "max_precision", "acc")}
    print("chosen kNN:", chosen["params"], chosen["op0.7"], chosen["op0.5"], flush=True)
    print("best baseline:", best_base_dir["name"], best_base_dir["op0.7"], best_base_dir["op0.5"], flush=True)

    def theta_of(row):
        for t in TARGETS:
            if row[f"op{t}"]:
                return row[f"op{t}"]["theta"], t
        return (row["max_precision"]["theta"], "max") if row["max_precision"] else (None, None)

    # ------------------------------------------------------------ per-repo evaluation of the selected methods
    def evaluate(repo, preds, theta, test_sits=None):
        ts = test_sits if test_sits is not None else data[repo]["te"]
        curve = metrics.sweep(ts, preds, THETAS)
        fixed = at(curve, theta) if theta is not None else None
        return {"acc": metrics.accuracy(ts, preds),
                "acc_exec": metrics.accuracy(ts, preds, only=lambda x: x.tool == "Bash" and not x.explore),
                "theta": theta,
                "at_tuned_theta": slim(fixed),
                "examples": fixed["examples"][:80] if fixed else [],
                "op0.7_repo": slim(metrics.operating_point(curve, 0.7)),
                "curve": [slim(c) for c in curve]}

    selected = {}
    for name in ("order-0/next", "order-1/next", "order-2/next"):
        selected[name] = ("base", baselines[name], None)
    selected[f"baseline-dir:{best_base_dir['name']}"] = ("base", baselines[best_base_dir["name"]],
                                                         theta_of(best_base_dir))
    for m, r in best_acc.items():
        selected[f"knn-acc:{m}"] = ("knn", Params(**r["params"]), None)
    for m, r in best_dir.items():
        selected[f"knn-dir:{m}"] = ("knn", Params(**r["params"]), theta_of(r))
    res["selected"] = {k: {"kind": v[0], "params": (v[1].__dict__ if v[0] == "knn" else None),
                           "theta": v[2]} for k, v in selected.items()}
    for repo in traces.REPOS:
        d = data[repo]
        out = {}
        for name, (kind, obj, th) in selected.items():
            pr = obj(d["tr"]).run(d["te"]) if kind == "base" else knn_preds(obj, repo)
            out[name] = evaluate(repo, pr, th[0] if th else None)
        res["per_repo"][repo] = out
        print(f"{repo}: " + "; ".join(f"{n} {v['acc']['template']:.3f}" for n, v in out.items() if "acc" in n
                                      or n.startswith("order")), flush=True)

    # ------------------------------------------------------------ grown memory (leave-task-out)
    p = Params(**chosen["params"])
    th = theta_of(chosen)[0]
    bb = baselines[best_base_dir["name"]]
    bth = theta_of(best_base_dir)[0]
    for repo in traces.REPOS:
        d = data[repo]
        allsess = S[repo]
        pk, pb, ts_all = [], [], []
        for task in sorted({s.task for s in d["test"]}):
            ms = sits_of([s for s in allsess if s.task != task])
            ts = sits_of([s for s in d["test"] if s.task == task])
            pk += knn_preds(p, repo, ms, ts)
            pb += bb(ms).run(ts)
            ts_all += ts
        res["grown"][repo] = {"memory_sessions": len(allsess),
                              "knn": evaluate(repo, pk, th, ts_all),
                              "baseline": evaluate(repo, pb, bth, ts_all)}
        g = res["grown"][repo]
        print(f"grown {repo}: kNN acc {g['knn']['acc']['template']:.3f} dir {g['knn']['at_tuned_theta']} | "
              f"baseline dir {g['baseline']['at_tuned_theta']}", flush=True)

    # ------------------------------------------------------------ cross-repo memory for the held-out repos
    for repo in HELD:
        ms = sits_of([s for r in traces.REPOS if r != repo for s in data[r]["train"]])
        res["cross_repo"][repo] = evaluate(repo, knn_preds(p, repo, ms), th)
    for m in emb.values():
        m.save()

    # ------------------------------------------------------------ latency
    texts = [s.hist_text for s in data["click"]["te"][:300]]
    lat = {}
    for m in models:
        lat[m] = embed.latency(m, texts)
        lat[m]["size_mb"] = round(embed.size_mb(m), 1) if m != "tfidf" else 0
        lat[m]["licence"] = embed.SPECS[m]["licence"] if m != "tfidf" else "n/a (fitted per repo)"
        lat[m]["dim"] = int(emb[m].encode(["x"]).shape[1]) if m != "tfidf" else None
    rng = np.random.default_rng(0)
    for n in (1000, 5000, 20000):
        for dim in (256, 384):
            X = rng.standard_normal((n, dim)).astype(np.float32)
            q = rng.standard_normal(dim).astype(np.float32)
            ts = []
            for _ in range(200):
                t1 = time.perf_counter()
                s = X @ q
                np.argpartition(-s, 10)[:10]
                ts.append((time.perf_counter() - t1) * 1000)
            ts.sort()
            lat[f"knn-{n}x{dim}"] = {"median_ms": round(ts[100], 3), "p95_ms": round(ts[190], 3)}
    res["latency"] = lat
    res["elapsed_s"] = round(time.time() - t0, 1)
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps(res, separators=(",", ":"), default=str))
    print("wrote", a.out, "in", res["elapsed_s"], "s")
    return 0


if __name__ == "__main__":
    sys.exit(main())
