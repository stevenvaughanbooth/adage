"""Compare action abstractions on the RepoBench traces: compression, cross-task recurrence,
held-out coverage and next-action prediction.

    python3 evaluate.py [--refresh] [--out results.json]

Methods (labels of Bash calls; every other tool call gets the same shared label in every method,
e.g. `edit:src`, `read:test:edited`, so only the command abstraction differs):

    literal        the command string (sandbox path replaced by $R)
    argv           shell-normalised segments (no heredoc bodies, cd, redirections, pipes)
    headsub        program + subcommand of the first segment (pattern.rs `HeadSub`, approx.)
    lgg-strict     typed-hole skeleton, flags literal and ordered
    lgg-loose      + star folding, flags as a set, output filters dropped
    emb-hash@τ     hashed-feature embeddings of the command, clustered at τ
    emb-potion@τ   potion-base-8M embeddings of the command, clustered at τ
    emb-potion+out@τ  … of the command plus the first 300 characters of its output
    grammar-L0..L3 role | env class | scope | relation; L3e with the concrete env (venv, uv, npx…)
    combo          grammar-L2 × LGG-loose argument signature (hole types only) ((a)+(b))

Metrics:
    cover90     distinct labels covering 90% of Bash calls (per repo, summed), and distinct labels
    recur       share of a repo's distinct labels seen in ≥ 2 distinct tasks; `recur_calls`: share of
                calls whose label is one of those
    xrepo       share of labels (pooled) seen in ≥ 2 repositories
    heldout     task-grouped 5-fold CV within each repo: `seen` = share of held-out Bash calls whose
                label occurs in the fit folds; `acc` = top-1 accuracy of the next Bash call's label
                from (previous call's label, its outcome), backed off to the previous label and the
                unigram; `base` = the fit folds' majority label; `kappa` = (acc − base) / (1 − base);
                `xent` = interpolated cross-entropy (bits; unseen labels share one UNK class, so
                compare it only within a column), `ig` = bits saved against the fit unigram
    train→eval  the same, fitting on the 7–10 `train` sessions per repo and testing on eval sessions
    *_ctx       the same model, but every method predicts one fixed, method-neutral target (the next
                Bash call's HeadSub identity) from its own context labels. This isolates how much
                the abstraction helps as a *trigger*: coarser labels cannot win by being easier to
                predict.
"""
from __future__ import annotations

import argparse
import collections
import json
import math
import os
import re
import sys

import numpy as np

import corpus
import embed
import grammar as G
import lgg
import shell

TAUS = [0.80, 0.85, 0.90, 0.95]


def HS(r):
    """The fixed, method-neutral prediction target: the next Bash call's HeadSub identity."""
    return r["headsub"]


def shared_label(a) -> str:
    if a.role == "edit":
        return f"edit:{a.scope}"
    if a.role == "read":
        return f"read:{a.scope}:{a.relation}"
    return a.role


def headsub(command: str, cwd: str) -> str:
    segs = [s for s in shell.segments(command, cwd) if s.argv and s.argv[0] not in ("echo", "export", "pwd")]
    if not segs:
        return "<empty>"
    a = segs[0].argv
    p = a[0]
    if re.match(r"(.*/)?python\d?(\.\d+)?$", p) and len(a) > 2 and a[1] == "-m":
        return f"{p} -m {a[2]}"
    if p in ("npm", "pnpm", "yarn", "git", "go", "make", "uv", "cargo", "npx", "pip", "pip3") and len(a) > 1:
        return f"{p} {a[1]}"
    return p


def build_items(L):
    """One record per call; Bash calls carry every method's deterministic labels."""
    sessions = []
    for l in L:
        s = l.sess
        recs = []
        for c, a in zip(s.calls, l.acts):
            r = {"bash": c.tool in ("Bash", "bash"), "outcome": a.outcome, "shared": shared_label(a)}
            if r["bash"]:
                cmd = c.command
                lit = re.sub(r"[ \t]+", " ", cmd.replace(s.cwd.rstrip("/"), "$R")).strip()
                segs = shell.segments(cmd, s.cwd)
                r["literal"] = lit
                r["argv"] = " && ".join(" ".join(x.argv) for x in segs) or "<empty>"
                r["headsub"] = headsub(cmd, s.cwd)
                r["lgg-strict"] = lgg.skeleton(cmd, s.cwd, l.facts, loose=False)
                r["lgg-loose"] = lgg.skeleton(cmd, s.cwd, l.facts, loose=True)
                for k in range(4):
                    r[f"grammar-L{k}"] = a.level(k)
                r["grammar-L3e"] = a.level(3, detail=True)
                r["intent"] = a.intent()
                holes = sorted(set(re.findall(r"<[A-Z]+(?::\w+)?>", r["lgg-loose"])))
                r["combo"] = a.level(2) + "|" + ",".join(holes)
                r["text"] = lit[:600]
                r["text_out"] = lit[:400] + "\n" + re.sub(r"\s+", " ", (c.out or "")[:300])
                r["act"] = a
                r["command"] = cmd
            recs.append(r)
        sessions.append({"repo": s.repo, "task": s.task, "sid": s.sid, "phase": s.phase, "root": s.root,
                         "model": s.model, "recs": recs})
    return sessions


DET = ["literal", "argv", "headsub", "lgg-strict", "lgg-loose", "grammar-L0", "grammar-L1", "grammar-L2", "grammar-L3", "grammar-L3e", "intent", "combo"]
EMB = [("emb-hash", "text", embed.hashed), ("emb-potion", "text", embed.potion), ("emb-potion+out", "text_out", embed.potion)]


class EmbCache:
    def __init__(self):
        self.v = {}

    def get(self, name, fn, texts):
        miss = [t for t in texts if (name, t) not in self.v]
        if miss:
            X = fn(miss)
            for t, x in zip(miss, X):
                self.v[(name, t)] = x
        return np.stack([self.v[(name, t)] for t in texts])


def emb_labels(sessions, fit_idx, test_idx, name, field, fn, tau, cache: EmbCache, repo: str):
    """Cluster the fit sessions' distinct texts; assign every Bash rec a label (test: nearest centroid ≥ τ)."""
    fit_texts = sorted({r[field] for i in fit_idx for r in sessions[i]["recs"] if r["bash"]})
    lab = {}
    if fit_texts:
        X = cache.get(name, fn, fit_texts)
        cl = embed.cluster(X, tau)
        for t, c in zip(fit_texts, cl):
            lab[t] = f"{repo}#{c}"
        ids, C = embed.centroids(X, cl)
    test_texts = sorted({r[field] for i in test_idx for r in sessions[i]["recs"] if r["bash"]} - set(lab))
    if test_texts:
        Xt = cache.get(name, fn, test_texts)
        if fit_texts:
            asg = embed.assign(Xt, ids, C, tau)
        else:
            asg = [None] * len(test_texts)
        for t, a in zip(test_texts, asg):
            lab[t] = f"{repo}#{a}" if a is not None else f"{repo}#new:{t}"
    return lambda r: lab[r[field]]


def cover(counts: collections.Counter, q=0.9) -> int:
    tot, acc = sum(counts.values()), 0
    for k, (_, n) in enumerate(counts.most_common(), 1):
        acc += n
        if acc >= q * tot:
            return k
    return len(counts)


def static_metrics(sessions, labeller) -> dict:
    by_repo = collections.defaultdict(list)
    for s in sessions:
        by_repo[s["repo"]].append(s)
    cov, dist, rec_l, rec_c, tot_l, tot_c = 0, 0, 0, 0, 0, 0
    per_repo = {}
    label_repos = collections.defaultdict(set)
    for repo, ss in by_repo.items():
        cnt, tasks = collections.Counter(), collections.defaultdict(set)
        for s in ss:
            for r in s["recs"]:
                if r["bash"]:
                    lb = labeller(r)
                    cnt[lb] += 1
                    tasks[lb].add(s["task"])
                    label_repos[lb].add(repo)
        c90 = cover(cnt)
        rl = sum(1 for k in cnt if len(tasks[k]) >= 2)
        rc = sum(n for k, n in cnt.items() if len(tasks[k]) >= 2)
        per_repo[repo] = {"cover90": c90, "distinct": len(cnt), "recur": rl / len(cnt), "recur_calls": rc / sum(cnt.values())}
        cov += c90
        dist += len(cnt)
        rec_l += rl
        tot_l += len(cnt)
        rec_c += rc
        tot_c += sum(cnt.values())
    xrepo = sum(1 for v in label_repos.values() if len(v) >= 2) / max(1, len(label_repos))
    return {"cover90": cov, "distinct": dist, "recur": rec_l / tot_l, "recur_calls": rec_c / tot_c, "xrepo": xrepo, "per_repo": per_repo}


def seq_of(s, labeller, target=None):
    """(context label, outcome, is_bash, target label) per call."""
    target = target or labeller
    return [((labeller(r) if r["bash"] else r["shared"]), (r["outcome"] if r["bash"] else ""), r["bash"],
             (target(r) if r["bash"] else None)) for r in s["recs"]]


def predictive(sessions, fit_idx, test_idx, labeller, target=None):
    """Predict the next Bash call's target label from the previous call's (context label, outcome).

    `target=None`: the method predicts its own labels (self-prediction). With a fixed `target`
    (the next call's HeadSub identity) every method is scored on the same outcome, so only the
    context abstraction differs. The model: counts at (label, outcome) → label → unigram, used at
    the first level with ≥ 2 observations (top-1), and for cross-entropy a Jelinek-Mercer
    interpolation (0.6 context, 0.4 unigram) over the fit vocabulary plus one UNK class.
    """
    uni, big, tri = collections.Counter(), collections.defaultdict(collections.Counter), collections.defaultdict(collections.Counter)
    for i in fit_idx:
        prev = ("<s>", "")
        for lb, oc, b, tg in seq_of(sessions[i], labeller, target):
            if b:
                uni[tg] += 1
                big[prev[0]][tg] += 1
                tri[prev][tg] += 1
            prev = (lb, oc)
    n = sum(uni.values())
    V = len(uni) + 1
    maj = uni.most_common(1)[0][0] if uni else None
    hits = base = seen = total = ctx_seen = 0
    xe = xe_uni = 0.0

    def pu(t):
        return (uni.get(t, 0) + 0.5) / (n + 0.5 * V)

    for i in test_idx:
        prev = ("<s>", "")
        for lb, oc, b, tg in seq_of(sessions[i], labeller, target):
            if b and n:
                total += 1
                d = None
                for tab, key in ((tri, prev), (big, prev[0])):
                    c = tab.get(key)
                    if c and sum(c.values()) >= 2:
                        d = c
                        break
                ctx_seen += d is not None
                dd = d or uni
                pred = dd.most_common(1)[0][0]
                hits += pred == tg
                base += maj == tg
                seen += tg in uni
                t = tg if tg in uni else "<unk>"
                p_u = pu(t)
                p = 0.6 * (dd.get(t, 0) / sum(dd.values())) + 0.4 * p_u if d is not None else p_u
                xe -= math.log2(p)
                xe_uni -= math.log2(p_u)
            prev = (lb, oc)
    T = max(1, total)
    return {"n": total, "acc": hits / T, "base": base / T, "seen": seen / T, "ctx_seen": ctx_seen / T,
            "xent": xe / T, "xent_uni": xe_uni / T, "ig": (xe_uni - xe) / T}


def folds(sessions, k=5):
    """Task-grouped folds per repo: tasks sorted by id, dealt round-robin (deterministic)."""
    by_repo = collections.defaultdict(list)
    for i, s in enumerate(sessions):
        by_repo[s["repo"]].append(i)
    out = []
    for repo, idx in sorted(by_repo.items()):
        tasks = sorted({sessions[i]["task"] for i in idx})
        f_of = {t: j % k for j, t in enumerate(tasks)}
        for f in range(k):
            test = [i for i in idx if f_of[sessions[i]["task"]] == f]
            fit = [i for i in idx if f_of[sessions[i]["task"]] != f]
            out.append((repo, fit, test))
    return out


def train_eval(sessions):
    by_repo = collections.defaultdict(list)
    for i, s in enumerate(sessions):
        by_repo[s["repo"]].append(i)
    out = []
    for repo, idx in sorted(by_repo.items()):
        fit = [i for i in idx if sessions[i]["phase"] == "train"]
        test = [i for i in idx if sessions[i]["phase"] == "eval"]
        out.append((repo, fit, test))
    return out


def pooled(results):
    n = sum(r["n"] for r in results)
    agg = {"n": n}
    for k in ("acc", "base", "seen", "ctx_seen", "xent", "xent_uni", "ig"):
        agg[k] = sum(r[k] * r["n"] for r in results) / max(1, n)
    agg["kappa"] = (agg["acc"] - agg["base"]) / max(1e-9, 1 - agg["base"])
    return agg


def run(refresh=False):
    L = corpus.load(refresh=refresh)
    sessions = build_items(L)
    cache = EmbCache()
    res = {"methods": {}}

    # trace facts (sanity: the architecture review's numbers)
    tr = [r for s in sessions for r in s["recs"] if r["bash"] and r["act"].role == "test-run"]
    lits = collections.Counter(r["literal"] for r in tr)
    res["facts"] = {
        "sessions": len(sessions), "calls": sum(len(s["recs"]) for s in sessions),
        "bash": sum(1 for s in sessions for r in s["recs"] if r["bash"]), "test_runs": len(tr),
        "test_unique_share": sum(1 for r in tr if lits[r["literal"]] == 1) / len(tr),
        "test_distinct_share": len(lits) / len(tr),
        "test_narrowed": sum(1 for r in tr if r["act"].scope in ("file", "test-id", "package")) / len(tr),
        "test_piped": sum(1 for r in tr if r["act"].piped) / len(tr),
        "roles": collections.Counter(r["act"].role for s in sessions for r in s["recs"] if r["bash"]).most_common(),
        "test_env": collections.Counter(r["act"].env for r in tr).most_common(),
        "test_scope": collections.Counter(r["act"].scope for r in tr).most_common(),
        "test_relation": collections.Counter(r["act"].relation for r in tr).most_common(),
    }

    splits = {"cv5": folds(sessions), "train_eval": train_eval(sessions)}
    for m in DET:
        lab = (lambda m: lambda r: r[m])(m)
        out = {"static": static_metrics(sessions, lab)}
        for sp, fl in splits.items():
            out[sp] = pooled([predictive(sessions, fit, test, lab) for _, fit, test in fl])
            out[sp + "_ctx"] = pooled([predictive(sessions, fit, test, lab, HS) for _, fit, test in fl])
        res["methods"][m] = out
        print(m, json.dumps({k: (v if k != "static" else {kk: vv for kk, vv in v.items() if kk != "per_repo"}) for k, v in out.items()}), file=sys.stderr)

    for name, field, fn in EMB:
        for tau in TAUS:
            key = f"{name}@{tau:.2f}"
            # static metrics: cluster everything per repo (transductive)
            by_repo = collections.defaultdict(list)
            for i, s in enumerate(sessions):
                by_repo[s["repo"]].append(i)
            labs = {}
            for repo, idx in by_repo.items():
                f = emb_labels(sessions, idx, [], name, field, fn, tau, cache, repo)
                for i in idx:
                    for r in sessions[i]["recs"]:
                        if r["bash"]:
                            labs[id(r)] = f(r)
            out = {"static": static_metrics(sessions, lambda r: labs[id(r)])}
            for sp, fl in splits.items():
                parts = []
                for repo, fit, test in fl:
                    f = emb_labels(sessions, fit, test, name, field, fn, tau, cache, repo)
                    parts.append((predictive(sessions, fit, test, f), predictive(sessions, fit, test, f, HS)))
                out[sp] = pooled([a for a, _ in parts])
                out[sp + "_ctx"] = pooled([b for _, b in parts])
            res["methods"][key] = out
            print(key, json.dumps({k: (v if k != "static" else {kk: vv for kk, vv in v.items() if kk != "per_repo"}) for k, v in out.items()}), file=sys.stderr)
    return res, sessions


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--refresh", action="store_true")
    ap.add_argument("--out", default=os.path.join(os.path.dirname(__file__), "results", "metrics.json"))
    a = ap.parse_args()
    res, _ = run(a.refresh)
    os.makedirs(os.path.dirname(a.out), exist_ok=True)
    with open(a.out, "w") as f:
        json.dump(res, f, indent=1, default=str)
