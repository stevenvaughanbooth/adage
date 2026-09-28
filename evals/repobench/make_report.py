#!/usr/bin/env python3
"""RepoBench report: tables, SVG charts and schema-v2 L3 records from the runner roots.

  python3 evals/repobench/make_report.py --runs /tmp/adage-repobench/runs --out evals/bench/results/<date>-repobench
        [--curves /tmp/adage-repobench/curves] [--narrative evals/repobench/narrative.md]

Reads every runner root under --runs (state.json), classifies each by name:
  <repo-id>              the headline (Claude Haiku 4.5): arms control, adage, agents-md, instincts, e1
  <repo-id>-sonnet5      the Claude Sonnet 5 slice
  gen-<bundle>           the discoverable-conventions control (evals/gen sandboxes with a CLAUDE.md)
and every learning curve under --curves (<repo-id>/curve.json). Writes <out>/README.md (tables,
the charts, the narrative file embedded), <out>/*.svg and <out>/results.jsonl (schema v2, tier
l3: one `arms` record per repository and comparison, and one pooled).

Statistics are the live runner's (demo/report.py): the task is the unit (pooled comparisons
treat (repository, task) as the task); CIs resample tasks; p permutes arms within tasks; Holm
across the metrics of each comparison; the sign-flip p asks whether the effect holds for new
tasks. `$ per resolved task` (the user's "$ per fix") is total cost / tasks resolved, per arm,
with a task-bootstrap CI of the difference. Harm: tasks the control resolved more often than the
treated arm.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import statistics
import sys
from pathlib import Path

import common

sys.path.insert(0, str(common.REPO / "evals" / "live"))
import runner  # noqa: E402 - and through it demo/report.py, the runner's statistics

demo_report = runner.demo_report

ARM_COLORS = {"control": "#2a78d6", "adage": "#eb6834", "agents-md": "#1baf7a", "instincts": "#eda100",
              "e1": "#e87ba4"}
ARM_ORDER = ["control", "adage", "agents-md", "instincts", "e1"]
INK, MUTED, GRID, SURFACE = "#1f1f1e", "#6b6a64", "#e4e3dc", "#fcfcfb"
HEADLINE_METRICS = ["pass_rate", "cost_usd", "total_turns", "turns_to_first_edit", "elapsed_s", "dead_end_hits",
                    "follow_rate"]
COMPARISONS = [("adage", "control"), ("agents-md", "control"), ("adage", "agents-md"), ("instincts", "control"),
               ("e1", "control")]
# EngineParams of the `--demo` preset every learn phase used (evals/baselines/l0-core.jsonl).
DEMO_PARAMS = {"learn": {"dead_end_consistency": [9, 10], "dead_end_signatures": 3, "dormant_exposure": 0.5,
                         "finish_line_contrast": 0.3, "fire_lower_bound": 0.5, "intent_contrast": 0.3,
                         "intent_ubiquitous": 0.8, "material_lower_bound": 0.1, "min_support": 3, "prime_rate": 0.5,
                         "retain_rate": 0.3},
               "runtime": {"control_share": 0.1, "follow_window": 3, "history": 32, "max_directives_per_decision": 3,
                           "max_directives_per_session": 8, "max_stop_requests": 2}}


# ------------------------------------------------------------------------------------ loading

def load_root(root: Path) -> dict:
    state = json.loads((root / "state.json").read_text())
    return state


def eval_runs(state: dict, repo: str) -> list[dict]:
    """The held-out runs with metrics, keyed for pooling: task = '<repo>/<task>'."""
    out = []
    for rid, r in state.get("runs", {}).items():
        if r.get("phase") != "eval" or not r.get("metrics"):
            continue
        out.append({"id": rid, "repo": repo, "task": f"{repo}/{r['task']}", "raw_task": r["task"], "arm": r["arm"],
                    "metrics": r["metrics"], "events": r.get("events", []), "score": r.get("score") or {}})
    return out


def train_runs(state: dict) -> list[dict]:
    return [r for r in state.get("runs", {}).values() if r.get("phase") == "train" and r.get("metrics")]


def kept(runs, max_turns):
    return [r for r in runs if demo_report.status_of(r["metrics"], max_turns)[0] not in demo_report.EXCLUDED]


# ---------------------------------------------------------------------------------- statistics

def per_resolved(runs):
    cost = sum(r["metrics"].get("cost_usd") or 0 for r in runs)
    solved = sum(1 for r in runs if r["metrics"].get("passed"))
    return cost / solved if solved else None


def per_resolved_ci(runs, a, b, n=2000, seed=11):
    """Task-bootstrap 95% CI of $/resolved(a) - $/resolved(b); resamples whole tasks."""
    tasks = {}
    for r in runs:
        if r["arm"] in (a, b):
            tasks.setdefault(r["task"], {a: [], b: []})[r["arm"]].append(r)
    groups = [g for g in tasks.values() if g[a] and g[b]]
    if len(groups) < 2:
        return None
    rng = random.Random(seed)
    diffs = []
    for _ in range(n):
        xa, xb = [], []
        for _ in groups:
            g = groups[rng.randrange(len(groups))]
            xa += g[a]
            xb += g[b]
        pa, pb = per_resolved(xa), per_resolved(xb)
        if pa is not None and pb is not None:
            diffs.append(pa - pb)
    if len(diffs) < n * 0.5:
        return None
    diffs.sort()
    return [demo_report._quantile(diffs, 0.025), demo_report._quantile(diffs, 0.975)]


def harm(runs, treated, control="control"):
    """(tasks the control resolved more often, tasks the treated arm resolved more often, tasks)."""
    by = {}
    for r in runs:
        if r["arm"] in (treated, control):
            by.setdefault(r["task"], {treated: [], control: []})[r["arm"]].append(1.0 if r["metrics"].get("passed") else 0.0)
    worse = better = n = 0
    for g in by.values():
        if g[treated] and g[control]:
            n += 1
            d = statistics.fmean(g[treated]) - statistics.fmean(g[control])
            worse += d < 0
            better += d > 0
    return worse, better, n


def comparison(runs, a, b, max_turns):
    cmp = demo_report.compare(runs, max_turns, a=a, b=b)
    ks = kept(runs, max_turns)
    ka = [r for r in ks if r["arm"] == a]
    kb = [r for r in ks if r["arm"] == b]
    pa, pb = per_resolved(ka), per_resolved(kb)
    w, bt, n = harm(ks, a, b)
    return {"a": a, "b": b, "cmp": cmp, "per_resolved": {a: pa, b: pb,
            "diff": None if pa is None or pb is None else pa - pb, "ci95": per_resolved_ci(ks, a, b)},
            "harm": {"worse": w, "better": bt, "tasks": n},
            "n_tasks": len({r["task"] for r in ks if r["arm"] in (a, b)}),
            "runs": {a: len(ka), b: len(kb)}}


# ------------------------------------------------------------------------------------ learned

def learned(state: dict) -> dict:
    tracks = state.get("tracks") or []
    reflexes = [t for t in tracks if not common.is_instinct(t)]
    instincts = [t for t in tracks if common.is_instinct(t)]
    eng = (((state.get("learn") or {}).get("repos") or {}))
    dropped, skipped = [], []
    for v in eng.values():
        dropped += [d.get("reason") for d in (v.get("engine") or {}).get("dropped", [])]
        skipped += (v.get("instincts") or {}).get("skipped", [])
    return {"reflexes": [(t["route"]["id"], t["route"].get("summary"), t["route"].get("strength"),
                          t["route"].get("state")) for t in reflexes],
            "instincts": [(t["route"]["id"], t["route"].get("summary")) for t in instincts],
            "dropped": dropped, "instinct_skipped": skipped}


def route_commands(route: dict) -> list[str]:
    """Every command text a route directs or names (its step and trigger), for matching."""
    out = []

    def walk(x):
        if isinstance(x, dict):
            for k, v in x.items():
                if k in ("run", "command", "cmd", "instead", "literal", "template") and isinstance(v, str):
                    out.append(v)
                walk(v)
        elif isinstance(x, list):
            for v in x:
                walk(v)
    walk({"step": route.get("step"), "trigger": route.get("trigger"), "slots": route.get("slots")})
    return out


def norm(cmd: str) -> str:
    c = " ".join(cmd.replace("./", "").split())
    for p in ("source .venv/bin/activate && ", ".venv/bin/"):
        c = c.replace(p, "")
    return c


def match_conventions(state: dict, conv: dict) -> dict:
    """Which ground-truth conventions some live route names (command-head match), and which routes
    match none. Coarser than `adage eval routes`, which needs a dataset: a route matches a
    convention when one of its commands, normalised (venv prefixes dropped), starts with the
    convention's command head (its first two words)."""
    routes = [t["route"] for t in state.get("tracks") or []]
    found, unmatched = set(), []
    for rt in routes:
        cmds = [norm(c) for c in route_commands(rt)]
        hit = False
        for c in conv.get("conventions", []):
            head = " ".join(norm(c["command"]).split()[:2])
            if any(x.startswith(head) for x in cmds):
                found.add(c["id"])
                hit = True
        if not hit:
            unmatched.append(rt["id"])
    return {"found": sorted(found), "total": len(conv.get("conventions", [])), "unmatched_routes": unmatched}


# --------------------------------------------------------------------------------------- SVG

def esc(s) -> str:
    return str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


def nice_max(v: float) -> float:
    if v <= 0:
        return 1.0
    e = 10 ** math.floor(math.log10(v))
    for m in (1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10):
        if m * e >= v:
            return m * e
    return 10 * e


def svg_bars(title: str, groups: list[str], arms: list[str], values: dict, cis: dict, fmt, unit: str,
             width=720, height=300) -> str:
    """Grouped vertical bars: groups on x, one bar per arm, value labels, optional CI whiskers."""
    left, right, top, bottom = 56, 16, 44, 56
    pw, ph = width - left - right, height - top - bottom
    vmax = nice_max(max([v for g in groups for a in arms for v in [values.get((g, a))] if v is not None]
                        + [ci[1] for ci in cis.values() if ci] + [1e-9]) * 1.08)
    gw = pw / max(1, len(groups))
    bw = min(28, (gw - 16) / max(1, len(arms)) - 2)
    y = lambda v: top + ph - ph * v / vmax
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" '
         f'font-family="system-ui, -apple-system, Segoe UI, sans-serif" role="img" aria-label="{esc(title)}">',
         f'<rect width="100%" height="100%" fill="{SURFACE}"/>',
         f'<text x="{left}" y="20" font-size="14" font-weight="600" fill="{INK}">{esc(title)}</text>']
    for i in range(5):
        v = vmax * i / 4
        o.append(f'<line x1="{left}" x2="{left + pw}" y1="{y(v):.1f}" y2="{y(v):.1f}" stroke="{GRID}" stroke-width="1"/>')
        o.append(f'<text x="{left - 6}" y="{y(v) + 4:.1f}" font-size="11" fill="{MUTED}" text-anchor="end">{esc(fmt(v))}</text>')
    lx = left
    for a in arms:
        o.append(f'<rect x="{lx}" y="28" width="10" height="10" rx="2" fill="{ARM_COLORS.get(a, MUTED)}"/>'
                 f'<text x="{lx + 14}" y="37" font-size="11" fill="{INK}">{esc(a)}</text>')
        lx += 20 + 7 * len(a) + 10
    for gi, g in enumerate(groups):
        x0 = left + gi * gw + (gw - len(arms) * (bw + 2)) / 2
        for ai, a in enumerate(arms):
            v = values.get((g, a))
            if v is None:
                continue
            x = x0 + ai * (bw + 2)
            h = max(0.5, ph * v / vmax)
            o.append(f'<path d="M{x:.1f},{top + ph:.1f} V{top + ph - h + 4:.1f} q0,-4 4,-4 h{bw - 8:.1f} q4,0 4,4 '
                     f'V{top + ph:.1f} Z" fill="{ARM_COLORS.get(a, MUTED)}"><title>{esc(g)} · {esc(a)}: {esc(fmt(v))}{esc(unit)}</title></path>')
            ci = cis.get((g, a))
            if ci:
                cx = x + bw / 2
                o.append(f'<line x1="{cx:.1f}" x2="{cx:.1f}" y1="{y(ci[0]):.1f}" y2="{y(ci[1]):.1f}" stroke="{INK}" stroke-width="1.5"/>')
            o.append(f'<text x="{x + bw / 2:.1f}" y="{y(v) - 5:.1f}" font-size="10" fill="{INK}" text-anchor="middle">{esc(fmt(v))}</text>')
        o.append(f'<text x="{left + gi * gw + gw / 2:.1f}" y="{top + ph + 18}" font-size="11" fill="{INK}" text-anchor="middle">{esc(g)}</text>')
    o.append(f'<line x1="{left}" x2="{left + pw}" y1="{top + ph}" y2="{top + ph}" stroke="{MUTED}" stroke-width="1"/>')
    o.append("</svg>")
    return "\n".join(o)


def svg_curve(title: str, series: dict, ylabel: str, fmt, width=720, height=280, smooth=4) -> str:
    """Lines over session index: raw points (dots) and a trailing mean (line), per arm."""
    left, right, top, bottom = 56, 90, 44, 40
    pw, ph = width - left - right, height - top - bottom
    n = max((len(v) for v in series.values()), default=1)
    vals = [p for v in series.values() for p in v if p is not None]
    vmax = nice_max(max(vals + [1e-9]) * 1.05)
    x = lambda i: left + pw * (i - 1) / max(1, n - 1)
    y = lambda v: top + ph - ph * v / vmax
    o = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" '
         f'font-family="system-ui, -apple-system, Segoe UI, sans-serif" role="img" aria-label="{esc(title)}">',
         f'<rect width="100%" height="100%" fill="{SURFACE}"/>',
         f'<text x="{left}" y="20" font-size="14" font-weight="600" fill="{INK}">{esc(title)}</text>',
         f'<text x="{left}" y="36" font-size="11" fill="{MUTED}">{esc(ylabel)}: dots are sessions, lines the mean of the last {smooth}</text>']
    for i in range(5):
        v = vmax * i / 4
        o.append(f'<line x1="{left}" x2="{left + pw}" y1="{y(v):.1f}" y2="{y(v):.1f}" stroke="{GRID}"/>')
        o.append(f'<text x="{left - 6}" y="{y(v) + 4:.1f}" font-size="11" fill="{MUTED}" text-anchor="end">{esc(fmt(v))}</text>')
    for i in range(1, n + 1):
        if i == 1 or i % 5 == 0 or i == n:
            o.append(f'<text x="{x(i):.1f}" y="{top + ph + 16}" font-size="11" fill="{MUTED}" text-anchor="middle">{i}</text>')
    o.append(f'<text x="{left + pw / 2:.1f}" y="{height - 6}" font-size="11" fill="{MUTED}" text-anchor="middle">sessions seen (task order = fix date)</text>')
    for arm, pts in series.items():
        col = ARM_COLORS.get(arm, MUTED)
        for i, v in enumerate(pts, 1):
            if v is not None:
                o.append(f'<circle cx="{x(i):.1f}" cy="{y(v):.1f}" r="4" fill="{col}" fill-opacity="0.45" stroke="{SURFACE}" '
                         f'stroke-width="2"><title>{esc(arm)} session {i}: {esc(fmt(v))}</title></circle>')
        roll = []
        for i in range(1, len(pts) + 1):
            w = [p for p in pts[max(0, i - smooth):i] if p is not None]
            roll.append(statistics.fmean(w) if w else None)
        d = " ".join(f"{'M' if j == 0 else 'L'}{x(i + 1):.1f},{y(v):.1f}" for j, (i, v) in
                     enumerate((i, v) for i, v in enumerate(roll) if v is not None))
        o.append(f'<path d="{d}" fill="none" stroke="{col}" stroke-width="2" stroke-linejoin="round"/>')
        last = next(((i, v) for i, v in reversed(list(enumerate(roll))) if v is not None), None)
        if last:
            o.append(f'<text x="{x(last[0] + 1) + 8:.1f}" y="{y(last[1]) + 4:.1f}" font-size="11" fill="{INK}">{esc(arm)} {esc(fmt(last[1]))}</text>')
    o.append(f'<line x1="{left}" x2="{left + pw}" y1="{top + ph}" y2="{top + ph}" stroke="{MUTED}"/>')
    o.append("</svg>")
    return "\n".join(o)


# ------------------------------------------------------------------------------------- tables

def f(x, d=2):
    if x is None:
        return "–"
    if isinstance(x, float):
        return f"{x:.{d}f}"
    return str(x)


def cmp_table(c: dict) -> str:
    a, b = c["a"], c["b"]
    rows = [f"| Metric | {a} | {b} | diff | 95% CI (tasks) | p | p (Holm) | p (tasks, sign-flip) |",
            "| --- | ---: | ---: | ---: | --- | ---: | ---: | ---: |"]
    for name in HEADLINE_METRICS:
        r = c["cmp"]["metrics"].get(name)
        if not r:
            continue
        ci = r.get("ci95")
        rows.append(f"| {name} ({r['aggregate']}) | {f(r.get(a))} | {f(r.get(b))} | {f(r.get('diff'))} | "
                    f"{'[' + f(ci[0]) + ', ' + f(ci[1]) + ']' if ci else '–'} | {f(r.get('p_value'), 3)} | "
                    f"{f(r.get('p_holm'), 3)} | {f(r.get('p_tasks'), 3)} |")
    pr = c["per_resolved"]
    ci = pr["ci95"]
    rows.append(f"| $ per resolved task | {f(pr[a])} | {f(pr[b])} | {f(pr['diff'])} | "
                f"{'[' + f(ci[0]) + ', ' + f(ci[1]) + ']' if ci else '–'} | – | – | – |")
    h = c["harm"]
    return "\n".join(rows) + (f"\n\nTasks: {c['n_tasks']}; runs {a} {c['runs'][a]}, {b} {c['runs'][b]}. "
                              f"Harm: {h['worse']} of {h['tasks']} tasks resolved less often with {a} than with {b}; "
                              f"{h['better']} more often.")


# ----------------------------------------------------------------------------------- records

def canon(x) -> str:
    return json.dumps(x, sort_keys=True, separators=(",", ":"))


def arms_record(suite: str, unit: str, split_kind: str, dataset: dict | None, c: dict, adage_version: str,
                sessions: int) -> dict:
    a, b = c["a"], c["b"]
    arms = {}
    for arm in (a, b):
        vals = {}
        for m in HEADLINE_METRICS:
            v = c["cmp"]["metrics"].get(m, {}).get(arm)
            if v is not None:
                vals[m] = round(float(v), 6)
        if c["per_resolved"].get(arm) is not None:
            vals["usd_per_resolved"] = round(c["per_resolved"][arm], 6)
        vals["runs"] = c["runs"][arm]
        arms[arm] = vals
    deltas = {}
    for m in HEADLINE_METRICS:
        r = c["cmp"]["metrics"].get(m) or {}
        if r.get("diff") is not None:
            deltas[m] = round(float(r["diff"]), 6)
            if r.get("ci95"):
                deltas[f"{m}.ci_low"], deltas[f"{m}.ci_high"] = (round(float(v), 6) for v in r["ci95"])
            if r.get("p_holm") is not None:
                deltas[f"{m}.p_holm"] = round(float(r["p_holm"]), 6)
    if c["per_resolved"]["diff"] is not None:
        deltas["usd_per_resolved"] = round(c["per_resolved"]["diff"], 6)
    deltas = dict(list(deltas.items())[:32])
    params_hash = hashlib.sha256(canon(DEMO_PARAMS).encode()).hexdigest()
    suite_def = {"suite": suite, "unit": unit, "comparison": f"{a}-vs-{b}"}
    run_id = hashlib.sha256(canon({"schema_version": 2, "tier": "l3", "suite": suite_def,
                                   "datasets": [dataset["content_sha256"]] if dataset else [],
                                   "params_hash": params_hash}).encode()).hexdigest()[:16]
    rec = {"schema_version": 2, "record": "arms", "run_id": run_id, "tier": "l3", "suite": suite,
           "adage_version": adage_version, "params": DEMO_PARAMS, "params_hash": params_hash, "exec_level": "e0",
           "split": {"kind": split_kind, "name": unit, "seed": "20260928"},
           "arms": arms, "deltas": deltas,
           "harm": {f"{a}_worse_than_{b}": c["harm"]["worse"], f"{a}_better_than_{b}": c["harm"]["better"]},
           "sessions": sum(c["runs"].values())}
    if dataset:
        rec["dataset"] = dataset
    return rec


def dataset_of(rid: str) -> dict:
    p = common.TASKS_DIR / f"{rid}.json"
    return {"id": f"repobench-local/{rid}", "content_sha256": hashlib.sha256(p.read_bytes()).hexdigest()}


# --------------------------------------------------------------------------------------- main

def spend_rows(roots: dict, curves: dict) -> list[tuple[str, int, float]]:
    rows = []
    for name, st in sorted(roots.items()):
        runs = [r for r in st["runs"].values() if r.get("metrics")]
        rows.append((name, len(runs), sum(r["metrics"].get("cost_usd") or 0 for r in runs)))
    for name, cv in sorted(curves.items()):
        rows.append((f"curve {name}", len(cv["entries"]), sum(e["metrics"].get("cost_usd") or 0 for e in cv["entries"])))
    return rows


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--runs", default=str(common.cache_dir() / "runs"))
    ap.add_argument("--curves", default=str(common.cache_dir() / "curves"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--narrative", default=str(common.HERE / "narrative.md"))
    ap.add_argument("--extra-spend", default=str(common.cache_dir() / "extra-spend.json"),
                    help="JSON list of [item, sessions, usd] rows not in any root (discarded runs, probes)")
    a = ap.parse_args(argv)
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    repos = common.load_repos()
    roots = {}
    for p in sorted(Path(a.runs).iterdir()):
        if (p / "state.json").exists() and not p.name.startswith("pilot"):
            roots[p.name] = load_root(p)
    curves = {}
    cdir = Path(a.curves)
    if cdir.exists():
        for p in sorted(cdir.iterdir()):
            if (p / "curve.json").exists():
                curves[p.name] = json.loads((p / "curve.json").read_text())
    adage_version = "0.1.0"
    md, records = [], []
    headline = {k: v for k, v in roots.items() if k in repos}
    sonnet = {k[: -len("-sonnet5")]: v for k, v in roots.items() if k.endswith("-sonnet5")}
    gen = {k: v for k, v in roots.items() if k.startswith("gen-")}

    md.append("# RepoBench-Local: Adage on real repositories\n")
    narrative = Path(a.narrative)
    if narrative.exists():
        md.append(narrative.read_text().rstrip() + "\n")

    # ---- repositories and tasks
    md.append("## Repositories, tasks and what Adage learned\n")
    md.append("| Repository | Pinned window | Tasks (train / buffer / test) | Split digest | Train sessions passed | "
              "Reflexes learned | Instinct routes | Conventions found (live index) | AGENTS.md lines |")
    md.append("| --- | --- | --- | --- | ---: | --- | ---: | --- | ---: |")
    learned_by = {}
    for rid, st in headline.items():
        sp = json.loads((common.HERE / "splits" / f"{rid}.json").read_text())
        part = sp["repos"][rid]
        tr = train_runs(st)
        L = learned(st)
        learned_by[rid] = L
        conv_p = common.CONVENTIONS_DIR / f"{rid}.json"
        conv = match_conventions(st, json.loads(conv_p.read_text())) if conv_p.exists() else None
        ctx = common.HERE / "context" / f"{rid}.md"
        md.append(f"| [{rid}]({repos[rid]['url']}) | {part['dates'][part['train'][0]][:10]} – "
                  f"{part['dates'][part['test'][-1]][:10]} | {len(part['train'])} / {len(part['buffer'])} / "
                  f"{len(part['test'])} | `{sp['digest'][:12]}` | {sum(1 for r in tr if r['metrics'].get('passed'))}/{len(tr)} | "
                  f"{', '.join(x[0] for x in L['reflexes']) or 'none'} | {len(L['instincts'])} | "
                  f"{(str(len(conv['found'])) + '/' + str(conv['total'])) if conv else '–'} | "
                  f"{len(ctx.read_text().splitlines()) if ctx.exists() else '–'} |")
    md.append("")
    md.append("Why nothing (or little) was learned, from `adage condition`'s own report (reasons, deduplicated):\n")
    for rid, L in learned_by.items():
        reasons = sorted({(r or "").split(" (")[0] for r in L["dropped"]})
        md.append(f"- **{rid}**: {len(L['dropped'])} candidate patterns dropped"
                  + (f" ({'; '.join(reasons[:6])})" if reasons else "")
                  + (f". Instincts not bound: {'; '.join(sorted(set(L['instinct_skipped'])))}" if L["instinct_skipped"] else ""))
    md.append("")

    # ---- pooled headline comparisons
    all_runs = [r for rid, st in headline.items() for r in eval_runs(st, rid)]
    max_turns = max((st.get("max_turns") or 30) for st in headline.values()) if headline else 30
    pooled = {}
    if all_runs:
        md.append("## Headline: pooled over repositories (Claude Haiku 4.5)\n")
        arms_present = [x for x in ARM_ORDER if any(r["arm"] == x for r in all_runs)]
        md.append(f"Arms run: {', '.join(arms_present)}. Tasks are (repository, task) pairs; every table is task-clustered.\n")
        for ta, tb in COMPARISONS:
            if ta in arms_present and tb in arms_present:
                c = comparison(all_runs, ta, tb, max_turns)
                pooled[(ta, tb)] = c
                md.append(f"### {ta} vs {tb}\n")
                md.append(cmp_table(c) + "\n")
                records.append(arms_record("repobench-local", "all", "suite", None, c, adage_version,
                                           sum(c["runs"].values())))
        # charts
        groups = [rid for rid in headline]
        vals, cis, vals2 = {}, {}, {}
        for rid in groups + ["all"]:
            rr = kept(all_runs if rid == "all" else [r for r in all_runs if r["repo"] == rid], max_turns)
            for arm in arms_present:
                ar = [r for r in rr if r["arm"] == arm]
                if ar:
                    vals[(rid, arm)] = statistics.fmean(r["metrics"].get("cost_usd") or 0 for r in ar)
                    vals2[(rid, arm)] = per_resolved(ar)
                    vals[(rid + " pass", arm)] = statistics.fmean(1.0 if r["metrics"].get("passed") else 0.0 for r in ar)
        (out / "cost-per-task.svg").write_text(svg_bars("Mean cost per task (USD), by repository", groups + ["all"],
                                                        arms_present, vals, {}, lambda v: f"${v:.2f}", ""))
        (out / "usd-per-resolved.svg").write_text(svg_bars("$ per resolved task (total cost / tasks resolved)",
                                                           groups + ["all"], arms_present, vals2, {},
                                                           lambda v: f"${v:.2f}", ""))
        pv = {(g[: -len(" pass")], arm): v for (g, arm), v in vals.items() if g.endswith(" pass")}
        (out / "resolve-rate.svg").write_text(svg_bars("Resolve rate (hidden tests pass), by repository",
                                                       groups + ["all"], arms_present, pv, {},
                                                       lambda v: f"{v * 100:.0f}%", ""))
        md.append("![Mean cost per task](cost-per-task.svg)\n")
        md.append("![$ per resolved task](usd-per-resolved.svg)\n")
        md.append("![Resolve rate](resolve-rate.svg)\n")

        # ---- per repository
        md.append("## Per repository\n")
        md.append("| Repository | Arm | Runs | Resolve rate | Mean cost | $ per resolved | Median turns | Median turns to first edit | Hit max turns | Follow rate |")
        md.append("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
        for rid in groups:
            rr = [r for r in all_runs if r["repo"] == rid]
            cmp = demo_report.compare(rr, max_turns)
            for arm in arms_present:
                ar = kept([r for r in rr if r["arm"] == arm], max_turns)
                if not ar:
                    continue
                m = cmp["metrics"]
                md.append(f"| {rid} | {arm} | {len(ar)} | {f(m['pass_rate'].get(arm))} | {f(m['cost_usd'].get(arm), 3)} | "
                          f"{f(per_resolved(ar))} | {f(m['total_turns'].get(arm), 1)} | {f(m['turns_to_first_edit'].get(arm), 1)} | "
                          f"{cmp['n_censored'].get(arm, 0)} | {f(m['follow_rate'].get(arm))} |")
            for ta, tb in COMPARISONS:
                if any(r["arm"] == ta for r in rr) and any(r["arm"] == tb for r in rr):
                    c = comparison(rr, ta, tb, max_turns)
                    records.append(arms_record("repobench-local", rid, "temporal", dataset_of(rid), c, adage_version,
                                               sum(c["runs"].values())))
        md.append("")

    # ---- Sonnet slice
    if sonnet:
        md.append("## Claude Sonnet 5 slice\n")
        sr = [r for rid, st in sonnet.items() for r in eval_runs(st, rid)]
        arms_present = [x for x in ARM_ORDER if any(r["arm"] == x for r in sr)]
        md.append(f"Repositories: {', '.join(sonnet)}; arms {', '.join(arms_present)}.\n")
        for ta, tb in COMPARISONS:
            if ta in arms_present and tb in arms_present:
                c = comparison(sr, ta, tb, max_turns)
                md.append(f"### Sonnet 5: {ta} vs {tb}\n")
                md.append(cmp_table(c) + "\n")
                records.append(arms_record("repobench-local-sonnet5", "all", "suite", None, c, adage_version,
                                           sum(c["runs"].values())))

    # ---- learning curves
    if curves:
        md.append("## Learning curve: does it get cheaper as you use it?\n")
        md.append("| Repository | Arm | Sessions | Resolve rate | Mean cost, sessions 1–5 | Mean cost, last 5 | Median turns, 1–5 | Median turns, last 5 | Reflexes live at the end |")
        md.append("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |")
        for rid, cv in curves.items():
            series_cost, series_turns = {}, {}
            for arm in ("control", "adage"):
                es = sorted((e for e in cv["entries"] if e["arm"] == arm), key=lambda e: e["pos"])
                if not es:
                    continue
                cost = [e["metrics"].get("cost_usd") for e in es]
                turns = [e["metrics"].get("turns") for e in es]
                series_cost[arm], series_turns[arm] = cost, turns
                c5 = [x for x in cost[:5] if x is not None]
                cl = [x for x in cost[-5:] if x is not None]
                t5 = [x for x in turns[:5] if x is not None]
                tl = [x for x in turns[-5:] if x is not None]
                ends = es[-1].get("reflexes_after") or []
                passed = [1.0 if e["metrics"].get("passed") else 0.0 for e in es]
                md.append(f"| {rid} | {arm} | {len(es)} | {f(statistics.fmean(passed))} | {f(statistics.fmean(c5) if c5 else None, 3)} | "
                          f"{f(statistics.fmean(cl) if cl else None, 3)} | {f(statistics.median(t5) if t5 else None, 1)} | "
                          f"{f(statistics.median(tl) if tl else None, 1)} | {', '.join(ends) if arm == 'adage' else '–'} |")
            (out / f"curve-{rid}-cost.svg").write_text(svg_curve(f"{rid}: cost per task as sessions accumulate",
                                                                 series_cost, "USD per session", lambda v: f"${v:.2f}"))
            (out / f"curve-{rid}-turns.svg").write_text(svg_curve(f"{rid}: turns per task as sessions accumulate",
                                                                  series_turns, "turns per session", lambda v: f"{v:.0f}"))
        md.append("")
        for rid in curves:
            md.append(f"![{rid} cost curve](curve-{rid}-cost.svg)\n")
            md.append(f"![{rid} turns curve](curve-{rid}-turns.svg)\n")
        md.append("Per session (adage sequence): reflexes live before the session, from `curve.json`:\n")
        for rid, cv in curves.items():
            es = sorted((e for e in cv["entries"] if e["arm"] == "adage"), key=lambda e: e["pos"])
            md.append(f"- **{rid}**: " + ", ".join(f"{e['pos']}:{e['reflexes_before']}" for e in es))
        md.append("")

    # ---- discoverable-conventions control
    if gen:
        md.append("## Discoverable-conventions control (evals/gen sandboxes with a committed CLAUDE.md)\n")
        gr = [r for name, st in gen.items() for r in eval_runs(st, name)]
        for name, st in gen.items():
            L = learned(st)
            md.append(f"- **{name}**: reflexes learned with the CLAUDE.md present: {', '.join(x[0] for x in L['reflexes']) or 'none'}; "
                      f"training passed {sum(1 for r in train_runs(st) if r['metrics'].get('passed'))}/{len(train_runs(st))}")
        md.append("")
        if gr:
            c = comparison(gr, "adage", "control", max_turns)
            md.append("### adage vs control, both with the documented conventions\n")
            md.append(cmp_table(c) + "\n")
            records.append(arms_record("gen-documented", "all", "suite", None, c, adage_version, sum(c["runs"].values())))

    # ---- spend
    md.append("## Spend\n")
    md.append("| Item | Sessions or calls | USD (reported `total_cost_usd`) |")
    md.append("| --- | ---: | ---: |")
    rows = spend_rows(roots, curves)
    ledger = common.cache_dir() / "spend.jsonl"
    if ledger.exists():
        by = {}
        for line in ledger.read_text().splitlines():
            x = json.loads(line)
            by.setdefault(x["kind"], [0, 0.0])
            by[x["kind"]][0] += 1
            by[x["kind"]][1] += x["usd"]
        rows += [(f"claude -p calls: {k}", n, u) for k, (n, u) in sorted(by.items())]
    ex = Path(a.extra_spend)
    if ex.exists():
        rows += [tuple(x) for x in json.loads(ex.read_text())]
    for name, n, usd in rows:
        md.append(f"| {name} | {n} | {usd:.2f} |")
    md.append(f"| **Total** | **{sum(r[1] for r in rows)}** | **{sum(r[2] for r in rows):.2f}** |")
    md.append("")

    (out / "README.md").write_text("\n".join(md) + "\n")
    with open(out / "results.jsonl", "w") as fh:
        for rec in records:
            fh.write(json.dumps(rec, sort_keys=True) + "\n")
    print(f"wrote {out / 'README.md'} and {len(records)} L3 records; total spend ${sum(r[2] for r in rows):.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
