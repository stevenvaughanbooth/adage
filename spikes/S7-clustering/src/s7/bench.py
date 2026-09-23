"""S7 benchmark. Parameters were frozen from the dev-corpus sweeps (sweep.py, seed 1); every
corpus here uses a different seed. Stages (default: all):

  quality  — S2 benchmark + 10k test corpus: ARI/purity/noise, rerun + reorder determinism
  scale    — 10k / 100k / 300k: wall-clock, peak RSS, quality; global vs per-repository
  perrepo  — per-repository clustering scored per repository
  purity   — behavioural split/merge effect (held-out directive precision/recall)

Writes results/bench_<stage>.json. Usage: PYTHONPATH=src .venv/bin/python -m s7.bench [stages]
"""
from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

import numpy as np

from s7 import corpus, embed, metrics, purity, runner

ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / ".cache"
RES = ROOT / "results"
SEED = 20260922

METHODS = [
    ("HDBSCAN, sklearn, 256-d", "sk_hdbscan", dict(min_cluster_size=20, min_samples=5)),
    ("HDBSCAN, hdbscan lib, PCA-32", "lib_hdbscan_pca32", dict(min_cluster_size=5)),
    ("HDBSCAN, Rust `hdbscan` crate 0.12", "hdbscan_crate", dict(mcs=20, min_pts=5)),
    ("HDBSCAN on exact kNN graph (portable)", "knn_hdbscan", dict(min_cluster_size=40, min_samples=5)),
    ("kNN-DBSCAN (Rust)", "knn_dbscan", dict(k=15, eps=0.75, min_pts=8, min_size=5)),
    ("Mutual-kNN connected components", "mknn_cc", dict(eps=0.75, k=10, min_size=5)),
    ("kNN + Leiden (leidenalg, seed 0)", "knn_leiden", dict(k=15, eps=0.6, resolution=2.0, min_size=10)),
    ("kNN + Louvain (Rust, deterministic)", "louvain", dict(k=10, eps=0.6, res=2.0, min_size=10)),
]
LOUVAIN = dict(k=10, eps=0.6, res=2.0, min_size=10)


def load(name: str):
    """Rows and embeddings for a corpus, cached."""
    CACHE.mkdir(exist_ok=True)
    p = CACHE / f"{name}.npz"
    rows = corpus.s2_benchmark() if name == "s2" else corpus.scaled(int(name[:-1]) * 1000, seed=SEED)
    if p.exists():
        z = np.load(p)
        return rows, z["E"], float(z["embed_s"])
    t = time.time()
    E = embed.embed([embed.normalize(r["text"]) for r in rows])
    s = time.time() - t
    np.savez(p, E=E, embed_s=s)
    return rows, E, s


def load_now():
    return subprocess.run(["uptime"], capture_output=True, text=True).stdout.strip()


def repo_ids(rows):
    names = sorted({r["repo"] for r in rows})
    m = {n: i for i, n in enumerate(names)}
    return np.array([m[r["repo"]] for r in rows])


def stage_quality():
    out = {"load_before": load_now(), "corpora": {}}
    for cname in ["s2", "10k"]:
        rows, E, es = load(cname)
        res = {"n": len(rows), "noise_share": round(sum(r["family"] is None for r in rows) / len(rows), 3), "methods": {}}
        from s7.ceiling import ceiling
        res["oracle"] = ceiling(rows, E)
        perm = np.random.default_rng(1).permutation(len(rows))
        inv = np.argsort(perm)
        for label, m, p in METHODS:
            a = runner.run(m, E, p, canonical=(m == "louvain"))
            if a.get("timeout"):
                res["methods"][label] = {"timeout": True}
                continue
            b = runner.run(m, E, p, canonical=(m == "louvain"))
            c = runner.run(m, E[perm], p, canonical=(m == "louvain"))
            rec = {
                **metrics.score(rows, a["labels"]),
                "rerun_identical": metrics.same_partition(a["labels"], b["labels"]),
                "reorder_identical": metrics.same_partition(a["labels"], c["labels"][inv]),
                "reorder_ari": round(float(metrics.adjusted_rand_score(a["labels"], c["labels"][inv])), 4),
                "wall_s": a["wall_s"], "peak_rss_mb": a["peak_rss_mb"], "params": p,
            }
            if m == "louvain":
                d = runner.run(m, E[perm], p, canonical=False)
                e = runner.run(m, E, p, canonical=False)
                rec["reorder_identical_without_canonical_sort"] = metrics.same_partition(e["labels"], d["labels"][inv])
            res["methods"][label] = rec
            print(cname, label, rec, flush=True)
        out["corpora"][cname] = res
    out["load_after"] = load_now()
    (RES / "bench_quality.json").write_text(json.dumps(out, indent=1, default=str))


def stage_scale():
    out = {"load_before": load_now(), "runs": []}
    # Resume: keep finished runs (set S7_RESUME=1), rerun the rest.
    import os
    prev = RES / "bench_scale.json"
    if os.environ.get("S7_RESUME") == "1" and prev.exists():
        out["runs"] = [r for r in json.loads(prev.read_text())["runs"] if not r.get("timeout")]
    done = {(r["method"], r["size"]) for r in out["runs"]}
    plan = [
        ("kNN + Louvain (Rust), global", "louvain", LOUVAIN, False, ["10k", "100k", "300k"], 3600),
        ("kNN + Louvain (Rust), per repository", "louvain", LOUVAIN, True, ["10k", "100k", "300k"], 3600),
        ("HDBSCAN, hdbscan lib, PCA-32", "lib_hdbscan_pca32", dict(min_cluster_size=5), False, ["10k", "100k", "300k"], 900),
        ("HDBSCAN, sklearn, 256-d", "sk_hdbscan", dict(min_cluster_size=20, min_samples=5), False, ["10k", "100k"], 900),
        ("HDBSCAN, Rust `hdbscan` crate 0.12", "hdbscan_crate", dict(mcs=20, min_pts=5), False, ["10k", "100k"], 900),
        ("kNN + Leiden (leidenalg), global", "knn_leiden", dict(k=15, eps=0.6, resolution=2.0, min_size=10), False, ["10k", "100k", "300k"], 1800),
    ]
    for label, m, p, per_repo, sizes, timeout in plan:
        for size in sizes:
            if (label, size) in done:
                continue
            rows, E, es = load(size)
            groups = repo_ids(rows) if per_repo else None
            r = runner.run(m, E, p, groups=groups, canonical=(m == "louvain"), timeout=timeout)
            rec = {"method": label, "size": size, "n": len(rows), "embed_s_python": round(es, 1), "timeout_s": timeout}
            if r.get("timeout"):
                rec["timeout"] = True
            else:
                rec.update({k: v for k, v in r.items() if k != "labels"})
                rec.update(metrics.score(rows, r["labels"]))
            out["runs"].append(rec)
            print(rec, flush=True)
            (RES / "bench_scale.json").write_text(json.dumps(out, indent=1))
            if rec.get("timeout"):
                break
    out["load_after"] = load_now()
    (RES / "bench_scale.json").write_text(json.dumps(out, indent=1))


def stage_perrepo():
    """Per-repository clustering scored per repository: a family found separately in ten
    repositories is ten correct families, which a global ARI would count as a split."""
    out = {}
    for size in ["100k", "300k"]:
        rows, E, _ = load(size)
        g = repo_ids(rows)
        r = runner.run("louvain", E, LOUVAIN, groups=g, canonical=True)
        per = []
        for rid in sorted(set(g.tolist())):
            ix = np.where(g == rid)[0]
            per.append(metrics.score([rows[i] for i in ix], r["labels"][ix]))
        keys = ["ari", "ari_families", "purity", "coverage", "noise_recall", "noise_recall_oneoff", "noise_recall_chatter", "clusters", "n"]
        out[size] = {
            "repos": len(per),
            "mean": {k: round(float(np.mean([p[k] for p in per])), 4) for k in keys},
            "min_ari": round(float(min(p["ari"] for p in per)), 4),
            "max_ari": round(float(max(p["ari"] for p in per)), 4),
            "wall_s": r["wall_s"],
            "peak_rss_mb": r["peak_rss_mb"],
        }
        print(size, out[size], flush=True)
    (RES / "bench_perrepo.json").write_text(json.dumps(out, indent=1))


def rust_cluster(p):
    return lambda X: runner.run("louvain", X, p, canonical=True)["labels"]


def oracle_cluster(rows, tr_index):
    t = metrics.truth_ids(rows)
    return lambda X: t[tr_index]


def stage_purity():
    out = {}
    kw = dict(bar=0.6, sep=0.85, merge_tau=0.8, min_size=10)
    # Global, 10k test corpus: Louvain, the portable HDBSCAN, and the oracle partition.
    rows, E, _ = load("10k")
    from s7 import methods as M
    out["10k/louvain"] = purity.evaluate(rows, E, rust_cluster(LOUVAIN), **kw)
    out["10k/knn_hdbscan"] = purity.evaluate(rows, E, lambda X: M.knn_hdbscan(X, min_cluster_size=40, min_samples=5), **kw)
    rng = np.random.default_rng(0)
    perm = rng.permutation(len(rows))
    tr = np.sort(perm[: int(0.7 * len(rows))])
    out["10k/oracle"] = purity.evaluate(rows, E, oracle_cluster(rows, tr), modes=("bar",), **kw)
    for k, v in out.items():
        print(k, json.dumps({m: {x: y for x, y in s.items() if x != "split_report"} for m, s in v.items()}), flush=True)
    # Per repository at 100k: aggregate held-out counts over repositories.
    rows, E, _ = load("100k")
    agg = {}
    for g in sorted({r["repo"] for r in rows}):
        ix = np.array([i for i, r in enumerate(rows) if r["repo"] == g])
        res = purity.evaluate([rows[i] for i in ix], E[ix], rust_cluster(LOUVAIN), **kw)
        for mode, s in res.items():
            a = agg.setdefault(mode, {"families": 0, "directives": 0, "wrong_directives": 0, "repos": 0})
            a["families"] += s["families"]
            a["directives"] += s["directives"]
            a["wrong_directives"] += s["wrong_directives"]
            a["repos"] += 1
            a.setdefault("precision_by_repo", []).append(s["directive_precision"])
            a.setdefault("recall_by_repo", []).append(s["directive_recall"])
            a.setdefault("noise_directed_by_repo", []).append(s["noise_directed"])
            a.setdefault("impure_directed_by_repo", []).append(s["impure_family_directed"])
    for mode, a in agg.items():
        for key in ["precision", "recall", "noise_directed", "impure_directed"]:
            v = a.pop(f"{key}_by_repo")
            a[f"{key}_mean"] = round(float(np.mean(v)), 4)
    out["100k_per_repo/louvain"] = agg
    print("100k per repo", json.dumps(agg), flush=True)
    (RES / "bench_purity.json").write_text(json.dumps(out, indent=1, default=str))


if __name__ == "__main__":
    RES.mkdir(exist_ok=True)
    CACHE.mkdir(exist_ok=True)
    stages = sys.argv[1:] or ["quality", "scale", "perrepo", "purity"]
    for s in stages:
        {"quality": stage_quality, "scale": stage_scale, "perrepo": stage_perrepo, "purity": stage_purity}[s]()
