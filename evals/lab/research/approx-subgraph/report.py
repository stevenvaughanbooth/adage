"""Tables from results/*.json (task-clustered 90% bootstrap; Wilson 90% for rates)."""

from __future__ import annotations

import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

from stats import boot, ratio_boot, wilson

HERE = Path(__file__).resolve().parent
RES = HERE / "results"
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]


def fmt(t, d=2):
    return f"{t[0]:.{d}f} [{t[1]:.{d}f}, {t[2]:.{d}f}]"


def agg(rows_by_repo: list[dict], tier="all"):
    """Pool several (repo) results of one alphabet."""
    H = [h for r in rows_by_repo for h in r["heldout"]]
    tasks = [h["task"] for h in H]
    saved = [h["bits0"] - h["bits"] for h in H]
    comp = ratio_boot(saved, [h["bits0"] for h in H], tasks)
    cov = sum(h["used"] > 0 for h in H)
    U = [u for r in rows_by_repo for u in r["use_ct"]]
    fol, pre = sum(u["n_follow"] for u in U), sum(u["n_prefix"] for u in U)
    lift = (sum(u["n_follow"] for u in U) / pre) / (sum(u["base"] * u["n_prefix"] for u in U) / pre) if pre else float("nan")
    betw = [b for u in U for b in u["between"]]
    out = {"n": len(H), "bits_saved": boot(saved, tasks), "compression": comp,
           "coverage": wilson(cov, len(H)), "table": np.mean([np.mean(r["table"]) for r in rows_by_repo]),
           "follow": wilson(fol, pre), "lift": lift, "between_median": statistics.median(betw) if betw else float("nan"),
           "between_mean": float(np.mean(betw)) if betw else float("nan")}
    for key in ("replay_ct", "replay_freq", "replay_null"):
        R = [x for r in rows_by_repo for x in r[key]]
        t = [x["task"] for x in R]
        iss = sum(x["issued"] for x in R)
        use = sum(x["useful"] for x in R)
        o = {"directives_per_session": iss / max(1, len(R)), "useful": wilson(use, iss)}
        for tier in ("fail", "fail+disc", "all"):
            o[tier] = boot([x["net"][tier] for x in R], t)
            o[tier + "_pct"] = ratio_boot([x["net"][tier] for x in R], [x["calls"] for x in R], t)
            dpc = [x["dpc"] for x in R if x["dpc"]]
            md = float(np.mean(dpc)) if dpc else 0.0
            o[tier + "_usd"] = boot([x["net"][tier] * (x["dpc"] or md) for x in R], t, stat=np.mean)
        out[key] = o
    C = [c for r in rows_by_repo for c in r["compile"]]
    out["compilable_run"] = (sum(all(x[0] == "run" for x in c) for c in C), len(C))
    return out


def table(setting, alphas=None):
    d = json.loads((RES / f"{setting}.json").read_text())
    by = defaultdict(dict)
    for k, v in d.items():
        repo, model, a = k.split("|")
        by[(model, a)][repo] = v
    lines = []
    lines.append(f"\n### {setting}\n")
    lines.append("| alphabet | model | sessions | labels | table | held-out bits saved/session | compression % | coverage | P(suffix follows prefix within 8) | lift | calls between (median) |")
    lines.append("|---|---|---:|---:|---:|---|---|---|---|---:|---:|")
    order = alphas or ["literal", "full", "key", "emb@0.9", "emb@0.8", "emb@0.7", "emb@0.6"]
    summary = {}
    for model in ("haiku", "sonnet"):
        for a in order:
            rows = [by[(model, a)][r] for r in REPOS if r in by.get((model, a), {})]
            if not rows:
                continue
            g = agg(rows)
            summary[(model, a)] = g
            labels = np.mean([np.mean(r["labels"]) for r in rows])
            lines.append(f"| {a} | {model} | {g['n']} | {labels:.0f} | {g['table']:.1f} | {fmt(g['bits_saved'], 1)} | "
                         f"{fmt(tuple(100 * x for x in g['compression']), 1)} | {fmt(g['coverage'])} | {fmt(g['follow'])} | "
                         f"{g['lift']:.2f} | {g['between_median']:.0f} |")
    lines.append("\nDelivery replay (MDL table vs the 20 most frequent closed patterns), net calls saved per session (p_follow 0.8, 0.1/directive, 0.3/misfire):\n")
    lines.append("| alphabet | model | selection | directives/session | useful | net calls: failed only | net: failed + discovery | net: all calls in window (skip-ahead bound) | % of calls (all) | $/session (fail+disc) | compiles to one command |")
    lines.append("|---|---|---|---:|---|---|---|---|---|---|---|")
    for (model, a), g in summary.items():
        for key, name in (("replay_ct", "MDL"), ("replay_null", "MDL suffixes, prefix ignored (control)"), ("replay_freq", "frequent top-20")):
            o = g[key]
            comp = f"{g['compilable_run'][0]}/{g['compilable_run'][1]}" if key == "replay_ct" else ""
            lines.append(f"| {a} | {model} | {name} | {o['directives_per_session']:.1f} | {fmt(o['useful'])} | {fmt(o['fail'])} | "
                         f"{fmt(o['fail+disc'])} | {fmt(o['all'])} | {fmt(tuple(100 * x for x in o['all_pct']), 1)} | "
                         f"{fmt(o['fail+disc_usd'], 3)} | {comp} |")
    return "\n".join(lines), summary


def per_repo(setting, a, model="haiku"):
    d = json.loads((RES / f"{setting}.json").read_text())
    lines = [f"\n{setting} / {a}: per repository\n",
             "| repo | sessions | bits saved/session | compression % | coverage | net calls (fail+disc) | net calls (all) |",
             "|---|---:|---|---|---|---|---|"]
    for r in REPOS:
        k = f"{r}|{model}|{a}"
        if k not in d:
            continue
        g = agg([d[k]])
        lines.append(f"| {r} | {g['n']} | {fmt(g['bits_saved'], 1)} | {fmt(tuple(100 * x for x in g['compression']), 1)} | "
                     f"{fmt(g['coverage'])} | {fmt(g['replay_ct']['fail+disc'])} | {fmt(g['replay_ct']['all'])} |")
    return "\n".join(lines)


if __name__ == "__main__":
    for s in sys.argv[1:] or ["chrono", "grown", "loro"]:
        if (RES / f"{s}.json").exists():
            print(table(s)[0])


def pm_table():
    d = json.loads((RES / "pm.json").read_text())
    lines = ["\n### process mining (grown folds)\n",
             "| alphabet | HM edges/fold | P(b follows a within 8) | lift | net calls (fail+disc) | net calls (all) | IM flower mass (fold 0, mean over repos) |",
             "|---|---:|---|---:|---|---|---:|"]
    for a in ("key", "emb@0.8", "emb@0.7"):
        rows = [d[f"{r}|{a}"] for r in REPOS if f"{r}|{a}" in d]
        U = [u for r in rows for u in r["use"]]
        fol, pre = sum(u["n_follow"] for u in U), sum(u["n_prefix"] for u in U)
        lift = (fol / pre) / (sum(u["base"] * u["n_prefix"] for u in U) / pre)
        R = [x for r in rows for x in r["replay"]]
        t = [x["task"] for x in R]
        fm = np.mean([r["trees"][0]["stats"]["flower_mass"] for r in rows])
        e = np.mean([np.mean(r["edges"]) for r in rows])
        lines.append(f"| {a} | {e:.1f} | {fmt(wilson(fol, pre))} | {lift:.2f} | {fmt(boot([x['net']['fail+disc'] for x in R], t))} | "
                     f"{fmt(boot([x['net']['all'] for x in R], t))} | {fm:.2f} |")
    return "\n".join(lines)
