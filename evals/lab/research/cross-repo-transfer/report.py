"""Tables from results/loro-*.json -> results/tables.md and result.json fields (printed)."""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import boot  # noqa: E402

REAL = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
SYN = ["gen-py-clinic", "gen-node-logistics"]


def load(tag=""):
    out = {}
    for f in (HERE / "results").glob(f"loro-*{tag}.json"):
        name = f.stem.removeprefix("loro-")
        if tag == "" and name.endswith("-strict"):
            continue
        out[name.removesuffix(tag)] = json.loads(f.read_text())
    return out


def fmt(t, d=2):
    return f"{t[0]:+.{d}f} [{t[1]:+.{d}f}, {t[2]:+.{d}f}]"


def pooled(R, repos, n, key):
    net, pct, usd, groups = [], [], [], []
    for X in repos:
        r = R.get(X, {}).get("results", {}).get(str(n))
        if not r or key not in r["per_session"]:
            continue
        for s in r["per_session"][key]:
            net.append(s["net"]); pct.append(s["net"] / max(1, s["calls"])); usd.append(s["usd"])
            groups.append(s["task"])
    return boot(net, groups), boot(pct, groups), boot(usd, groups), len(net)


def main():
    lines = []
    for tag, title in (("", "window = Bash + failed calls"), ("-strict", "strict window = failed / dead-end calls only")):
        R = load(tag)
        if not R:
            continue
        lines.append(f"\n## {title}\n")
        lines.append("Calls saved per held-out control session, net of misfires and carry cost, 90% task-clustered bootstrap.\n")
        pols = ["hier/pre", "hier/post", "pooled/pre", "own/pre", "own/post", "u11/pre", "u11/post", "oracle/pre"]
        lines.append("| repo | n own | " + " | ".join(pols) + " |")
        lines.append("|---|---:|" + "---|" * len(pols))
        for X in REAL + SYN:
            if X not in R:
                continue
            for n, r in R[X]["results"].items():
                cells = [fmt(r["summary"][p]["net_calls"]) for p in pols]
                lines.append(f"| {X} | {r['own_sessions']} | " + " | ".join(cells) + " |")
        for grp, repos in (("pooled real (5)", REAL), ("pooled python (3)", REAL[:3])):
            for n in ("0", "1", "3", "10"):
                cells = []
                for p in pols:
                    a, b, c, k = pooled(R, repos, n, p)
                    cells.append(fmt(a))
                lines.append(f"| **{grp}** | {n} | " + " | ".join(cells) + " |")
        lines.append("\nShare of session calls and $ per session (pooled real repos):\n")
        lines.append("| n own | policy | calls/session | % of calls | $/session |")
        lines.append("|---:|---|---|---|---|")
        for n in ("0", "1", "3", "10"):
            for p in ("hier/pre", "hier/post", "own/pre", "u11/pre", "u11/post", "oracle/pre"):
                a, b, c, k = pooled(R, REAL, n, p)
                lines.append(f"| {n} | {p} | {fmt(a)} | {fmt(tuple(100 * x for x in b), 1)}% | {fmt(c, 4)} |")
        lines.append("\nDirectives, usefulness (Wilson 90%), misfires, unverifiable, affected sessions (day 0, hier/pre):\n")
        lines.append("| repo | sessions | directives | useful [90%] | misfires | unverified | affected | mean calls | mean $ |")
        lines.append("|---|---:|---:|---|---:|---:|---:|---:|---:|")
        for X in REAL + SYN:
            if X not in R:
                continue
            s = R[X]["results"]["0"]["summary"]["hier/pre"]
            u = s["useful"]
            lines.append(f"| {X} | {s['sessions']} | {s['directives']} | {u[0]:.2f} [{u[1]:.2f}, {u[2]:.2f}] | "
                         f"{s['misfires']} | {s['unverified']} | {s['affected']} | {s['mean_calls']:.1f} | {s['mean_cost']:.3f} |")
        if tag == "":
            for n in ("0", "10"):
                lines.append(f"\n## Replacements the hierarchical learner chose at n = {n} own sessions\n")
                lines.append("| held-out repo | dead-end head (h0) | learned replacement (c) | P(productive) | trials | c in X: productive/runs | h0 in X |")
                lines.append("|---|---|---|---:|---:|---|---|")
                for X in REAL + SYN:
                    for d in ((R.get(X, {}).get("discoveries") or {}).get(n) or []):
                        if d["policy"] != "hier":
                            continue
                        lines.append(f"| {X} | `{d['from_head']}` | `{d['to_head']}` | {d['p']:.2f} | {d['trials']} | "
                                     f"{d['verified_in_X'][0]}/{d['verified_in_X'][1]} | {d['from_in_X'][0]}/{d['from_in_X'][1]} |")
    (HERE / "results" / "tables.md").write_text("\n".join(lines) + "\n")
    print("\n".join(lines))


if __name__ == "__main__":
    main()
