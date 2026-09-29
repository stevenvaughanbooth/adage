"""Content-aware output shaping: learn, per repository, which LINE TEMPLATES of tool output the
agent ever used, and elide lines of never-used templates (PreToolUse/PostToolUse rewrite; the
full output is parked in a file so a miss costs a re-fetch, not the task).

Line templates: Drain (He et al. 2017; drain3 0.9.11 core, vendored from its sdist) over the
training split's tool-output lines, with numbers masked (Drain3's default-style masking).
A template is elided when it has >= m training lines from >= 2 sessions and its referenced rate
is <= eps; (m, eps) chosen on the training split. Held-out accounting is the same as
policies.py: kept tokens + a 15-token note per elided run, and re-fetch penalties for
referenced lines that were elided.
"""
from __future__ import annotations

import collections
import json
import pickle
import re
import sys

import numpy as np

sys.path.insert(0, "/tmp/adage-research/token-cost/pkg/drain3-0.9.11")
from drain3.drain import Drain  # noqa: E402

from common import boot, boot_ratio, fam, session_cost  # noqa: E402
from policies import CPT, S, call_price, wilson  # noqa: E402

NUM = re.compile(r"(?<![A-Za-z_])[-+]?\d+(?:\.\d+)?%?")
NOTE = 15


def mask(line):
    line = re.sub(r"^\s*\d+\t", "", line)  # Read's line-number prefix
    return NUM.sub("<NUM>", line.strip())[:300]


def eligible(r, with_read):
    return with_read or r["key"] != "Read"


def lines_of(r):
    # line texts are not kept in the annotated pickle; re-derive from the raw pickle lazily
    return r["_lines"]


def attach_lines():
    raw = pickle.load(open("/tmp/adage-research/token-cost/sessions.pkl", "rb"))
    by = {s["sid"]: s for s in raw}
    for s in S:
        rs = by[s["sid"]]["results"]
        for r, rr in zip(s["results"], rs):
            r["_lines"] = rr["text"].split("\n")


def train_drain(train, with_read, use_key):
    d = Drain(sim_th=0.4, depth=4, max_children=100)
    stats = collections.defaultdict(lambda: [0, 0, set()])
    for s in train:
        for r in s["results"]:
            if not eligible(r, with_read) or r["carry"] == 0:
                continue
            used = set(r[use_key])
            for i, l in enumerate(r["_lines"][:2000]):
                ml = mask(l)
                if not ml:
                    continue
                c, _ = d.add_log_message(ml)
                st = stats[c.cluster_id]
                st[0] += 1
                st[1] += i in used
                st[2].add(s["sid"])
    return d, stats


def elided(d, stats, r, m, eps):
    out = np.zeros(len(r["_lines"]), bool)
    for i, l in enumerate(r["_lines"]):
        ml = mask(l)
        if not ml:
            continue
        c = d.match(ml, "never")
        if c is None:
            continue
        n, u, ss = stats[c.cluster_id]
        if n >= m and len(ss) >= 2 and u / n <= eps:
            out[i] = True
    return out


def outcome(s, r, drop, use_key, pen_mult=1.0):
    if not drop.any() or r["carry"] == 0:
        return 0.0, False, 0.0
    tpc = CPT[fam(s["model"])]["tok_per_char"]
    lc = np.array(r["line_chars"], float)
    runs = int((drop & ~np.r_[False, drop[:-1]]).sum())
    saved_tok = lc[drop].sum() * tpc - NOTE * runs
    gross = saved_tok * r["carry"]
    miss = [u for u in r[use_key] if drop[u]]
    pen = 0.0
    if miss:
        regs = []
        for u in miss:
            lo, hi = max(0, u - 10), min(len(lc) - 1, u + 10)
            if regs and lo - regs[-1][1] <= 30:
                regs[-1][1] = max(regs[-1][1], hi)
            else:
                regs.append([lo, hi])
        cp = call_price(s, r["seen_call"])
        by_region = (1 + len(regs)) * cp + sum(lc[a:b + 1].sum() for a, b in regs) * tpc * r["carry"]
        whole = 2 * cp + lc[drop].sum() * tpc * r["carry"]
        pen = pen_mult * min(by_region, whole)
    return gross - pen, bool(miss), gross


def run(train, test, with_read, use_key="used", eval_key="used", grid=((5, 0.0), (20, 0.0), (5, 0.01), (20, 0.02), (50, 0.02), (50, 0.05))):
    d, stats = train_drain(train, with_read, use_key)
    # choose (m, eps) on train (in-sample: Drain statistics are train counts)
    best, bestv = None, 0.0
    for m, eps in grid:
        v = sum(outcome(s, r, elided(d, stats, r, m, eps), use_key)[0]
                for s in train for r in s["results"] if eligible(r, with_read))
        if v > bestv:
            best, bestv = (m, eps), v
    saved, gross, miss, shaped, base = [], [], [], [], []
    for s in test:
        sv = g = 0.0
        mm = nn = 0
        if best:
            for r in s["results"]:
                if not eligible(r, with_read):
                    continue
                drop = elided(d, stats, r, *best)
                v, mi, gr = outcome(s, r, drop, eval_key)
                if drop.any():
                    nn += 1
                    mm += mi
                sv += v
                g += gr
        saved.append(sv); gross.append(g); miss.append(mm); shaped.append(nn); base.append(session_cost(s))
    return best, dict(saved=np.array(saved), gross=np.array(gross), misses=np.array(miss),
                      shaped=np.array(shaped), base=np.array(base))


def summ(best, ev):
    s, b = ev["saved"], ev["base"]
    return dict(params=best, n=len(s), usd_saved=boot(s), pct_saved=boot_ratio(s, b),
                gross_pct=boot_ratio(ev["gross"], b), shaped_per_session=float(ev["shaped"].mean()),
                miss_rate_per_shaped_output=wilson(int(ev["misses"].sum()), int(ev["shaped"].sum())))


def main():
    attach_lines()
    train = [s for s in S if s["split"] == "train"]
    held = [s for s in S if s["arm"] == "control" and s["split"] == "test"]
    out = {}
    for f in ("haiku", "sonnet"):
        H = [s for s in held if fam(s["model"]) == f]
        res = {}
        for with_read in (False,):  # Read (source code) lines: Drain templates are not meaningful and it is too slow
            tag = "all outputs" if with_read else "non-Read outputs"
            parts = []
            for repo in sorted({s["repo"] for s in H}):
                best, ev = run([s for s in train if s["repo"] == repo], [s for s in H if s["repo"] == repo], with_read)
                parts.append((best, ev))
            ev = {k: np.concatenate([p[1][k] for p in parts]) for k in parts[0][1]}
            res[f"per-repo Drain, {tag}"] = summ([p[0] for p in parts], ev)
            # strict usage measure (verbatim re-use only): optimistic about what is unused
            parts = []
            for repo in sorted({s["repo"] for s in H}):
                parts.append(run([s for s in train if s["repo"] == repo], [s for s in H if s["repo"] == repo],
                                 with_read, "used_strict", "used_strict"))
            ev = {k: np.concatenate([p[1][k] for p in parts]) for k in parts[0][1]}
            res[f"per-repo Drain, {tag}, strict-use measure (optimistic)"] = summ([p[0] for p in parts], ev)
            # leave-one-repo-out
            parts = []
            for repo in sorted({s["repo"] for s in H}):
                parts.append(run([s for s in train if s["repo"] != repo], [s for s in H if s["repo"] == repo], with_read))
            ev = {k: np.concatenate([p[1][k] for p in parts]) for k in parts[0][1]}
            res[f"LORO Drain, {tag}"] = summ([p[0] for p in parts], ev)
        out[f] = res
    json.dump(out, open("drainfilter.json", "w"), indent=1, default=str)
    for f, res in out.items():
        print("=====", f)
        for k, v in res.items():
            print(k, json.dumps(v, default=lambda x: round(x, 4) if isinstance(x, float) else str(x)))


if __name__ == "__main__":
    main()
