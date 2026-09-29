"""Held-out evaluation of file prefetch: recall@k, then calls and dollars under the counterfactual.

Parts (all printed and written to results.json):
  A. task level, all 188 tasks, target = gold source files (+ gold test files): content and git
     scorers alone and fused (fusion weights leave-one-repo-out). The largest n; no traces needed.
  B. session level, held-out control sessions of the split's test tasks: recall of the edited
     files E, the necessary reads N and T = E ∪ N; kNN over past traces; learned fusion (trained on
     the split's train sessions of all repos, or leave-one-repo-out).
  C. counterfactual: a top-k path hint at the first prompt; calls and $ removed (targets.py), net
     of the hint's carry cost; ceiling = the oracle hint (H = T).
  D. learning curve over kNN memory size; E. hook latency.
"""

from __future__ import annotations

import collections
import json
import pickle
import sys
import time
from pathlib import Path

import numpy as np
from sklearn.linear_model import LogisticRegression

import data
import repo as R
import retrieve as RT
import targets as TG

FEATS = pickle.loads((RT.CACHE / "features.pkl").read_bytes())
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
BASE_FEATS = ["bm25", "bm25c", "p8", "p32", "code16", "hot", "co_bm25", "co_content", "ref_content"]
HINT_TOKENS_PER_PATH, HINT_HEADER = 15, 40
KS = (3, 5, 10)
# scenario -> (follow rate p, calls tier, $ tier, misfire probability q per wrong hinted file)
SCEN = {
    "optimistic": (1.0, "all", "all", 0.0),
    "realistic": (0.5, "all", "all", 0.1),
    "conservative": (0.5, "search", "search", 0.2),
}
rng0 = np.random.default_rng(0)


def boot(x, B=2000, seed=0):
    x = np.asarray(x, float)
    if len(x) == 0:
        return [float("nan")] * 3
    rng = np.random.default_rng(seed)
    m = x[rng.integers(0, len(x), (B, len(x)))].mean(1)
    return [round(float(x.mean()), 4), round(float(np.percentile(m, 5)), 4), round(float(np.percentile(m, 95)), 4)]


def ratio_boot(num, den, B=2000, seed=0):
    num, den = np.asarray(num, float), np.asarray(den, float)
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, len(num), (B, len(num)))
    r = num[idx].sum(1) / den[idx].sum(1)
    return [round(float(num.sum() / den.sum()), 4), round(float(np.percentile(r, 5)), 4), round(float(np.percentile(r, 95)), 4)]


def recall(scores: np.ndarray, files: list, target: set, k: int):
    if not target:
        return None
    top = {files[i] for i in np.argsort(-scores, kind="stable")[:k]}
    return len(top & target) / len(target)


# ------------------------------------------------------------------ kNN over past traces
_qemb = {}


def qemb(text: str, name: str = "code16") -> np.ndarray:
    k = (name, text)
    if k not in _qemb:
        _qemb[k] = RT.model(name).encode([text])[0]
    return _qemb[k]


def trace_files(s, d) -> set:
    """What a past session touched that mattered: edited, or opened before its last edit."""
    return d["E"] | d["pre"]


def knn_scores(prompt: str, files: list, memory: list, k: int = 5) -> np.ndarray:
    """memory: [(prompt, files set)] from the same repository (other tasks only)."""
    out = np.zeros(len(files), np.float32)
    if not memory:
        return out
    q = qemb(prompt)
    M = np.stack([qemb(p) for p, _ in memory])
    sims = M @ q
    pos = {f: i for i, f in enumerate(files)}
    for j in np.argsort(-sims)[:k]:
        w = max(float(sims[j]), 0) ** 2
        for f in memory[j][1]:
            if f in pos:
                out[pos[f]] += w
    return out


def prior_scores(files: list, memory: list) -> np.ndarray:
    out = np.zeros(len(files), np.float32)
    pos = {f: i for i, f in enumerate(files)}
    for _, fs in memory:
        for f in fs:
            if f in pos:
                out[pos[f]] += 1
    return out


# ------------------------------------------------------------------ fusion
def design(f: dict, names: list) -> np.ndarray:
    cols = []
    for n in names:
        x = f[n]
        cols.append(RT.norm(x))
        cols.append(1.0 / (1.0 + RT.rank_of(x)))
    return np.stack(cols, 1)


def fit(rows: list, names: list, C: float = 0.3):
    """rows: [(feature dict, files, target set)] -> logistic regression over per-file features.
    Negatives are subsampled to the top-50 of any scorer (the candidates that compete)."""
    X, y = [], []
    for f, files, tgt in rows:
        D = design(f, names)
        cand = set()
        for n in names:
            cand |= set(np.argsort(-f[n])[:50].tolist())
        for i in sorted(cand | {files.index(t) for t in tgt if t in files}):
            X.append(D[i]); y.append(files[i] in tgt)
    X, y = np.asarray(X), np.asarray(y)
    m = LogisticRegression(C=C, max_iter=2000, class_weight="balanced")
    m.fit(X, y)
    return m


def fused(m, f: dict, names: list) -> np.ndarray:
    return m.decision_function(design(f, names))


# ------------------------------------------------------------------ data
def main():
    S = data.load()
    info = TG.annotate(S)
    res = {}
    by_sid = {s.sid: s for s in S}

    # ---------------- A. task level (all tasks), target gold src (+ tests)
    A = collections.defaultdict(lambda: collections.defaultdict(list))
    task_rows = collections.defaultdict(list)  # repo -> rows for fusion
    for tid, F in FEATS.items():
        t = R.tasks(F["repo"])[tid]
        G = {p for p in t.get("src_files", []) if p in F["files"]}
        GT = G | {p for p in t.get("test_files", []) if p in F["files"]}
        task_rows[F["repo"]].append((tid, F, G, GT))
    fusion_names = BASE_FEATS
    for held in REPOS:
        train = [(F["f"], F["files"], GT) for r in REPOS if r != held for _, F, G, GT in task_rows[r]]
        m = fit(train, fusion_names)
        for tid, F, G, GT in task_rows[held]:
            sc = dict(F["f"])
            sc["fusion_loro"] = fused(m, F["f"], fusion_names)
            sc["rrf_bm25_code16"] = 1 / (60 + RT.rank_of(F["f"]["bm25"])) + 1 / (60 + RT.rank_of(F["f"]["code16"]))
            sc["random"] = rng0.random(len(F["files"]))
            for name, x in sc.items():
                for k in (1, 5, 10):
                    for tn, tg in (("G", G), ("GT", GT)):
                        r = recall(x, F["files"], tg, k)
                        if r is not None:
                            A[name][f"{tn}@{k}"].append(r)
                            A[name][f"{tn}@{k}:{held}"].append(r)
    res["A_task_level"] = {n: {k: boot(v) for k, v in d.items() if ":" not in k} for n, d in A.items()}
    res["A_task_level_by_repo"] = {n: {k: round(float(np.mean(v)), 3) for k, v in d.items() if ":" in k and k.startswith("GT@5")} for n, d in A.items()}
    res["A_n_tasks"] = sum(len(v) for v in task_rows.values())
    print("A. task level (n=%d tasks)" % res["A_n_tasks"])
    for n, d in sorted(res["A_task_level"].items(), key=lambda kv: -kv[1]["GT@5"][0]):
        print(f"  {n:18s} G@5 {d['G@5']}  G@10 {d['G@10']}  GT@5 {d['GT@5']}  GT@10 {d['GT@10']}")

    # ---------------- B. session level
    split = {r: R.split(r) for r in REPOS}
    train_tasks = {r: set(split[r]["train"]) for r in REPOS}
    test_tasks = {r: set(split[r]["test"]) for r in REPOS}
    memory_all = collections.defaultdict(list)  # repo -> [(task, prompt, files, arm, model)]
    for s in S:
        if s.sid in info:
            memory_all[s.repo].append((s.task, s.prompt, trace_files(s, info[s.sid]), s.arm, s.phase))

    def train_memory(repo, n=None, seed=0):
        mem = [(p, fs) for (t, p, fs, arm, ph) in memory_all[repo] if arm == "train" and t in train_tasks[repo]]
        if n is not None and n < len(mem):
            idx = np.random.default_rng(seed).choice(len(mem), n, replace=False)
            mem = [mem[i] for i in idx]
        return mem

    def grown_memory(repo, task):
        # every other task's sessions (any arm), leave-task-out: "a repository with more history"
        return [(p, fs) for (t, p, fs, arm, ph) in memory_all[repo] if t != task]

    def scores_for(task, repo, prompt, mem):
        F = FEATS[task]
        sc = dict(F["f"])
        sc["knn"] = knn_scores(prompt, F["files"], mem)
        sc["prior"] = prior_scores(F["files"], mem)
        return F, sc

    # fusion trained on the split's train sessions (label = what the train session touched that mattered)
    def train_rows(repos, mem_fn):
        rows = []
        for s in S:
            if s.arm != "train" or s.repo not in repos or s.sid not in info or s.task not in FEATS:
                continue
            mem = [(p, fs) for p, fs in mem_fn(s.repo) if p != s.prompt]
            F, sc = scores_for(s.task, s.repo, s.prompt, mem)
            tgt = info[s.sid]["E"] | info[s.sid]["pre"]
            rows.append((sc, F["files"], tgt))
        return rows

    names_trace = BASE_FEATS + ["knn", "prior"]
    m_in = fit(train_rows(REPOS, train_memory), names_trace)
    m_loro = {h: fit(train_rows([r for r in REPOS if r != h], train_memory), BASE_FEATS) for h in REPOS}
    m_content = fit(train_rows(REPOS, train_memory), BASE_FEATS)

    held = [s for s in S if s.phase == "eval" and s.arm == "control" and s.sid in info
            and s.task in test_tasks[s.repo] and s.task in FEATS]
    print(f"B. held-out control sessions: {len(held)} "
          f"({collections.Counter((s.repo, s.model) for s in held)})")

    def all_scorers(s):
        F, sc = scores_for(s.task, s.repo, s.prompt, train_memory(s.repo))
        sc["fusion"] = fused(m_in, sc, names_trace)
        sc["fusion_content"] = fused(m_content, sc, BASE_FEATS)
        sc["fusion_loro"] = fused(m_loro[s.repo], sc, BASE_FEATS)
        g = dict(sc)
        g["knn"] = knn_scores(s.prompt, F["files"], grown_memory(s.repo, s.task))
        g["prior"] = prior_scores(F["files"], grown_memory(s.repo, s.task))
        sc["knn_grown"] = g["knn"]
        sc["fusion_grown"] = fused(m_in, g, names_trace)
        return F, sc

    B = collections.defaultdict(lambda: collections.defaultdict(list))
    C = collections.defaultdict(lambda: collections.defaultdict(list))
    P = collections.defaultdict(list)  # pass-rate instrument: sessions that never edited a gold file
    for s in held:
        d = info[s.sid]
        F, sc = all_scorers(s)
        files = F["files"]
        cm = data.hcost(s.sid)
        per_call = cm.usd_per_call()
        touched = set().union(*d["access"])
        for name, x in list(sc.items()) + [("oracle", None)]:
            for k in KS:
                if name == "oracle":
                    H = set(d["T"])
                else:
                    H = {files[i] for i in np.argsort(-x, kind="stable")[:k]}
                    for tn in ("E", "N", "T"):
                        r = recall(x, files, d[tn], k)
                        if r is not None:
                            B[name][f"{tn}@{k}"].append(r)
                inj = HINT_HEADER + HINT_TOKENS_PER_PATH * len(H)
                carry = cm.injection(inj, -1)[0]
                ca, ga = TG.savings(s, TG.removable(s, d, H, tier="all"))
                cs, gs = TG.savings(s, TG.removable(s, d, H, tier="search"))
                wrong = H - d["T"] - touched  # hinted files the agent never opened: a misfire read's cost
                mis = sum(per_call + cm.injection(int(min(len(R.text(s.repo, d["tree"][f])) / 4, 25000)), -1)[0]
                          for f in wrong)
                for sc_name, (p_f, tier_c, tier_g, q) in SCEN.items():
                    c = (ca if tier_c == "all" else cs) * p_f - q * len(wrong)
                    g = (ga if tier_g == "all" else gs) * p_f - carry - q * mis
                    C[(name, sc_name)][f"calls@{k}"].append(c)
                    C[(name, sc_name)][f"usd@{k}"].append(g)
                    C[(name, sc_name)][f"base_usd@{k}"].append(s.dollars)
                    C[(name, sc_name)][f"base_calls@{k}"].append(len(s.calls))
                    C[(name, sc_name)][f"repo@{k}"].append(f"{s.repo}/{s.model}")
                C[(name, "carry")][f"usd@{k}"].append(carry)
                C[(name, "carry")][f"base_usd@{k}"].append(s.dollars)
        if d["G"] and not (d["G"] & d["E"]):
            top = {files[i] for i in np.argsort(-sc["fusion"])[:5]}
            P["miss_gold"].append(1)
            P["gold_in_top5"].append(bool(d["G"] <= top))
        P["n"].append(1)
        P["passed"].append(bool(s.passed))
        P["passed_given_gold_edit"].append(bool(s.passed) if d["G"] & d["E"] else None)
    res["B_session_recall"] = {n: {k: boot(v) for k, v in d.items()} for n, d in B.items()}
    res["B_n_sessions"] = len(held)
    print("B. recall (held-out control sessions)")
    for n, d in sorted(res["B_session_recall"].items(), key=lambda kv: -kv[1]["T@5"][0]):
        print(f"  {n:16s} E@5 {d['E@5']} E@10 {d['E@10']} N@5 {d['N@5']} N@10 {d['N@10']} T@5 {d['T@5']} T@10 {d['T@10']}")
    res["C_counterfactual"] = collections.defaultdict(dict)
    print("C. counterfactual path hint at the prompt: calls removed/session and % of session $ saved, net")
    for (n, scn), d in C.items():
        out = {}
        for k in KS:
            if f"usd@{k}" not in d:
                continue
            out[f"usd@{k}"] = boot(d[f"usd@{k}"])
            out[f"usd_pct@{k}"] = ratio_boot(d[f"usd@{k}"], d[f"base_usd@{k}"])
            if scn != "carry":
                out[f"calls@{k}"] = boot(d[f"calls@{k}"])
                out[f"calls_pct@{k}"] = ratio_boot(d[f"calls@{k}"], d[f"base_calls@{k}"])
                byr = {}
                for r in sorted(set(d[f"repo@{k}"])):
                    idx = [i for i, x in enumerate(d[f"repo@{k}"]) if x == r]
                    byr[r] = dict(n=len(idx), calls=round(float(np.mean([d[f"calls@{k}"][i] for i in idx])), 2),
                                  usd_pct=round(float(np.sum([d[f"usd@{k}"][i] for i in idx]) / np.sum([d[f"base_usd@{k}"][i] for i in idx])), 3))
                out[f"by_repo@{k}"] = byr
        res["C_counterfactual"][n][scn] = out
    for scn in list(SCEN) + ["carry"]:
        print(f" scenario {scn}: {SCEN.get(scn, 'hint carry cost only')}")
        rows = [(n, v[scn]) for n, v in res["C_counterfactual"].items() if scn in v]
        for n, d in sorted(rows, key=lambda kv: -kv[1]["usd_pct@5"][0]):
            line = f"   {n:16s}"
            for k in KS:
                if f"usd@{k}" in d:
                    line += f" | k={k}: " + (f"calls {d['calls@' + str(k)][0]:+.2f} [{d['calls@' + str(k)][1]:+.2f},{d['calls@' + str(k)][2]:+.2f}] " if "calls@" + str(k) in d else "") + f"$% {100 * d['usd_pct@' + str(k)][0]:+.1f} [{100 * d['usd_pct@' + str(k)][1]:+.1f},{100 * d['usd_pct@' + str(k)][2]:+.1f}]"
            print(line)
    pg = [x for x in P["passed_given_gold_edit"] if x is not None]
    res["P_pass_instrument"] = dict(
        sessions=len(P["n"]), pass_rate=round(float(np.mean(P["passed"])), 3),
        missed_gold=len(P["miss_gold"]), missed_gold_share=round(len(P["miss_gold"]) / len(P["n"]), 3),
        gold_in_fusion_top5_when_missed=round(float(np.mean(P["gold_in_top5"])), 3) if P["gold_in_top5"] else None,
        pass_rate_when_gold_edited=round(float(np.mean(pg)), 3))
    print("P.", res["P_pass_instrument"])

    # ---------------- D. learning curve (kNN memory size), in-repo, fusion fixed
    D = {}
    for n in (0, 1, 2, 5, 10):
        rec, usd, base = [], [], []
        for s in held:
            d = info[s.sid]
            vals_r, vals_u = [], []
            for seed in range(3):
                mem = train_memory(s.repo, n, seed) if n else []
                F, sc = scores_for(s.task, s.repo, s.prompt, mem)
                x = fused(m_in, sc, names_trace)
                vals_r.append(recall(x, F["files"], d["T"], 5) or 0)
                H = {F["files"][i] for i in np.argsort(-x)[:5]}
                vals_u.append(TG.savings(s, TG.removable(s, d, H), HINT_HEADER + 5 * HINT_TOKENS_PER_PATH)[1])  # optimistic
                if n == 0:
                    break
            rec.append(np.mean(vals_r)); usd.append(np.mean(vals_u)); base.append(s.dollars)
        D[n] = dict(T5=boot(rec), usd=boot(usd), usd_pct=ratio_boot(usd, base))
        print(f"D. memory {n:2d} sessions/repo: T@5 {D[n]['T5']}  $ {D[n]['usd']}  %$ {D[n]['usd_pct']}")
    res["D_learning_curve"] = D

    # ---------------- E. latency (per query, from a precomputed index)
    res["E_latency_ms"] = latency()
    print("E. latency ms", res["E_latency_ms"])
    (RT.CACHE / "results.json").write_text(json.dumps(res, indent=1))
    (Path(__file__).parent / "results.json").write_text(json.dumps(res, indent=1))
    return res


def latency():
    """Per-query cost of the hook-time part, with everything else precomputed at learn time:
    BM25 sparse matrix, chunk embedding matrix (float16), co-change matrix, kNN memory matrix."""
    out = {}
    for repo in ("click", "vue-core"):
        t = next(iter(R.tasks(repo).values()))
        ix = RT.index(repo, t["base"])
        ix.bm25("warm"); ix.cochange()
        owner, E = [], []
        for i, f in enumerate(ix.files):
            e = RT.blob_chunks_emb(repo, "code16", f, ix.tree[f])
            E.append(e); owner += [i] * len(e)
        E = np.concatenate(E).astype(np.float32); owner = np.asarray(owner)
        mem = np.random.default_rng(0).standard_normal((500, E.shape[1])).astype(np.float32)
        Cm, hot = ix.cochange()
        ts = []
        for tt in list(R.tasks(repo).values())[:20]:
            q = tt["prompt"]
            t0 = time.perf_counter()
            b = ix.bm25(q)
            qv = RT.model("code16").encode([q])[0]
            s = E @ qv
            fe = np.full(len(ix.files), -1.0, np.float32)
            np.maximum.at(fe, owner, s)
            _ = mem @ qv
            z = RT.norm(b) + RT.norm(fe)
            co = RT.propagate(Cm, z)
            top = np.argsort(-(z + RT.norm(co)))[:5]
            ts.append((time.perf_counter() - t0) * 1000)
        out[repo] = dict(files=len(ix.files), chunks=int(len(E)), median=round(float(np.median(ts)), 2),
                         p95=round(float(np.percentile(ts, 95)), 2))
    return out


if __name__ == "__main__":
    main()
