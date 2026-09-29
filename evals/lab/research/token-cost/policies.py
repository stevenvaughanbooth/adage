"""Learned output shaping (PreToolUse rewrite): keep the head H and tail T lines of a tool output,
park the rest in a file the agent can read by range. (H, T) is learned per repository and
context from training traces by minimising the cumulative-context $ of what is kept plus the
expected $ of re-fetching lines the agent later referenced.

Counterfactual accounting for one output r (first seen at call k):
  kept      = tokens(head H + tail T + a 40-token pointer note) * carry(k)
  miss      = referenced lines fell in the elided middle
  re-fetch  = the agent locates and reads them: the referenced lines are padded +-10 and merged
              into regions (gap <= 30 lines); cost = (1 + n_regions) extra calls, each re-reading the
              context at call k from the cache and emitting ~150 output tokens, plus the region tokens
              carried from k (conservative: the real re-fetch is later, so cheaper), or, when
              cheaper, one call that re-reads the whole elided middle.
Saved $ = original carry $ - kept $ - re-fetch $.

Context keys (learned back-off): repo|template|ranged -> repo|key -> repo|tool -> all|template ->
all|tool -> all. The template/key come from the lab's normaliser (hand-written; see README).
"""
from __future__ import annotations

import collections
import json
import math
import pickle
import sys

import numpy as np

from common import PRICES, boot, boot_ratio, fam, session_cost

D = pickle.load(open("/tmp/adage-research/token-cost/annotated.pkl", "rb"))
S, CPT = D["sessions"], D["cpt"]
HS = [0, 10, 30, 60, 120, 250, 500]
TS = [0, 10, 30, 60, 120]
GRID = [(h, t) for h in HS for t in TS] + [None]
NOTE_TOK = 40
MIN_OUT, MIN_SESS = 5, 3


def ranged(r):
    inp = r["input"] or {}
    return bool(inp.get("offset") or inp.get("limit"))


def ctx_keys(s, r, loro=False):
    tool = r["key"].split(":")[0]
    keys = [] if loro else [
        f"{s['repo']}|{r['template']}|{ranged(r)}", f"{s['repo']}|{r['key']}", f"{s['repo']}|{tool}"]
    return keys + [f"*|{r['template']}|{ranged(r)}", f"*|{tool}", "*"]


def call_price(s, k):
    p = PRICES[fam(s["model"])]
    c = s["calls"][min(k, len(s["calls"]) - 1)]
    ctx = c["cw"] + c["cr"] + c["inp"]
    return (ctx * p["cr"] + 150 * p["out"]) / 1e6 + 150 * r_carry(s, k)


def r_carry(s, k):
    from common import carry_multiplier
    return carry_multiplier(s, k)


def outcome(s, r, act, pen_mult=1.0):
    """(saved $, missed?) for applying act=(H,T) to output r in session s."""
    if act is None or r["carry"] == 0:
        return 0.0, False
    H, T = act
    lc = r["line_chars"]
    n = len(lc)
    if n <= H + T + 5:
        return 0.0, False
    tpc = CPT[fam(s["model"])]["tok_per_char"]
    orig = r["tok"] * r["carry"]
    kept_chars = sum(lc[:H]) + (sum(lc[n - T:]) if T else 0)
    kept = (kept_chars * tpc + NOTE_TOK) * r["carry"]
    miss = [u for u in r["used"] if H <= u < n - T]
    pen = 0.0
    if miss:
        regs = []
        for u in miss:
            lo, hi = max(H, u - 10), min(n - T - 1, u + 10)
            if regs and lo - regs[-1][1] <= 30:
                regs[-1][1] = max(regs[-1][1], hi)
            else:
                regs.append([lo, hi])
        cp = call_price(s, r["seen_call"])
        reg_tok = sum(sum(lc[a:b + 1]) for a, b in regs) * tpc
        by_region = (1 + len(regs)) * cp + reg_tok * r["carry"]
        mid_tok = (sum(lc) - kept_chars) * tpc
        whole = 2 * cp + mid_tok * r["carry"]
        pen = pen_mult * min(by_region, whole)
    return orig - kept - pen, bool(miss)


def learn(train, loro_repo=None, pen_mult=1.0):
    """Per context key: total train saving per action and support."""
    tab = collections.defaultdict(lambda: np.zeros(len(GRID)))
    sup = collections.defaultdict(set)
    cnt = collections.Counter()
    for s in train:
        if loro_repo and s["repo"] == loro_repo:
            continue
        for r in s["results"]:
            if r["carry"] == 0:
                continue
            vals = np.array([outcome(s, r, a, pen_mult)[0] for a in GRID])
            for key in ctx_keys(s, r, loro=bool(loro_repo)):
                tab[key] += vals
                sup[key].add(s["sid"])
                cnt[key] += 1
    pol = {}
    for key, v in tab.items():
        if cnt[key] >= MIN_OUT and len(sup[key]) >= MIN_SESS:
            best = int(np.argmax(v))
            pol[key] = GRID[best] if v[best] > 0 else None
    return pol


def decide(pol, s, r, loro=False):
    for key in ctx_keys(s, r, loro):
        if key in pol:
            return pol[key]
    return None


def evaluate(pol, sessions, loro=False, pen_mult=1.0):
    saved, gross, misses, shaped, base = [], [], [], [], []
    for s in sessions:
        sv = g = 0.0
        m = n = 0
        for r in s["results"]:
            a = decide(pol, s, r, loro)
            v, miss = outcome(s, r, a, pen_mult)
            if a is not None and v != 0.0:
                n += 1
                m += miss
            sv += v
            g += outcome(s, r, a, 0.0)[0]
        saved.append(sv)
        gross.append(g)
        misses.append(m)
        shaped.append(n)
        base.append(session_cost(s))
    return dict(saved=np.array(saved), gross=np.array(gross), misses=np.array(misses),
                shaped=np.array(shaped), base=np.array(base))


def oracle(sessions):
    """Two ceilings: (a) best (H,T) per output in hindsight (policy-class ceiling, no penalty
    since chosen to cover the referenced lines, or none); (b) delete every unreferenced line."""
    a_, b_ = [], []
    for s in sessions:
        tpc = CPT[fam(s["model"])]["tok_per_char"]
        a = b = 0.0
        for r in s["results"]:
            a += max(outcome(s, r, act)[0] for act in GRID)
            lc = np.array(r["line_chars"], float)
            used = np.zeros(len(lc), bool)
            used[r["used"]] = True
            b += lc[~used].sum() * tpc * r["carry"]
        a_.append(a)
        b_.append(b)
    return np.array(a_), np.array(b_)


def wilson(k, n, z=1.645):
    if n == 0:
        return (float("nan"),) * 3
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return p, c - h, c + h


def summarise(ev, label):
    s, b = ev["saved"], ev["base"]
    k, n = int(ev["misses"].sum()), int(ev["shaped"].sum())
    return dict(label=label, n_sessions=len(s),
                usd_saved_per_session=boot(s), pct_saved=boot_ratio(s, b),
                gross_usd_per_session=boot(ev["gross"]), gross_pct=boot_ratio(ev["gross"], b),
                shaped_outputs_per_session=float(ev["shaped"].mean()),
                miss_rate_per_shaped_output=wilson(k, n),
                sessions_with_any_miss=wilson(int((ev["misses"] > 0).sum()), len(s)),
                refetch_calls_added_per_session_upper=float(ev["misses"].mean() * 2))


def main():
    train = [s for s in S if s["split"] == "train"]
    held = [s for s in S if s["arm"] == "control" and s["split"] == "test"]
    out = {}
    pol = learn(train)
    json.dump({k: v for k, v in sorted(pol.items())}, open("policy_learned.json", "w"), indent=0)
    for f in ("haiku", "sonnet"):
        H = [s for s in held if fam(s["model"]) == f]
        res = {}
        res["learned_per_repo"] = summarise(evaluate(pol, H), "learned per repo (train split)")
        res["learned_pen_x2"] = summarise(evaluate(pol, H, pen_mult=2.0), "same policy, re-fetch penalty x2")
        pol2 = learn(train, pen_mult=2.0)
        res["learned_with_pen_x2_eval_x2"] = summarise(evaluate(pol2, H, pen_mult=2.0), "learned & scored at penalty x2")
        # restricted to one tool family at a time (which part carries the win)
        for fam_name, pred in (("Read only", lambda r: r["key"] == "Read"),
                               ("Bash only", lambda r: r["key"].startswith("Bash:")),
                               ("Grep/Glob only", lambda r: r["key"] in ("Grep", "Glob"))):
            sub = [dict(s, results=[r for r in s["results"] if pred(r)]) for s in H]
            ev = evaluate(pol, sub)
            ev["base"] = np.array([session_cost(s) for s in H])
            res[f"part: {fam_name}"] = summarise(ev, fam_name)
        # leave-one-repo-out: parameters learned only on the other repositories' train traces
        if f == "haiku":
            parts = []
            for repo in sorted({s["repo"] for s in H}):
                pl = learn(train, loro_repo=repo)
                parts.append(evaluate(pl, [s for s in H if s["repo"] == repo], loro=True))
            ev = {k: np.concatenate([p[k] for p in parts]) for k in parts[0]}
            res["leave_one_repo_out"] = summarise(ev, "LORO: learned on the other repos")
        else:
            ev = evaluate(learn(train, loro_repo="__none__"), H, loro=True)
            res["cross_repo_global"] = summarise(ev, "global keys only")
        a, b = oracle(H)
        base = np.array([session_cost(s) for s in H])
        res["ceiling_policy_class"] = dict(usd=boot(a), pct=boot_ratio(a, base))
        res["ceiling_drop_all_unreferenced"] = dict(usd=boot(b), pct=boot_ratio(b, base))
        per_repo = {}
        ev = evaluate(pol, H)
        for repo in sorted({s["dataset"] for s in H}):
            idx = [i for i, s in enumerate(H) if s["dataset"] == repo]
            per_repo[repo] = dict(n=len(idx), usd=boot(ev["saved"][idx]), pct=boot_ratio(ev["saved"][idx], ev["base"][idx]))
        res["per_repo"] = per_repo
        out[f] = res
    json.dump(out, open("policies.json", "w"), indent=1)
    for f, res in out.items():
        print("=====", f)
        for k, v in res.items():
            print(k, json.dumps(v, default=lambda x: round(x, 4))[:600])


if __name__ == "__main__":
    main()
