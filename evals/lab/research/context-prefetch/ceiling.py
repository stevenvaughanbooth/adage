"""Oracle ceiling of file prefetch on held-out control sessions (all test tasks; Haiku and Sonnet).

Rows per repo/model; columns:
- calls, $: session means.
- oracle path hint, tier "search": the location calls (no file opened) attributed to a target.
- oracle path hint, tier "all": plus the reads of non-target files on the way to a target.
- oracle read batch: the targets' full contents injected at the prompt (carry cost included).
- pre-gold: every request before the first touch of a gold source file (the literature agent's
  localisation ceiling, reproduced for comparison).
"""
import collections
import json
from pathlib import Path

import numpy as np

import data
import targets as TG


def boot_ratio(num, den, B=2000, seed=0):
    num, den = np.asarray(num, float), np.asarray(den, float)
    idx = np.random.default_rng(seed).integers(0, len(num), (B, len(num)))
    r = num[idx].sum(1) / den[idx].sum(1)
    return [round(float(num.sum() / den.sum()), 4), round(float(np.percentile(r, 5)), 4), round(float(np.percentile(r, 95)), 4)]


def main():
    S = data.load()
    info = TG.annotate(S)
    rows = collections.defaultdict(list)
    for s in S:
        if s.phase != "eval" or s.arm != "control" or s.sid not in info:
            continue
        d = info[s.sid]
        n = len(s.calls)
        cs, ds = TG.savings(s, TG.removable(s, d, d["T"], tier="search"), inject_tokens=40 + 15 * len(d["T"]))
        ca, da = TG.savings(s, TG.removable(s, d, d["T"], tier="all"), inject_tokens=40 + 15 * len(d["T"]))
        ftok = sum(len(TG.R.text(s.repo, d["tree"][f])) for f in d["T"]) / 4
        cb, db = TG.savings(s, TG.removable(s, d, d["T"], batch=True), inject_tokens=ftok)
        g = [i for i, a in enumerate(d["access"]) if a & d["G"]]
        pre_gold = set(range(g[0])) if g else set()
        cg, dg = TG.savings(s, pre_gold)
        fe = min([i for i, c in enumerate(s.calls) if c.tool in TG.EDIT] or [n])
        rows[f"{s.repo}/{s.model}"].append(dict(
            n=n, D=s.dollars, E=len(d["E"]), N=len(d["N"]), new=len(d["new"]), fe=fe,
            n_attr=len(d["attr"]), cs=cs, ds=ds, ca=ca, da=da, cb=cb, db=db, cg=cg, dg=dg,
            passed=bool(s.passed), editedG=bool(d["G"] & d["E"]) if d["G"] else None, model=s.model))
    out = {}
    groups = dict(rows)
    groups["ALL haiku"] = [r for k, v in rows.items() if k.endswith("haiku") for r in v]
    groups["ALL sonnet"] = [r for k, v in rows.items() if k.endswith("sonnet") for r in v]
    groups["ALL"] = [r for v in rows.values() for r in v]
    for k, rs in groups.items():
        m = lambda f: round(float(np.mean([r[f] for r in rs])), 2)
        D = [r["D"] for r in rs]
        N = [r["n"] for r in rs]
        out[k] = dict(
            sessions=len(rs), calls=m("n"), usd=round(float(np.mean(D)), 4), E=m("E"), N=m("N"), new_files=m("new"),
            first_edit_call=m("fe"),
            hint_search=dict(calls=m("cs"), calls_pct=boot_ratio([r["cs"] for r in rs], N), usd_pct=boot_ratio([r["ds"] for r in rs], D)),
            hint_all=dict(calls=m("ca"), calls_pct=boot_ratio([r["ca"] for r in rs], N), usd_pct=boot_ratio([r["da"] for r in rs], D)),
            read_batch=dict(calls=m("cb"), usd_pct=boot_ratio([r["db"] for r in rs], D)),
            pre_gold=dict(calls=m("cg"), usd_pct=boot_ratio([r["dg"] for r in rs], D)),
        )
        o = out[k]
        print(f"{k:18s} n={len(rs):3d} calls {o['calls']:5.1f} ${o['usd']:.3f} |E| {o['E']} |N| {o['N']} "
              f"| hint/search {o['hint_search']['calls']:4.1f} calls {100*o['hint_search']['usd_pct'][0]:5.1f}% "
              f"| hint/all {o['hint_all']['calls']:4.1f} calls {100*o['hint_all']['usd_pct'][0]:5.1f}% {o['hint_all']['usd_pct'][1:]} "
              f"| batch {100*o['read_batch']['usd_pct'][0]:6.1f}% | pre-gold {o['pre_gold']['calls']:4.1f} calls {100*o['pre_gold']['usd_pct'][0]:4.1f}%")
    allr = groups["ALL"]
    g = [r for r in allr if r["editedG"] is not None]
    out["pass_by_gold_edit"] = {str(e): dict(n=len([r for r in g if r["editedG"] == e]),
                                             pass_rate=round(float(np.mean([r["passed"] for r in g if r["editedG"] == e])), 3))
                                for e in (True, False)}
    print("pass rate by 'edited a gold source file':", out["pass_by_gold_edit"])
    (Path(__file__).parent / "ceiling.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
