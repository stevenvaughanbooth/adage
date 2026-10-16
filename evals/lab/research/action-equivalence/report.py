"""Markdown tables from results/*.json -> results/tables.md."""
import json

import data

R = data.HERE / "results"
ORDER = ["taxonomy", "intent", "head", "headsub", "literal", "lgg-loose", "lgg-L", "lgg-H", "tfidf", "potion", "a2v",
         "rrr", "triplet", "hybrid", "rrr-first", "hybrid-first"]
KIND = {"taxonomy": "hand-written (action.rs)", "intent": "hand-written", "head": "trivial", "headsub": "trivial",
        "literal": "trivial", "lgg-loose": "prior lab (hand-typed holes)", "lgg-L": "learned (4)", "lgg-H": "learned (4)",
        "tfidf": "learned (2)", "potion": "learned (2)", "a2v": "learned (1)", "rrr": "learned (3)", "triplet": "learned (3)",
        "hybrid": "learned (2+3)", "rrr-first": "learned (3, state-free)", "hybrid-first": "learned (2+3, state-free)"}


def gold_table():
    g = json.load(open(R / "gold_eval.json"))
    ci = json.load(open(R / "gold_ci.json"))
    env = json.load(open(R / "env_split.json"))
    rows = ["| representation | kind | pair P | pair R | **pair F1** [90% CI] | purity / inv. purity | classes on gold | 300-pair acc | py test runs: same-env recall / cross-env split | LORO F1 within / cross |",
            "|---|---|---:|---:|---|---|---:|---:|---|---|"]
    for k in ORDER:
        a = g["in_repo"][k]
        lo = g["loro"][k]
        w = sum(v["within"]["F1"] for v in lo.values()) / len(lo)
        c = sum(v["cross"]["F1"] for v in lo.values()) / len(lo)
        f = f"**{a['F1']:.2f}**" + (f" [{ci[k][1]:.2f}, {ci[k][2]:.2f}]" if k in ci else "")
        e = env.get(k)
        es = f"{e['test']['same_env_recall']:.2f} / {e['test']['cross_env_split']:.2f}" if e else "–"
        rows.append(f"| {k} | {KIND[k]} | {a['P']:.2f} | {a['R']:.2f} | {f} | {a['purity']:.2f} / {a['inv_purity']:.2f} | {a['clusters']} | {g['pairs300'][k]['acc']:.2f} | {es} | {w:.2f} / {c:.2f} |")
    per = ["| representation | " + " | ".join(data.REPOS) + " |", "|---|" + "---:|" * len(data.REPOS)]
    for k in ORDER:
        lo = g["loro"][k]
        per.append(f"| {k} | " + " | ".join(f"{lo[r]['within']['F1']:.2f} / {lo[r]['cross']['F1']:.2f}" for r in data.REPOS) + " |")
    return "\n".join(rows), "\n".join(per)


def down_table(setting, mode):
    src = json.load(open(R / ("downstream_loro.json" if setting == "loro" else "downstream_in.json")))[setting]
    rows = ["| representation | directives | useful (Wilson90 LB) | Ŝ calls/session [90% CI] | % of calls | Ŝ strict-Sat | $/session |",
            "|---|---:|---:|---|---:|---:|---:|"]
    for k in sorted(src, key=lambda k: -src[k][mode]["S_calls"]):
        a = src[k][mode]
        rows.append(f"| {k} | {a['directives']} | {a['useful_rate']:.2f} ({a['useful_wilson90'][0]:.2f}) | {a['S_calls']:+.2f} [{a['S_ci90'][0]:+.2f}, {a['S_ci90'][1]:+.2f}] | "
                    f"{100 * a['S_pct_calls']:+.1f}% | {a['S_strict_calls']:+.2f} | {a['S_usd']:+.4f} |")
    return "\n".join(rows)


def harness_table():
    h = json.load(open(R / "harness.json"))
    rows = ["| policy (learner × representation) | driver | $ saved/task [90% CI] | share of $ [90% CI] | calls saved/task [90% CI] | directives | useful |",
            "|---|---|---|---|---|---:|---:|"]
    for n in ("taxonomy", "rrr-first", "hybrid-first", "lgg-loose"):
        for d in ("split", "loro", "leave_task_out", "leave_task_out_strict", "leave_task_out_pfollow05"):
            if d not in h[n]:
                continue
            s = h[n][d]["summary"]
            rows.append(f"| {n} | {d} | {s['usd_saved_per_task']:+.4f} [{s['usd_saved_ci'][0]:+.4f}, {s['usd_saved_ci'][1]:+.4f}] | "
                        f"{100 * s['usd_saved_share']:+.1f}% [{100 * s['usd_saved_share_ci'][0]:+.1f}, {100 * s['usd_saved_share_ci'][1]:+.1f}] | "
                        f"{s['calls_saved_per_task']:+.2f} [{s['calls_saved_ci'][0]:+.2f}, {s['calls_saved_ci'][1]:+.2f}] | {s['directives']} | {s['useful_rate']:.2f} |")
    return "\n".join(rows)


if __name__ == "__main__":
    a, b = gold_table()
    out = ["## Gold (intrinsic)", a, "", "## LORO per repository (within / cross pair F1)", b]
    for st in ("train-split", "grown", "loro"):
        for md in ("post-hoc", "pre-learned", "pre-hindsight"):
            out += ["", f"## Downstream {st} / {md}", down_table(st, md)]
    out += ["", "## Harness", harness_table()]
    (R / "tables.md").write_text("\n".join(out) + "\n")
    print("\n".join(out))
