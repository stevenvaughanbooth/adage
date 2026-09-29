"""Tables + result.json from result_raw.json.   python3 report.py > tables.md"""

from __future__ import annotations

import json
import pickle
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
R = json.load(open(HERE / "result_raw.json"))
S = pickle.load(open("/tmp/adage-research/case-retrieval-scale/sessions.pkl", "rb"))
SID = {s.sid: s for s in S}
FAMILY = ("attrs", "click", "vue-core")  # bases where every memory size up to 100 exists
TG = ["skip", "next", "avoid", "skip+avoid", "summary", "prior", "labprior"]
NAMES = {"skip": "kNN skip-ahead (achieving action)", "next": "kNN next action", "avoid": "kNN avoid-what-failed",
         "skip+avoid": "kNN skip + avoid", "summary": "whole-session summary (3 lines)",
         "prior": "prior: learned landmark, no similarity", "labprior": "prior: lab's most-often-useful command"}


def boot(x, n=2000, seed=7):
    x = np.asarray(x, float)
    if len(x) == 0:
        return (float("nan"),) * 3
    b = np.random.default_rng(seed).integers(0, len(x), (n, len(x)))
    m = x[b].mean(1)
    return float(x.mean()), float(np.percentile(m, 5)), float(np.percentile(m, 95))


def f3(t, sign=True):
    if t[0] is None or (isinstance(t[0], float) and np.isnan(t[0])):
        return "–"
    fmt = "{:+.2f}" if sign else "{:.2f}"
    return f"{fmt.format(t[0])} [{fmt.format(t[1])}, {fmt.format(t[2])}]"


def prec(a):
    p = a.get("precision") or [None]
    if p[0] is None:
        return "– (0)"
    return f"{p[0]:.2f} [{p[1]:.2f}, {p[2]:.2f}] ({a['issued_total']:.0f})"


def row(name, tg, a):
    return (f"| {name} | {NAMES[tg]} | {a['issued'][0]:.2f} | {prec(a)} | {a['useful'][0]:.2f} | "
            f"{a['calls_saved'][0]:.2f} | {f3(a['net_calls'])} | **{f3(a['net_harm'])}** | "
            f"{a['pct_usd_harm'][0] * 100:+.1f}% | {a['harmful_rate'] or 0:.2f} |")


def subset(a, bases, filt=None):
    xs = [v for sid, v in a["per_session"].items() if SID[sid].base in bases and (filt is None or filt(SID[sid]))]
    return boot(xs), len(xs)


def main():
    C = R["configs"]
    out = []
    P = out.append
    hdr = ("| memory | policy | directives/session | precision [Wilson 90] (issued) | useful/session | failed cmds "
           "saved/session | net calls (no harm) [90% CI] | **net calls, harm-adjusted** [90% CI] | net $ harm-adj. | "
           "wrong+harmful rate |\n|---|---|---:|---|---:|---:|---|---|---:|---:|")
    P("### A. Learning curve, same-repository memory (all 208 held-out control sessions where the size exists)\n")
    P(hdr)
    for N in ("5", "10", "20", "50", "100", "all"):
        c = C.get(f"same-{N}")
        if not c:
            continue
        for tg in TG:
            if tg in c:
                P(row(f"{N} ({c[tg]['sessions']} test s.)", tg, c[tg]))
    P("\n### B. Same curve on the attrs + click + vue-core family only (every size exists; harm-adjusted net calls/session [90% CI])\n")
    P("| policy | " + " | ".join(f"N={N}" for N in ("5", "10", "20", "50", "100", "all")) + " |")
    P("|---|" + "---|" * 6)
    curve = {}
    for tg in TG:
        cells = []
        for N in ("5", "10", "20", "50", "100", "all"):
            c = C[f"same-{N}"][tg]
            m, n = subset(c, FAMILY)
            curve.setdefault(tg, []).append((N, m))
            cells.append(f"{f3(m)} (n={n})")
        P(f"| {NAMES[tg]} | " + " | ".join(cells) + " |")
    # extrapolation: net = a + b log2(N) on the family
    P("\n### C. Extrapolation (least squares, harm-adjusted net calls vs log2 N, family subset; `all` ≈ its mean memory size)\n")
    P("| policy | slope per doubling | predicted at N=500 | N where it would reach U11 post-hoc (+1.00) |\n|---|---:|---:|---:|")
    Nall = C["same-all"]["memory"]["sessions_mean"]
    extra = {}
    for tg in TG:
        xs = np.log2([float(n) if n != "all" else 130.0 for n, _ in curve[tg]])
        ys = np.array([m[0] for _, m in curve[tg]])
        b, a = np.polyfit(xs, ys, 1)
        at500 = a + b * np.log2(500)
        cross = 2 ** ((1.0 - a) / b) if b > 1e-6 else float("inf")
        extra[tg] = {"slope_per_doubling": float(b), "at_500": float(at500), "n_to_reach_u11": float(cross)}
        P(f"| {NAMES[tg]} | {b:+.3f} | {at500:+.2f} | {'never (slope ≤ 0)' if cross == float('inf') else ('> 10^6 (never in practice)' if cross > 1e6 else f'{cross:,.0f}')} |")
    P("\n### D. Memory from other repositories\n")
    P(hdr)
    for name in ("other-all", "same-5+other-all", "same-20+other-all", "same-all+other-all"):
        c = C[name]
        for tg in TG:
            if tg in c:
                P(row(f"{name} (~{c['memory']['sessions_mean']:.0f} s.)", tg, c[tg]))
    P("\n### E. Ablations at same-repository `all` memory\n")
    P(hdr)
    for name, c in C.items():
        if not name.startswith("same-all/"):
            continue
        for tg in ("skip", "avoid", "skip+avoid", "summary", "next"):
            P(row(name.split("/")[1], tg, c[tg]))
    P("\n### F. U11-comparable subset (v2 Haiku control sessions of click / attrs / marshmallow / vue-core), harm-adjusted net calls/session\n")
    P("| memory | " + " | ".join(NAMES[t] for t in TG) + " |\n|---|" + "---|" * len(TG))
    v2 = lambda s: s.version == "v2" and "haiku" in s.model  # noqa: E731
    for N in ("5", "20", "all"):
        c = C[f"same-{N}"]
        P(f"| same-{N} | " + " | ".join(f3(subset(c[t], ("click", "attrs", "marshmallow", "vue-core"), v2)[0])
                                        for t in TG) + " |")
    c = C["same-all+other-all"]
    P("| same-all+other-all | " + " | ".join(f3(subset(c[t], ("click", "attrs", "marshmallow", "vue-core"), v2)[0])
                                            for t in TG) + " |")
    P("\nReference (Rust replay Ŝ, 82 v2 Haiku control sessions): U11 + learned reflexes **+1.00** [+0.56, +1.46] post-hoc; "
      "U11 pre-emptive **+2.11** [+1.47, +2.70].")
    o = R["oracle"]
    P(f"\n### G. Ceiling (hindsight oracle of this lever on the 208 test sessions)\n\n"
      f"- failed commands any run/avoid directive could remove: **{o['calls_per_session']:.2f} calls/session = "
      f"{o['pct_calls'] * 100:.1f}% of calls, {o['pct_usd'] * 100:.1f}% of $** (sessions average "
      f"{o['calls_per_session_total']:.1f} calls; {o['failed_per_session']:.2f} failed calls of any kind).\n"
      f"- loose upper bound (every failed or exploratory non-edit step before the last new working command; most are "
      f"reads of code the agent needed): {o['loose_calls_per_session']:.1f} calls/session = {o['loose_pct_calls'] * 100:.0f}% of calls, "
      f"{o['loose_pct_usd'] * 100:.0f}% of $.")
    L = R["latency"]
    P("\n### H. Latency and memory size (Python; brute-force kNN, float32; bytes if stored as i16 potion-8M)\n")
    P("| memory | sessions | situations | stored size | query per decision point | build |\n|---|---:|---:|---:|---:|---:|")
    for k in ("same-all", "same-all+other-all"):
        x = L[k]
        P(f"| {k} | {x['sessions']} | {x['situations']} | {x['bytes_i16'] / 1e6:.1f} MB | "
          f"{x['query_ms_per_decision_point_py']:.2f} ms | {x['build_s_py']:.2f} s |")
    P(f"\npotion-8M embed of one text: {L['embed_ms_median']:.2f} ms median (3 texts per decision point).")
    print("\n".join(out))
    return extra


if __name__ == "__main__":
    ex = main()
    json.dump(ex, open(HERE / "extrapolation.json", "w"), indent=1)
