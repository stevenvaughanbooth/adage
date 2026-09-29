"""Headline evaluation: learned output shaping, net of re-fetch cost, on the common harness's
held-out control sessions, with task-clustered 90% CIs, under both cache TTLs and three harm models.

Actions a hook can take on one tool output (all leave an elision note, so the agent can tell):
  POS(H,T)      keep the first H and last T lines. Read: PreToolUse updatedInput limit=H (T=0 only
                is expressible as a Read limit; T>0 needs PostToolUse). Bash: PreToolUse wrapper
                that tees the full output to a file and prints head/tail (exit code preserved).
  REL(w,h,t)    keep anchor lines +- w (identifiers the agent already wrote, rare in this output),
                plus head h / tail t. Needs the output: PostToolUse `updatedToolOutput`.
  GREP(w)       Read only: window [first grep-hit line - w, last + w] when an earlier output named
                lines of this file (`file:123:`); PreToolUse updatedInput offset/limit.
The learner picks, per context key (repo|template|ranged -> repo|key -> repo|tool -> all|... ->
all), the action (or none) that maximises net training $ under the harm model being evaluated.

Harm models for a miss (a later-referenced line was elided):
  H1  one extra call at the session's mean $/call (waste-anatomy/trim.py's model);
  H2  one extra call re-reading the context at that point + the missed lines (+-10, merged)
      re-entering the context;
  H3  (1 + regions) extra calls + the missed regions' tokens, or 2 calls + the whole elided part
      when cheaper (the conservative model used in policies.py).
"""
from __future__ import annotations

import collections
import json
import os
import pickle
import sys

import numpy as np

from common import PRICES, boot_cluster, fam, harness_sets

D = pickle.load(open("/tmp/adage-research/token-cost/annotated.pkl", "rb"))
S, CPT = D["sessions"], D["cpt"]
ANC = pickle.load(open("/tmp/adage-research/token-cost/anchors.pkl", "rb"))
for s in S:
    for r, (a, g) in zip(s["results"], ANC[s["sid"]]):
        r["anc"], r["grep"] = a, g
        r["lc"] = np.array(r["line_chars"], float)
        if os.environ.get("USE_STRICT"):
            r["used"] = r["used_strict"]  # sensitivity: only verbatim re-use counts as use (optimistic)

POS = [("POS", h, t) for h in (0, 10, 30, 60, 120, 250, 500) for t in (0, 10, 30, 60, 120)]
REL = [("REL", w, h, t) for w in (3, 10, 30) for h in (0, 30) for t in (0, 30)]
GREP = [("GREP", w) for w in (20, 60, 150)]
FAMILIES = {"POS": POS, "REL": REL, "GREP": GREP, "ALL": POS + REL + GREP,
            "PreToolUse-only": [("PPOS", h, t) for _, h, t in POS] + GREP}
NOTE = 40


def price(s, ttl):
    p = dict(PRICES[fam(s["model"])])
    if ttl == "5m":
        p["cw1h"] = p["cw5m"]
    return p


def carry(s, k, ttl):
    n = len(s["calls"])
    if k >= n:
        return 0.0
    p = price(s, ttl)
    return (p["cw1h"] + p["cr"] * (n - 1 - k)) / 1e6


def base_cost(s, ttl):
    p = price(s, ttl)
    return sum((c["inp"] * p["inp"] + c["out"] * p["out"] + c["cw1h"] * p["cw1h"]
                + (c["cw"] - c["cw1h"]) * p["cw5m"] + c["cr"] * p["cr"]) / 1e6 for c in s["calls"])


def call_price(s, k, ttl):
    p = price(s, ttl)
    c = s["calls"][min(k, len(s["calls"]) - 1)]
    return ((c["cw"] + c["cr"] + c["inp"]) * p["cr"] + 150 * p["out"]) / 1e6 + 150 * carry(s, k, ttl)


def keep_mask(r, a):
    n = len(r["lc"])
    m = np.zeros(n, bool)
    if a[0] in ("POS", "PPOS"):
        _, h, t = a
        if n <= h + t + 5:
            return None
        if a[0] == "PPOS" and r["key"] == "Read" and t:
            return None  # a Read limit cannot keep a tail
        m[:h] = True
        if t:
            m[n - t:] = True
    elif a[0] == "REL":
        _, w, h, t = a
        if n < 40:
            return None
        m[:h] = True
        if t:
            m[n - t:] = True
        for i in list(r["anc"]) + list(r["grep"]):
            m[max(0, i - w):i + w + 1] = True
    else:
        if r["key"] != "Read" or not r["grep"] or n < 40:
            return None
        _, w = a
        m[max(0, r["grep"][0] - w):r["grep"][-1] + w + 1] = True
    if m.mean() > 0.9:
        return None
    return m


def outcome(s, r, a, harm, ttl):
    """(net $ saved, gross $ saved, missed?) of action a on output r."""
    if a is None:
        return 0.0, 0.0, False
    k = r["seen_call"]
    cm = carry(s, k, ttl)
    if cm == 0:
        return 0.0, 0.0, False
    m = keep_mask(r, a)
    if m is None:
        return 0.0, 0.0, False
    tpc = CPT[fam(s["model"])]["tok_per_char"]
    lc = r["lc"]
    runs = int((~m & np.r_[True, m[:-1]]).sum())
    removed_tok = lc[~m].sum() * tpc - NOTE * runs
    gross = removed_tok * cm
    miss = [u for u in r["used"] if not m[u]]
    if not miss or harm == "H0":
        return gross, gross, bool(miss)
    if harm == "H1":
        return gross - base_cost(s, ttl) / len(s["calls"]), gross, True
    regs = []
    n = len(lc)
    for u in miss:
        lo, hi = max(0, u - 10), min(n - 1, u + 10)
        if regs and lo - regs[-1][1] <= 30:
            regs[-1][1] = max(regs[-1][1], hi)
        else:
            regs.append([lo, hi])
    reg_tok = sum(lc[x:y + 1].sum() for x, y in regs) * tpc
    cp = call_price(s, k, ttl)
    if harm == "H2":
        pen = cp + reg_tok * cm
    else:
        pen = min((1 + len(regs)) * cp + reg_tok * cm, 2 * cp + lc[~m].sum() * tpc * cm)
    return gross - pen, gross, True


def ranged(r):
    inp = r["input"] or {}
    return bool(inp.get("offset") or inp.get("limit"))


def ctx_keys(s, r, loro):
    tool = r["key"].split(":")[0]
    own = [] if loro else [f"{s['repo']}|{r['template']}|{ranged(r)}", f"{s['repo']}|{r['key']}", f"{s['repo']}|{tool}"]
    return own + [f"*|{r['template']}|{ranged(r)}", f"*|{tool}", "*"]


def learn(train, actions, harm, loro=False, min_out=5, min_sess=3):
    tab = collections.defaultdict(lambda: np.zeros(len(actions)))
    sup, cnt = collections.defaultdict(set), collections.Counter()
    for s in train:
        for r in s["results"]:
            if r["seen_call"] >= len(s["calls"]):
                continue
            v = np.array([outcome(s, r, a, harm, "1h")[0] for a in actions])
            for key in ctx_keys(s, r, loro):
                tab[key] += v
                sup[key].add(s["sid"])
                cnt[key] += 1
    pol = {}
    for key, v in tab.items():
        if cnt[key] >= min_out and len(sup[key]) >= min_sess:
            b = int(np.argmax(v))
            pol[key] = actions[b] if v[b] > 0 else None
    return pol


def apply(pol, sessions, harm, ttl, loro=False):
    rows = []
    for s in sessions:
        net = gross = 0.0
        shaped = miss = 0
        for r in s["results"]:
            a = None
            for key in ctx_keys(s, r, loro):
                if key in pol:
                    a = pol[key]
                    break
            v, g, mi = outcome(s, r, a, harm, ttl)
            if g:
                shaped += 1
                miss += mi
            net += v
            gross += g
        rows.append(dict(net=net, gross=gross, shaped=shaped, miss=miss, base=base_cost(s, ttl), task=s["task"],
                         calls=len(s["calls"])))
    return rows


def summ(rows, label):
    net = [r["net"] for r in rows]
    base = [r["base"] for r in rows]
    g = [r["task"] for r in rows]
    sh, mi = sum(r["shaped"] for r in rows), sum(r["miss"] for r in rows)
    return dict(label=label, n_sessions=len(rows), n_tasks=len(set(g)),
                usd_per_session=boot_cluster(net, groups=g), pct_of_cost=boot_cluster(net, base, g),
                gross_pct=boot_cluster([r["gross"] for r in rows], base, g),
                shaped_per_session=sh / len(rows), misses_per_session=mi / len(rows),
                miss_rate_per_shaped=(mi / sh) if sh else None,
                sessions_with_miss=float(np.mean([r["miss"] > 0 for r in rows])),
                baseline_usd=float(np.mean(base)))


def oracle_rows(sessions, actions, ttl):
    """Per-output hindsight choice within the action class, charged nothing (ceiling of the class)."""
    rows = []
    for s in sessions:
        v = sum(max([0.0] + [outcome(s, r, a, "H3", ttl)[0] for a in actions]) for r in s["results"])
        rows.append(dict(net=v, gross=v, shaped=0, miss=0, base=base_cost(s, ttl), task=s["task"], calls=len(s["calls"])))
    return rows


def unreferenced_rows(sessions, ttl):
    rows = []
    for s in sessions:
        tpc = CPT[fam(s["model"])]["tok_per_char"]
        v = 0.0
        for r in s["results"]:
            m = np.ones(len(r["lc"]), bool)
            m[r["used"]] = False
            v += r["lc"][m].sum() * tpc * carry(s, r["seen_call"], ttl)
        rows.append(dict(net=v, gross=v, shaped=0, miss=0, base=base_cost(s, ttl), task=s["task"], calls=len(s["calls"])))
    return rows


def main():
    fams = sys.argv[1:] or ["ALL", "POS", "REL", "GREP", "PreToolUse-only"]
    out = {}
    for model in ("haiku", "sonnet"):
        held, train = harness_sets(S, model)
        res = {}
        for fam_name in fams:
            acts = FAMILIES[fam_name]
            for harm in ("H1", "H2", "H3"):
                pol = learn(train, acts, harm)
                for ttl in ("1h", "5m"):
                    res[f"{fam_name}|{harm}|{ttl}|split"] = summ(apply(pol, held, harm, ttl), "split")
                if model == "haiku" and fam_name in ("ALL", "PreToolUse-only"):
                    rows = []
                    for repo in sorted({s["repo"] for s in held}):
                        pl = learn([s for s in train if s["repo"] != repo], acts, harm, loro=True)
                        rows += apply(pl, [s for s in held if s["repo"] == repo], harm, "1h", loro=True)
                    res[f"{fam_name}|{harm}|1h|loro"] = summ(rows, "leave-one-repo-out")
                if fam_name == "ALL" and harm == "H3":
                    json.dump({k: v for k, v in sorted(pol.items())}, open(f"policy_{model}_ALL_H3.json", "w"), indent=0)
            for ttl in ("1h", "5m"):
                res[f"{fam_name}|ceiling_class|{ttl}"] = summ(oracle_rows(held, acts, ttl), "hindsight best action per output")
        for ttl in ("1h", "5m"):
            res[f"ceiling_drop_all_unreferenced|{ttl}"] = summ(unreferenced_rows(held, ttl), "drop every unreferenced line, free")
        out[model] = res
        for k, v in res.items():
            print(model, k, "net %", tuple(round(100 * x, 2) for x in v["pct_of_cost"]),
                  "$", tuple(round(x, 4) for x in v["usd_per_session"]),
                  "gross %", round(100 * v["gross_pct"][0], 2), "shaped", round(v["shaped_per_session"], 2),
                  "miss/sess", round(v["misses_per_session"], 2), flush=True)
    tag = "_strict" if os.environ.get("USE_STRICT") else ""
    json.dump(out, open(("evaluate" if len(fams) > 1 else f"evaluate_{fams[0]}") + tag + ".json", "w"), indent=1)


if __name__ == "__main__":
    main()
