"""Print the README tables from results/*.json (markdown)."""

from __future__ import annotations

import json
import sys
from pathlib import Path

R = Path(__file__).resolve().parent / "results"
PRE_THETA = 0.5  # pre-declared operating point (calibrated p >= 0.5); not tuned on test


def f(x, d=2):
    return "–" if x is None else (f"{x:.{d}f}" if isinstance(x, float) else str(x))


def ci(c, d=2):
    return "–" if not c or c[0] is None else f"[{c[0]:.{d}f}, {c[1]:.{d}f}]"


def at(curve, th):
    c = [x for x in curve if abs(x["theta"] - th) < 1e-9]
    return c[0] if c else None


def best(curve):
    return max(curve, key=lambda c: c["net_inst"]) if curve else None


def row(name, c, tag):
    if c is None:
        return f"| {name} | {tag} | 0 | – | – | – | – | – | – | – |"
    return (f"| {name} | {tag} θ={c['theta']} | {c['deliveries_per_session']:.2f} | {f(c['precision'])} "
            f"{ci(c.get('precision_ci'))} | {f(c['taken_rate'])} | {c['net_fsim']:.2f} | {c['net_relfail']:.2f} | "
            f"**{c['net_inst']:.2f}** {ci(c.get('net_inst_ci'))} | {c['net_inst_usd_pct']:.1f}% {ci(c.get('net_inst_usd_pct_ci'), 1)} | "
            f"{c['net_explore']:.2f} | {c['net_dist']:.1f} |")


HDR = ("| model | operating point | deliveries/session | precision (Wilson 90%) | taken later | net fsim | net relfail | "
       "**net calls (inst)** [90% CI] | net $ (inst) [90% CI] | net explore (loose) | skip dist × p_follow |\n"
       "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")


def main():
    sp = json.loads((R / "split-K128.json").read_text())
    print("## split\n", json.dumps({k: sp[k] for k in ("K", "tightness", "train", "test", "next_cluster")}))
    print("ceiling", json.dumps(sp["ceiling"]))
    print(HDR)
    for m, v in sp["models"].items():
        print(row(m, at(v["curve"], PRE_THETA), "pre"))
        print(row(m, best(v["curve"]), "best-on-test"))
    if (R / "loro-K128.json").exists():
        lo = json.loads((R / "loro-K128.json").read_text())
        print("\n## loro")
        for repo, v in lo.items():
            print(f"\n### {repo} train={v['train']} test={v['test']} ceiling inst={v['ceiling']['unlimited']['inst']}")
            print(HDR)
            for m, mv in v["models"].items():
                print(row(m, at(mv["curve"], PRE_THETA), "pre"))
                print(row(m, best(mv["curve"]), "best-on-test"))
    if (R / "curve-K128.json").exists():
        cu = json.loads((R / "curve-K128.json").read_text())
        print("\n## curve")
        print(HDR)
        for n, v in cu.items():
            for m, curve in v["models"].items():
                print(row(f"n={v['train_sessions']} {m}", at(curve, PRE_THETA), "pre"))
                print(row(f"n={v['train_sessions']} {m}", best(curve), "best-on-test"))


if __name__ == "__main__":
    main()
