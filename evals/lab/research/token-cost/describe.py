"""Where do the dollars go? Descriptive cost anatomy on held-out control sessions.

Buckets below are a hand-written MEASUREMENT instrument (not used by any policy).
"""
from __future__ import annotations

import collections
import json
import pickle
import re

import numpy as np

from common import PRICES, boot, boot_ratio, call_cost, fam, session_cost

D = pickle.load(open("/tmp/adage-research/token-cost/annotated.pkl", "rb"))
S, CPT = D["sessions"], D["cpt"]
TEST_RE = re.compile(r"pytest|test|vitest|jest")


def bucket(r):
    k = r["key"]
    if k == "Read":
        inp = r["input"] or {}
        return "Read (ranged)" if inp.get("offset") or inp.get("limit") else "Read (whole file)"
    if k.startswith("Bash:") and TEST_RE.search(k + " " + r["template"]):
        return "Bash test run"
    if k in ("Grep", "Glob", "Bash:grep", "Bash:rg", "Bash:find", "Bash:ls", "Bash:git grep"):
        return "search (grep/find/ls)"
    if k in ("Bash:cat", "Bash:sed", "Bash:head", "Bash:tail"):
        return "Bash file view"
    if k in ("Edit", "Write", "MultiEdit"):
        return "Edit/Write"
    return "other Bash/tools"


def read_span(r):
    inp = r["input"] or {}
    o = int(inp.get("offset") or 1)
    lim = inp.get("limit")
    n = r["n_lines"]
    return o, o + (int(lim) if lim else n) - 1


def rereads(s):
    """Tokens of Read output that re-show lines already shown by an earlier Read of the same
    unchanged file (no Edit/Write to that file in between). Returns list of (result, frac_dup)."""
    seen = collections.defaultdict(list)  # path -> list of (lo, hi)
    out = []
    for r in s["results"]:
        path = r["full"].split(" ", 1)[-1]
        if r["key"] in ("Edit", "Write", "MultiEdit") and not r["is_error"]:
            seen.pop(path, None)
            continue
        if r["key"] != "Read" or r["is_error"] or r["n_lines"] < 3:
            continue
        lo, hi = read_span(r)
        cover = np.zeros(hi - lo + 1, bool)
        for a, b in seen[path]:
            x, y = max(a, lo), min(b, hi)
            if x <= y:
                cover[x - lo:y - lo + 1] = True
        frac = cover.mean() if len(cover) else 0
        if frac > 0:
            out.append((r, float(frac)))
        seen[path].append((lo, hi))
    return out


def main():
    from common import harness_sets
    rep = {}
    for f in ("haiku", "sonnet"):
        H, _ = harness_sets(S, f)
        tot = np.array([session_cost(s) for s in H])
        res_cost = np.array([sum(r["tok"] * r["carry"] for r in s["results"]) for s in H])
        by_b = collections.defaultdict(lambda: np.zeros(len(H)))
        unused_b = collections.defaultdict(lambda: np.zeros(len(H)))
        big = np.zeros(len(H))
        early = np.zeros(len(H))
        rr = np.zeros(len(H))
        n_rr = np.zeros(len(H))
        for i, s in enumerate(H):
            n = len(s["calls"])
            for r in s["results"]:
                c = r["tok"] * r["carry"]
                by_b[bucket(r)][i] += c
                lc = np.array(r["line_chars"], float)
                used = np.zeros(len(lc), bool)
                used[r["used"]] = True
                unused_b[bucket(r)][i] += c * (lc[~used].sum() / max(lc.sum(), 1))
                if r["tok"] >= 2000:
                    big[i] += c
                if r["seen_call"] < n / 3:
                    early[i] += c
            for r, frac in rereads(s):
                rr[i] += r["tok"] * r["carry"] * frac
                n_rr[i] += 1
        # prefix (system prompt + tools) cost: first call's write, re-read every call
        pre = []
        maxctx = []
        for s in H:
            p = PRICES[f]
            c0 = s["calls"][0]
            tokens0 = c0["cw"] + c0["cr"] + c0["inp"]
            pre.append(tokens0 * (p["cw1h"] * c0["cw"] / max(tokens0, 1) + p["cr"] * c0["cr"] / max(tokens0, 1)) / 1e6
                       + tokens0 * p["cr"] * (len(s["calls"]) - 1) / 1e6)
            maxctx.append(max(c["cw"] + c["cr"] + c["inp"] for c in s["calls"]))
        outc = np.array([sum(c["out"] * PRICES[f]["out"] for c in s["calls"]) / 1e6 for s in H])
        # marginal $ of one tool call: its own request (reads the context, writes output) plus the
        # carry of its output+result; averaged over calls
        marg = []
        for s in H:
            p = PRICES[f]
            n = len(s["calls"])
            per = []
            for k, c in enumerate(s["calls"]):
                own = call_cost(c, p) - c["cw"] * p["cw1h"] / 1e6  # the new-content write belongs to the previous call
                res_tok = sum(r["tok"] for r in s["results"] if r["use_call"] == k)
                carry = (c["out"] + res_tok) * ((p["cw1h"] + p["cr"] * max(n - 2 - k, 0)) / 1e6 if k < n - 1 else 0)
                per.append(own + carry)
            marg.append(np.mean(per))
        marg = np.array(marg)
        r = dict(
            n_sessions=len(H),
            mean_cost=boot(tot),
            mean_calls=float(np.mean([len(s["calls"]) for s in H])),
            max_context_tokens=int(max(maxctx)), median_max_context=float(np.median(maxctx)),
            compactions=0,
            share_tool_results=boot_ratio(res_cost, tot),
            share_system_prefix=boot_ratio(np.array(pre), tot),
            share_output_tokens=boot_ratio(outc, tot),
            share_by_bucket={b: boot_ratio(v, tot) for b, v in sorted(by_b.items(), key=lambda x: -x[1].sum())},
            share_unreferenced_by_bucket={b: boot_ratio(v, tot) for b, v in sorted(unused_b.items(), key=lambda x: -x[1].sum())},
            share_unreferenced_all=boot_ratio(sum(unused_b.values()), tot),
            share_big_outputs_ge2k_tokens=boot_ratio(big, tot),
            share_first_third_results=boot_ratio(early, tot),
            reread_dollars_per_session=boot(rr), reread_share=boot_ratio(rr, tot),
            rereads_per_session=float(n_rr.mean()),
            marginal_dollars_per_call=boot(marg),
        )
        rep[f] = r
    json.dump(rep, open("describe.json", "w"), indent=1)
    for f, r in rep.items():
        print("==", f)
        for k, v in r.items():
            if isinstance(v, dict):
                print(k)
                for kk, vv in v.items():
                    print("   ", kk, tuple(round(x, 4) for x in vv))
            else:
                print(k, tuple(round(x, 4) for x in v) if isinstance(v, tuple) else v)


if __name__ == "__main__":
    main()
