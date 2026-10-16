"""Held-out evaluation of the learned offload policy (split and leave-one-repo-out), Haiku and
Sonnet separately, under the harness cost model (offload.py semantics).

    python3 evaluate.py            # writes results/learned.json and caches predictions

Policy at each decision point t: take the best candidate (learner.Model.top); deliver it when
  (a) p ≥ θ   (θ chosen on the training pool by cross-fitting, never on held-out), and
  (b) the expected value is positive: p · V_turn > (1 − p) · C_inject, where the hook knows the
      result size (it ran the command) and the context size (the transcript), and the remaining
      responses are estimated from the pool's mean session length.
Once delivered, a key is not delivered again until the next edit.
"""
from __future__ import annotations

import json
import pickle
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

from data import corpus, model_of, records
from learner import Model, authoring, label_window, template
from offload import TOK_PER_CHAR, WRAPPER, aggregate, fmt, hit_tokens, result_tokens, session_value

sys.path.insert(0, "/home/user/adage/evals/lab/research")
from harness import costmodel as CM  # noqa: E402
from harness.leakage import assert_clean  # noqa: E402

CACHE = Path("/tmp/adage-research/execution-offload")
CACHE.mkdir(parents=True, exist_ok=True)
THETAS = (0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9)


def read_size_table(sessions):
    """Mean output chars of each Read key (to price a delivered-but-unwanted Read); pool-only."""
    tab = defaultdict(list)
    for s in sessions:
        for r in records(s):
            if r.tool == "Read":
                tab[r.args.get("file_path", "")].append(r.out_chars)
    allm = float(np.mean([v for l in tab.values() for v in l])) if tab else 6000.0
    return {k: float(np.mean(v)) for k, v in tab.items()}, allm


def cand_tokens(cand, R, model, sizes):
    if cand.i >= 0 or cand.out_chars:
        chars = cand.out_chars
    else:
        chars = sizes[0].get(cand.args.get("file_path", ""), sizes[1])
    return int(chars * TOK_PER_CHAR[model] + len(cand.text) / 4 + WRAPPER)


def predictions(model_obj, S):
    return {s.sid: model_obj.top(s) for s in S}


def policy_rows(S, preds, theta, model, sizes, mean_len, value_rule=True, p_skip=1.0, harm_calls=0.0,
                select=None):
    """Score the policy. select(cand_rec, s) → bool restricts which offloads are allowed."""
    rows, cl, hitlog = [], [], []
    for s in S:
        R = records(s)
        cm = CM.of(s)
        rt = CM.rates(s.model)
        r_read, r_out, r_in = rt[3] / 1e6, rt[1] / 1e6, rt[0] / 1e6
        delivered = set()
        hits, misses, removed = [], [], set()
        for t, key, src, cand, p in preds[s.sid]:
            if t >= 0 and authoring(R[t]):
                delivered = set()
            if key is None or p < theta or key in delivered:
                continue
            if select is not None and not select(cand, s):
                continue
            tok = cand_tokens(cand, R, model, sizes)
            if value_rule:
                j = R[t].resp if t >= 0 else 0
                rs = s.responses[j] if s.responses else None
                ctx = (rs.cache_read + rs.cache_write + rs.input) if rs else 15000
                remaining = max(3.0, mean_len - (t + 1))
                v_turn = ctx * r_read + 150 * r_out
                c_inj = tok * (2 * r_in + r_read * remaining)
                if p * v_turn <= (1 - p) * c_inj:
                    continue
            delivered.add(key)
            k = label_window(R, t, key)
            if k is not None and k not in removed:
                removed.add(k)
                hits.append((t, k, hit_tokens(s, R[k])))  # the hook returns what the agent's call returned
                hitlog.append((s.sid, t, k, src, R[k].tool, R[k].text[:80], p))
            else:
                misses.append((t, tok))
        rows.append(session_value(s, hits, misses, model, p_skip=p_skip, harm_calls=harm_calls))
        cl.append(s.cluster)
    return rows, cl, hitlog


_THETA = {}


def pick_theta(pool_sessions, model, folds=3, seed=0):
    key = tuple(sorted(s.sid for s in pool_sessions))  # θ is a property of the pool (Haiku traces)
    if key not in _THETA:
        _THETA[key] = _pick_theta(pool_sessions, "haiku", folds, seed)
    return _THETA[key]


def _pick_theta(pool_sessions, model, folds=4, seed=0):
    """Cross-fit on the pool by task: choose θ with the best mean net $ share on the held-out folds."""
    tasks = sorted({(s.repo, s.task) for s in pool_sessions})
    rng = np.random.default_rng(seed)
    rng.shuffle(tasks)
    fold_of = {t: i % folds for i, t in enumerate(tasks)}
    score = Counter()
    for f in range(folds):
        tr = [s for s in pool_sessions if fold_of[(s.repo, s.task)] != f]
        te = [s for s in pool_sessions if fold_of[(s.repo, s.task)] == f]
        if not tr or not te:
            continue
        m = Model(tr)
        pr = predictions(m, te)
        sizes = read_size_table(tr)
        for th in THETAS:
            rows, cl, _ = policy_rows(te, pr, th, model, sizes, m.pool.mean_len)
            score[th] += sum(r["usd"] for r in rows) / max(sum(r["base_usd"] for r in rows), 1e-9)
    return max(THETAS, key=lambda th: score[th]), dict(score)


def run_protocol(name, pairs, model, theta_mode="cv"):
    """pairs: [(pool_sessions, test_sessions)] → pooled report at chosen θ and a θ sweep."""
    all_rows = defaultdict(list)
    all_cl = defaultdict(list)
    logs = []
    thetas = []
    for pool, test in pairs:
        assert_clean(pool, test)
        m = Model(pool)
        pr = predictions(m, test)
        sizes = read_size_table(pool)
        th, _ = pick_theta(pool, model) if theta_mode == "cv" else (theta_mode, None)
        thetas.append(th)
        for variant, kw in {"chosen": dict(theta=th), **{f"theta={x}": dict(theta=x) for x in THETAS},
                            "chosen p_skip=0.5": dict(theta=th, p_skip=0.5),
                            "chosen harm=1 call/miss": dict(theta=th, harm_calls=1.0),
                            "chosen no value rule": dict(theta=th, value_rule=False)}.items():
            rows, cl, hl = policy_rows(test, pr, model=model, sizes=sizes, mean_len=m.pool.mean_len, **kw)
            all_rows[variant] += rows
            all_cl[variant] += cl
            if variant == "chosen":
                logs += hl
    res = {v: aggregate(all_rows[v], all_cl[v]) for v in all_rows}
    res["thetas"] = thetas
    print(f"== {name} ({model}) thetas={thetas}")
    for v in ("chosen", "chosen p_skip=0.5", "chosen harm=1 call/miss", "chosen no value rule"):
        print("  ", v, "|", fmt(res[v]))
    for x in THETAS:
        a = res[f"theta={x}"]
        print(f"   θ={x}: {100*a['usd_share']:+.1f}% calls {a['calls_saved']:+.2f} hits {a['hits_per_session']:.2f} misses {a['misses_per_session']:.2f}")
    return res, logs


POOL_MAX = 120  # CPU budget: the leave-one-repo-out pool is subsampled, stratified by repository


def subsample(pool, n=POOL_MAX, seed=0):
    if len(pool) <= n:
        return pool
    rng = np.random.default_rng(seed)
    by = defaultdict(list)
    for s in sorted(pool, key=lambda x: x.sid):
        by[s.repo].append(s)
    out = []
    for r, ss in sorted(by.items()):
        k = max(1, round(n * len(ss) / len(pool)))
        out += [ss[i] for i in rng.choice(len(ss), size=min(k, len(ss)), replace=False)]
    return out


def main():
    c = corpus()
    out = {}
    for model in ("haiku", "sonnet"):
        test = c.held_out(model=model)
        repos = sorted({s.repo for s in test})
        # split: pool = every training-phase session (train tasks only, all repositories)
        pool = c.train()
        split_pairs = [(pool, [s for s in test if s.repo == r]) for r in repos]
        res_s, log_s = run_protocol("split", split_pairs, model)
        # leave one repository out: pool = every session (any arm) of the other repositories
        loro_pairs = [(subsample([s for s in c.sessions if s.repo != r]), [s for s in test if s.repo == r])
                      for r in repos]
        res_l, log_l = run_protocol("loro", loro_pairs, model)
        out[model] = {"split": res_s, "loro": res_l}
        pickle.dump({"split": log_s, "loro": log_l}, open(CACHE / f"hits-{model}.pkl", "wb"))
    json.dump(out, open("results/learned.json", "w"), indent=1)


if __name__ == "__main__":
    main()
