#!/usr/bin/env python3
"""Render the showcase's publishable outputs from the pre-registered analysis ($0).

  python3 render.py [--analysis analysis.json] [--out-dir .] [--status auto|synthetic|interim-stopped-for-harm]

Writes <out-dir>/RESULTS.md (for people) and <out-dir>/results.json (for the website, schema
`adage-showcase-results/1`). Only what prereg.json defines is reported, worded by its `headline_rule`:
a change is called a cut or a rise only when its 90% interval excludes 0; otherwise it is "no detectable
change". Every family is reported, with n, spend and the pre-registered caveats. `--analysis` defaults to
analysis.json (analyze.py --out analysis.json); the dry run renders synthetic data with --status synthetic.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PREREG_PATH = HERE / "prereg.json"
PREREG = json.loads(PREREG_PATH.read_text())
SCHEMA = "adage-showcase-results/1"
LABEL = {"usd": "cost per task", "calls": "tool calls per task", "wall": "wall time per task"}


def pct(x: float | None, signed: bool = True) -> str:
    if x is None:
        return "n/a"
    return f"{x * 100:+.1f}%" if signed else f"{x * 100:.1f}%"


def ci(c: list | None) -> str:
    return f"[{c[0] * 100:+.1f}, {c[1] * 100:+.1f}]" if c else "n/a"


def verdict(r: dict | None) -> str:
    """The pre-registered wording: by the 90% interval alone."""
    if not r:
        return "not measured"
    lo, hi = r["ci90"]
    if hi < 0:
        return "cut"
    if lo > 0:
        return "raised"
    return "no detectable change"


def sentence(r: dict | None, what: str) -> str:
    v = verdict(r)
    if v == "cut":
        return f"cut the {what} by {abs(r['change']) * 100:.1f}% (90% CI {abs(r['ci90'][1]) * 100:.1f}-{abs(r['ci90'][0]) * 100:.1f}%)"
    if v == "raised":
        return f"raised the {what} by {r['change'] * 100:.1f}% (90% CI {r['ci90'][0] * 100:.1f}-{r['ci90'][1] * 100:.1f}%)"
    if v == "not measured":
        return f"{what}: not measured"
    return f"made no detectable change in the {what} ({pct(r['change'])}, 90% CI {ci(r['ci90'])})"


def metric(r: dict | None) -> dict | None:
    if not r:
        return None
    return {"change": r["change"], "ci90": r["ci90"], "verdict": verdict(r), "adage_mean": r["adage_mean"],
            "control_mean": r["control_mean"], "pairs": r["pairs"], "variants": r["variants"]}


def pass_block(c: dict) -> dict:
    return {"adage": c["arms"]["adage"]["pass"], "control": c["arms"]["control"]["pass"],
            "paired_diff": c["pass_diff"], "net": c["pass_net"],
            "verdict": ("lower" if c["pass_diff"]["ci90"][1] < 0 else "higher" if c["pass_diff"]["ci90"][0] > 0
                        else "no detectable change") if c.get("pass_diff") else "not measured"}


def contrast_block(c: dict | None) -> dict | None:
    if not c:
        return None
    return {"usd_per_task": metric(c["usd"]), "tool_calls": metric(c["calls"]), "wall_time": metric(c["wall"]),
            "pass_rate": pass_block(c), "card_delivery": c["card_delivery"],
            "censored_sessions": {"adage": c["arms"]["adage"]["censored"], "control": c["arms"]["control"]["censored"]},
            "usd_per_resolved_task": {"adage": c["arms"]["adage"]["usd_per_resolved"],
                                      "control": c["arms"]["control"]["usd_per_resolved"]},
            "sensitivity_uncensored_usd": metric(c.get("sensitivity_uncensored_usd"))}


def build(a: dict, status: str) -> dict:
    pr = PREREG
    comp = a.get("complete") or {}
    if status == "auto":
        status = "final" if comp.get("haiku_complete") and comp.get("sonnet_complete") else "incomplete"
    fams = []
    for fid in pr["design"]["families_in_order"]:
        f = a["families"].get(fid) or {}
        g = f.get("gate") or {}
        meta = pr["families"][fid]
        fams.append({"id": fid, "repository": meta["repository"], "ecosystem": meta["ecosystem"],
                     "task": meta["procedure"], "held_out_variants": len(meta["held_out"]),
                     "card_formed": g.get("card_formed"), "probe_delivered": g.get("held_out_delivered"),
                     "false_fires": g.get("false_fires"), "results": contrast_block(f.get("contrast")),
                     "delivered_subset_usd": metric(f.get("delivered_subset_usd")), "harness": f.get("harness")})
    son = {fid: contrast_block((a["sonnet"].get(fid) or {}).get("contrast")) for fid in pr["design"]["sonnet_arm"]["families"]}
    pooled = a["pooled"]
    caveats = list(pr["reporting"]["caveats_preregistered"])
    cen = pooled["arms"]["adage"]["censored"] + pooled["arms"]["control"]["censored"]
    if cen:
        caveats.append(f"{cen} held-out sessions hit the 60-turn cap and are counted at their cost so far "
                       "(sensitivity without them is reported).")
    if status != "final":
        caveats.insert(0, {"incomplete": "Not every pre-registered session finished; see `complete`.",
                           "synthetic": "SYNTHETIC DATA from the dry run: these numbers illustrate the pipeline, "
                                        "they are not results.",
                           "interim-stopped-for-harm": "Stopped at the interim harm check (prereg R4): only repeat 1."
                           }.get(status, status))
    return {
        "schema": SCHEMA, "status": status, "generated": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "prereg": {"path": "evals/showcase/prereg.json", "sha256": hashlib.sha256(PREREG_PATH.read_bytes()).hexdigest(),
                   "written": pr["written"]},
        "question": pr["question"],
        "design": {"families": len(fams), "held_out_variants_per_family": pr["design"]["held_out_per_family"],
                   "repeats": pr["design"]["repeats"], "training_sessions_per_family": pr["design"]["training_per_family"],
                   "arms": pr["design"]["arms"], "primary_model": "Claude Haiku 4.5",
                   "secondary_model": "Claude Sonnet 5 (2 families, 6 variants, 1 repeat)",
                   "interval": "90%, Rao-Wu bootstrap stratified by family, clustered by variant"},
        "primary": {"metric": "usd_per_task_change", "model": "claude-haiku-4-5", **(metric(pooled["usd"]) or {}),
                    "families": pooled["usd"]["families"] if pooled.get("usd") else 0,
                    "sentence": "Adage " + sentence(pooled.get("usd"), "cost per task")},
        "secondary": {"tool_calls": metric(pooled["calls"]), "wall_time": metric(pooled["wall"]),
                      "pass_rate": pass_block(pooled), "card_delivery": pooled["card_delivery"],
                      "false_fires": a["false_fires"]["pooled"], "false_fires_at_prompt": a["false_fires"]["at_prompt"],
                      "false_fires_per_family": a["false_fires"]["per_family"]},
        "diagnostic": {"sensitivity_uncensored_usd": metric(pooled.get("sensitivity_uncensored_usd")),
                       "probe_delivered_subset_usd": metric(pooled.get("delivered_subset_usd")),
                       "usd_per_resolved_task": {"adage": pooled["arms"]["adage"]["usd_per_resolved"],
                                                 "control": pooled["arms"]["control"]["usd_per_resolved"]},
                       "censored_sessions": cen},
        "families": fams,
        "sonnet": {"note": pr["reporting"]["sonnet_rule"], "pooled": contrast_block(a["sonnet"].get("pooled")),
                   "families": son},
        "complete": comp,
        "spend": {"total_usd": a["spend_total_usd"], "by_root": a["spend"],
                  "budget_central_usd": pr["budget"]["central_usd"]["total"], "cap_usd": pr["budget"]["caps_usd"]["global"]},
        "caveats": caveats,
        "data": {"analysis": "evals/showcase/analysis.json", "code": "evals/showcase/",
                 "raw_runs": ("evals/showcase/live/<root>/state.json: every session's metrics and hidden-check "
                              "result, plus each root's gate.json, learner log and learned home; transcripts "
                              "not committed" if status != "synthetic" and (HERE / "live").is_dir()
                              else "not committed (transcripts); roots listed in prereg.json")},
    }


def md(r: dict) -> str:
    p = r["primary"]
    s = r["secondary"]
    L = []
    title = "# Adage showcase: recurring work on five repositories"
    L += [title, ""]
    if r["status"] != "final":
        L += [f"> **Status: {r['status'].upper()}.** {r['caveats'][0]}", ""]
    L += [f"On 5 recurring-task families on 5 open-source repositories, with the product learning each family from "
          f"4 ordinary sessions by itself, {p['sentence']}. That covers {p.get('pairs', 0)} held-out pairs "
          f"({2 * p.get('pairs', 0)} sessions, {p.get('variants', 0)} task variants, {p.get('families', 0)} families) "
          f"with Claude Haiku 4.5. "
          f"Pre-registered in [prereg.json](prereg.json) (sha256 `{r['prereg']['sha256'][:12]}`).", ""]
    pr_ = s["pass_rate"]
    L += ["## Pooled (Claude Haiku 4.5)", "",
          "| metric | control | adage | change [90% CI] | reading |", "| --- | ---: | ---: | --- | --- |"]
    for key, name, fmt in (("primary", "cost per task", "${:.3f}"), ("tool_calls", "tool calls", "{:.1f}"),
                           ("wall_time", "wall time (s)", "{:.0f}")):
        m = p if key == "primary" else s[key]
        if m and m.get("change") is not None:
            L.append(f"| {name} | {fmt.format(m['control_mean'])} | {fmt.format(m['adage_mean'])} | "
                     f"{pct(m['change'])} {ci(m['ci90'])} | {m['verdict']} |")
    L.append(f"| hidden tests passed | {pr_['control']['k']}/{pr_['control']['n']} | {pr_['adage']['k']}/{pr_['adage']['n']} "
             f"| {pr_['paired_diff']['diff'] * 100:+.1f} pts {ci(pr_['paired_diff']['ci90'])} | {pr_['verdict']} |")
    cd, ff = s["card_delivery"], s["false_fires"]
    L += ["", f"- **Card delivered** in {cd['k']} of {cd['n']} Adage sessions ({pct(cd['rate'], False)}, "
              f"Wilson 90% [{cd['wilson90'][0] * 100:.0f}, {cd['wilson90'][1] * 100:.0f}]%).",
          f"- **False fires:** {ff['k']} of {ff['n']} same-repository negative prompts got a card "
          f"(upper bound {ff['wilson90'][1] * 100:.1f}%): {s['false_fires_at_prompt']['k']} at the prompt, "
          f"{ff['k'] - s['false_fires_at_prompt']['k']} at the first edit of a card file.",
          f"- **Censored at 60 turns:** {r['diagnostic']['censored_sessions']} sessions. Without them: "
          f"{pct((r['diagnostic']['sensitivity_uncensored_usd'] or {}).get('change'))} "
          f"{ci((r['diagnostic']['sensitivity_uncensored_usd'] or {}).get('ci90'))}.", ""]
    L += ["## Per family (8 held-out variants x 2 repeats each)", "",
          "| family | repository | ecosystem | cost per task [90% CI] | tool calls | passed (ctl / adage) | card delivered | false fires |",
          "| --- | --- | --- | --- | --- | --- | --- | --- |"]
    for f in r["families"]:
        x = f["results"]
        if not x:
            L.append(f"| {f['id']} | {f['repository']} | {f['ecosystem']} | not run | | | | |")
            continue
        u, c, pa = x["usd_per_task"], x["tool_calls"], x["pass_rate"]
        ffs = f["false_fires"] or ["?", "?"]
        L.append(f"| {f['id']} | {f['repository']} | {f['ecosystem']} | {pct(u['change'])} {ci(u['ci90'])} | "
                 f"{pct(c['change'])} | {pa['control']['k']}/{pa['control']['n']} / {pa['adage']['k']}/{pa['adage']['n']} | "
                 f"{x['card_delivery']['k']}/{x['card_delivery']['n']} | {ffs[0]}/{ffs[1]} |")
    L += ["", PREREG["reporting"]["caveats_preregistered"][3], ""]
    so = r["sonnet"]
    L += ["## A stronger model: Claude Sonnet 5 (directional)", ""]
    if so["pooled"]:
        u = so["pooled"]["usd_per_task"]
        L.append(f"On {u['pairs']} pairs ({', '.join(k for k, v in so['families'].items() if v)}), Adage "
                 f"{sentence({k: u[k] for k in ('change', 'ci90')}, 'cost per task')}; tool calls {pct(so['pooled']['tool_calls']['change'])} "
                 f"{ci(so['pooled']['tool_calls']['ci90'])}; hidden tests passed {so['pooled']['pass_rate']['control']['k']}/"
                 f"{so['pooled']['pass_rate']['control']['n']} (control) vs {so['pooled']['pass_rate']['adage']['k']}/"
                 f"{so['pooled']['pass_rate']['adage']['n']} (adage).")
        L += ["", f"Directional only: {so['note'][0].lower() + so['note'][1:]}."]
    else:
        L.append("Not run.")
    sp = r["spend"]
    L += ["", "## Spend", "",
          f"${sp['total_usd']:.2f} in total (training, held out, the Sonnet arm and any re-run included), against a "
          f"pre-registered central estimate of ${sp['budget_central_usd']:.2f} and a hard cap of ${sp['cap_usd']:.0f}.", ""]
    L += ["## Caveats", ""] + [f"- {c}" for c in r["caveats"]]
    L += ["", "## Data", "", "- Pre-registration: [prereg.json](prereg.json); analysis: `analyze.py` -> `analysis.json`; "
          "this page: `render.py` -> `RESULTS.md`, `results.json`.",
          f"- Complete: {json.dumps(r['complete'])}.", ""]
    return "\n".join(L)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--analysis", default=str(HERE / "analysis.json"))
    ap.add_argument("--out-dir", default=str(HERE))
    ap.add_argument("--status", default="auto", choices=["auto", "synthetic", "interim-stopped-for-harm"])
    a = ap.parse_args(argv)
    res = build(json.loads(Path(a.analysis).read_text()), a.status)
    out = Path(a.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    (out / "results.json").write_text(json.dumps(res, indent=1) + "\n")
    (out / "RESULTS.md").write_text(md(res))
    print(f"{out / 'RESULTS.md'}, {out / 'results.json'}: {res['status']}; {res['primary']['sentence']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
