#!/usr/bin/env python3
"""Freeze a finished demo or live run into a results directory:

  index.html     the dashboard with the final state embedded (no server needed)
  artifact.html  the same page without its document skeleton, for publishing
  summary.json   per-arm statistics for the held-out runs, and what Adage learned
  state.json     the full run state (timelines, directives, metrics)

  python3 demo/report.py [ROOT] [--out DIR]

ROOT is a demo root (default /tmp/adage-demo) or a live-runner root (evals/live/runner.py). The
default output is demo/results/<stamp>-<model>/. The live runner calls build() itself and writes
<root>/results/.

The statistics (shared with the live runner) treat the task, not the run, as the unit: arm labels
are permuted within each task, confidence intervals resample whole tasks, and p-values are
Holm-adjusted across the metrics. Runs whose harness or scorer failed are left out of both arms and
counted; runs that hit max_turns are kept, counted, and capped at max_turns. See
evals/live/README.md ("Statistics").
"""

import json
import random
import re
import statistics
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent


def med(v):
    v = [x for x in v if x is not None]
    return statistics.median(v) if v else None


def avg(v):
    v = [x for x in v if x is not None]
    return sum(v) / len(v) if v else None


# ------------------------------------------------------------------------------------ run status

# Every finished run gets one. Only `ok` and `censored` runs are compared: an infrastructure or
# scorer failure says nothing about the agent, so it is left out of every arm alike, and counted.
STATUSES = ("ok", "censored", "infra_error", "scorer_error")
EXCLUDED = ("infra_error", "scorer_error")


def classify_status(result, tool_calls, turns, score, scorer_ok=True, max_turns=None):
    """(status, reason) of one finished run.

    infra_error   the harness failed: no result message, an error result other than the turn
                  limit, or a session that did nothing (no tool calls in at most one turn, and no
                  reply: one that only answered, e.g. asked what the ticket means, is `ok`, failed)
    scorer_error  the scorer crashed or printed no verdict (`passed`)
    censored      the session hit max_turns: its outcome is what it had done by then
    ok            everything else"""
    result = result or {}
    subtype = result.get("subtype")
    if not result:
        return "infra_error", "no result message from the harness"
    if subtype == "error_during_execution" or (result.get("is_error") and subtype != "error_max_turns"):
        detail = str(result.get("result") or "")[:200]
        return "infra_error", f"harness error ({subtype or 'is_error'})" + (f": {detail}" if detail else "")
    if not tool_calls and (turns or 0) <= 1:
        # A successful reply with text but no tool call is the agent's own choice (real Haiku
        # sessions answered "What does W-1043 entail?" and stopped): a failed task, not the harness's.
        if subtype == "success" and str(result.get("result") or "").strip():
            return "ok", "answered without a tool call"
        return "infra_error", f"no tool calls in {turns or 0} turn(s)"
    if not scorer_ok or not isinstance(score, dict) or "passed" not in score:
        return "scorer_error", "the scorer crashed or printed no verdict"
    if subtype == "error_max_turns" or (subtype is None and max_turns and (turns or 0) >= max_turns):
        return "censored", f"hit max_turns ({max_turns or '?'})"
    return "ok", None


def status_of(m, max_turns=None):
    """A run's recorded status or, for runs recorded before statuses existed, the one its metrics
    imply (the scorer's health was not recorded then, so those are never scorer errors)."""
    if m.get("status") in STATUSES:
        return m["status"], m.get("status_reason")
    sub = m.get("result_subtype")
    result = {"subtype": sub} if sub is not None or m.get("tool_calls") else {}
    return classify_status(result, m.get("tool_calls"), m.get("turns"), {"passed": m.get("passed")}, True, max_turns)


# --------------------------------------------------------------------------------------- metrics
#
# Each metric maps (metrics, status, max_turns) to (value, imputed). A run that never edited has no
# first edit. Rather than dropping it (which flatters whichever arm gives up more often), its value
# is restricted at what was observed: max_turns if it was censored, else the turns (or seconds) it
# ran. A censored run's total turns count as max_turns.

def _plain(key):
    return lambda m, st, mt: (m.get(key), False)


def flag(key):
    return lambda m, st, mt: (None if m.get(key) is None else (1.0 if m[key] else 0.0), False)


def _first_edit_turns(m, st, mt):
    v = m.get("turns_to_first_edit")
    if v is not None:
        return v, False
    if st == "censored" and mt:
        return mt, True
    return m.get("tool_turns", m.get("turns")), True


def _first_edit_time(m, st, mt):
    v = m.get("time_to_first_edit")
    return (v, False) if v is not None else (m.get("elapsed"), True)


def _total_turns(m, st, mt):
    return (mt if st == "censored" and mt else m.get("turns")), False


def _follow_rate(m, st, mt):
    f = m.get("follow") or {}
    return (f["followed"] / f["exposed"] if f.get("exposed") else None), False


METRICS = {
    "turns_to_first_edit": (_first_edit_turns, med),
    "time_to_first_edit_s": (_first_edit_time, med),
    "total_turns": (_total_turns, med),
    "elapsed_s": (_plain("elapsed"), med),
    "explore_calls_before_edit": (_plain("explore_calls_before_edit"), med),
    "dead_end_hits": (_plain("dead_end_hits"), avg),
    "pass_rate": (flag("passed"), avg),
    # Newer runs record these; older states (the first demo) do not, and report None.
    "hidden_pass_rate": (flag("hidden_passed"), avg),
    "obligation_met_rate": (flag("obligation_met"), avg),
    "follow_rate": (_follow_rate, avg),
    "stale_generated_rate": (flag("stale_generated"), avg),
    "cost_usd": (_plain("cost_usd"), avg),
}


# ------------------------------------------------------------------------------------ statistics
#
# The unit of analysis is the task, not the run: repeats of one task are not independent, so a
# test that shuffles runs across tasks overstates n. Arms are randomised within (task, repeat), so
# the test permutes arm labels within each task, and the confidence interval resamples tasks.

def _by_task(points, a, b):
    """[(task, arm, value)] -> [(values of a, values of b)] for the tasks that have both arms."""
    tasks = {}
    for t, arm, v in points:
        if arm in (a, b) and v is not None:
            tasks.setdefault(t, ([], []))[0 if arm == a else 1].append(v)
    return [tasks[t] for t in sorted(tasks, key=str) if tasks[t][0] and tasks[t][1]]


def _task_mean_diff(groups):
    return sum(sum(x) / len(x) - sum(y) / len(y) for x, y in groups) / len(groups)


def task_mean_diff(points, a="adage", b="holdout"):
    """Mean over tasks of (task mean of a - task mean of b), or None if no task has both arms."""
    groups = _by_task(points, a, b)
    return _task_mean_diff(groups) if groups else None


def cluster_perm_p(points, a="adage", b="holdout", n=10000, seed=7):
    """Two-sided permutation test stratified by task: arm labels are shuffled within each task (the
    randomisation unit), and the statistic is the mean over tasks of the difference in task means.
    `points` is [(task, arm, value)]. None with fewer than two tasks that have both arms."""
    groups = _by_task(points, a, b)
    if len(groups) < 2:
        return None
    obs = abs(_task_mean_diff(groups))
    rng = random.Random(seed)
    work = [(len(x), x + y) for x, y in groups]
    hits = 0
    for _ in range(n):
        for _, vals in work:
            rng.shuffle(vals)
        if abs(_task_mean_diff([(vals[:k], vals[k:]) for k, vals in work])) >= obs - 1e-12:
            hits += 1
    return (hits + 1) / (n + 1)


def task_signflip_p(points, a="adage", b="holdout", n=10000, seed=7):
    """Two-sided sign-flip test on the per-task differences in means: the task is the only unit, so
    it asks whether the effect would hold for new tasks, not only new runs of these. Exact up to 16
    tasks (its smallest p is 2 / 2^tasks), Monte Carlo above. None with fewer than two tasks."""
    d = [sum(x) / len(x) - sum(y) / len(y) for x, y in _by_task(points, a, b)]
    if len(d) < 2:
        return None
    obs = abs(sum(d)) - 1e-12
    if len(d) <= 16:
        hits = sum(1 for mask in range(1 << len(d))
                   if abs(sum(-v if mask >> i & 1 else v for i, v in enumerate(d))) >= obs)
        return hits / (1 << len(d))
    rng = random.Random(seed)
    hits = sum(1 for _ in range(n) if abs(sum(v if rng.random() < 0.5 else -v for v in d)) >= obs)
    return (hits + 1) / (n + 1)


def _quantile(sorted_v, q):
    i = q * (len(sorted_v) - 1)
    lo = int(i)
    hi = min(lo + 1, len(sorted_v) - 1)
    return sorted_v[lo] + (sorted_v[hi] - sorted_v[lo]) * (i - lo)


def cluster_boot_ci(points, agg, a="adage", b="holdout", n=2000, seed=11, level=0.95):
    """Percentile bootstrap CI for agg(a) - agg(b) that resamples whole tasks with replacement (a
    task's runs stay together). None with fewer than two tasks that have both arms."""
    groups = _by_task(points, a, b)
    if len(groups) < 2:
        return None
    rng = random.Random(seed)
    diffs = []
    for _ in range(n):
        xa, xb = [], []
        for _ in groups:
            x, y = groups[rng.randrange(len(groups))]
            xa += x
            xb += y
        diffs.append(agg(xa) - agg(xb))
    diffs.sort()
    alpha = (1 - level) / 2
    return [_quantile(diffs, alpha), _quantile(diffs, 1 - alpha)]


def holm(pvals):
    """Holm's step-down adjustment of {name: p} (None stays None). Controls the family-wise error
    rate across the metrics that are compared together."""
    ranked = sorted((p, k) for k, p in pvals.items() if p is not None)
    out = {k: None for k in pvals}
    m, running = len(ranked), 0.0
    for i, (p, k) in enumerate(ranked):
        running = max(running, min(1.0, (m - i) * p))
        out[k] = running
    return out


def compare(runs, max_turns=None, a="adage", b="holdout", n_perm=10000, n_boot=2000):
    """Per-arm statistics of held-out runs (records with `arm`, `task` and `metrics`): each arm's
    aggregate and n (with the runs that had no value and the runs whose value was restricted), and
    between arms `a` and `b` the difference of the aggregates with a task-bootstrap 95% CI, the
    task-stratified permutation p-value, and the Holm-adjusted p-value across the metrics. Runs
    with an infra or scorer error are left out of every arm and counted; censored runs stay in and
    are counted."""
    arms = sorted({r["arm"] for r in runs})
    counts = {arm: {s: 0 for s in STATUSES} for arm in arms}
    kept = []
    for r in runs:
        st, _ = status_of(r["metrics"], max_turns)
        counts[r["arm"]][st] += 1
        if st not in EXCLUDED:
            kept.append((r, st))
    rows = {}
    for name, (f, agg) in METRICS.items():
        vals = [(r.get("task"), r["arm"], *f(r["metrics"], st, max_turns)) for r, st in kept]
        row = {"aggregate": agg.__name__}
        for arm in arms:
            row[arm] = agg([v for _, x, v, _ in vals if x == arm])
        row["n"] = {arm: sum(1 for _, x, v, _ in vals if x == arm and v is not None) for arm in arms}
        row["n_missing"] = {arm: sum(1 for _, x, v, _ in vals if x == arm and v is None) for arm in arms}
        row["n_imputed"] = {arm: sum(1 for _, x, v, imp in vals if x == arm and v is not None and imp) for arm in arms}
        if a in arms and b in arms:
            pts = [(t, x, v) for t, x, v, _ in vals if v is not None]
            row["diff"] = None if row[a] is None or row[b] is None else row[a] - row[b]
            row["ci95"] = cluster_boot_ci(pts, agg, a, b, n_boot)
            row["n_tasks"] = len(_by_task(pts, a, b))
            row["task_mean_diff"] = task_mean_diff(pts, a, b)
            row["p_value"] = cluster_perm_p(pts, a, b, n_perm)
            row["p_tasks"] = task_signflip_p(pts, a, b)
        rows[name] = row
    for name, p in holm({k: r["p_value"] for k, r in rows.items() if "p_value" in r}).items():
        rows[name]["p_holm"] = p
    return {
        "status_counts": counts,
        "excluded": {arm: sum(counts[arm][s] for s in EXCLUDED) for arm in arms},
        "n_censored": {arm: counts[arm]["censored"] for arm in arms},
        "metrics": rows,
    }


def fmt(x, digits=3):
    return "-" if x is None else f"{x:.{digits}g}" if isinstance(x, float) else str(x)


def table(cmp, a="adage", b="holdout"):
    """compare()'s result as a plain-text table, with the Holm-adjusted p-values."""
    lines = [f"{'metric':<26} {a:>9} {b:>9} {'diff':>9} {'95% CI (tasks)':>20} {'n':>7} {'tasks':>5} "
             f"{'p':>7} {'p_holm':>7} {'p_tasks':>7}"]
    for name, r in cmp["metrics"].items():
        ci, n = r.get("ci95"), r.get("n", {})
        lines.append(f"{name:<26} {fmt(r.get(a)):>9} {fmt(r.get(b)):>9} {fmt(r.get('diff')):>9} "
                     f"{('[' + fmt(ci[0]) + ', ' + fmt(ci[1]) + ']') if ci else '-':>20} "
                     f"{str(n.get(a, 0)) + '/' + str(n.get(b, 0)):>7} {fmt(r.get('n_tasks')):>5} "
                     f"{fmt(r.get('p_value')):>7} {fmt(r.get('p_holm')):>7} {fmt(r.get('p_tasks')):>7}")
    lines.append(f"left out (infra/scorer errors): {cmp['excluded']}  censored (hit max_turns): {cmp['n_censored']}")
    return "\n".join(lines)


def stats_for(runs, max_turns=None):
    """compare() in this page's summary format (without_adage / with_adage; n as [adage, holdout])."""
    cmp = compare(runs, max_turns)
    pair = lambda d: [d.get("adage", 0), d.get("holdout", 0)]  # noqa: E731
    out = {}
    for name, r in cmp["metrics"].items():
        out[name] = {"without_adage": r.get("holdout"), "with_adage": r.get("adage"), "aggregate": r["aggregate"],
                     "diff": r.get("diff"), "ci95": r.get("ci95"), "n_tasks": r.get("n_tasks"),
                     "p_value": r.get("p_value"), "p_holm": r.get("p_holm"), "p_tasks": r.get("p_tasks"),
                     "n": pair(r["n"]), "n_missing": pair(r["n_missing"]), "n_imputed": pair(r["n_imputed"])}
    return out, {k: cmp[k] for k in ("status_counts", "excluded", "n_censored")}


def summarise(state):
    runs = [r for r in state["runs"].values() if r.get("metrics")]
    all_ev = [r for r in runs if r["phase"] == "eval" and r.get("arm") in ("holdout", "adage")]
    rounds = sorted({r.get("round", 1) for r in all_ev})
    latest = rounds[-1] if rounds else 1
    ev = [r for r in all_ev if r.get("round", 1) == latest]
    max_turns = state.get("max_turns")
    summary = {
        "config": state.get("config"),
        "model": state.get("model"),
        "harness_version": state.get("harness_version"),
        "isolated": state.get("isolated"),
        "seed": state.get("seed"),
        # States from before the flag existed learned later rounds from every earlier session.
        "leaky": bool(state.get("leaky", latest > 1)),
        "generated": time.strftime("%Y-%m-%dT%H:%M:%S"),
        "training_runs": sum(1 for r in runs if r["phase"] == "train"),
        "training_pass_rate": avg([1.0 if r["metrics"].get("passed") else 0.0 for r in runs if r["phase"] == "train"
                                   and status_of(r["metrics"], max_turns)[0] not in EXCLUDED]),
        "eval_runs": {a: sum(1 for r in ev if r["arm"] == a) for a in ("holdout", "adage")},
        "learned": state.get("learn"),
        "routes": [
            {"track": r["track"], "id": r["route"]["id"], "aspect": r["route"]["aspect"], "effect": r["route"]["effect"],
             "trigger": r["route"]["trigger"], "step": r["route"]["step"], "evidence": r["route"]["evidence"]}
            for r in state.get("tracks", [])
        ],
    }
    summary["latest_round"] = latest
    summary["metrics"], summary["runs_by_status"] = stats_for(ev, max_turns)
    summary["rounds"] = {}
    for rd in rounds:
        rr = [r for r in all_ev if r.get("round", 1) == rd]
        info = (state.get("rounds") or {}).get(str(rd), {})
        metrics, by_status = stats_for(rr, max_turns)
        summary["rounds"][str(rd)] = {
            "metrics": metrics, "runs_by_status": by_status,
            "leaky": bool(info.get("leaky", rd > 1 and "leaky" not in state)),
            "routes": [x["route"]["id"] + ":" + x["route"]["aspect"] for x in (info.get("tracks") or [])],
        }
    followed = sum(1 for r in ev if r["arm"] == "adage" for d in r["metrics"].get("adage", []) if d.get("kind") == "followed")
    emitted = sum(1 for r in ev if r["arm"] == "adage" for d in r["metrics"].get("adage", []) if d.get("kind") == "directives")
    summary["directives"] = {"emitted": emitted, "followed": followed,
                             "stop_requests": sum(1 for r in ev for d in r["metrics"].get("adage", []) if d.get("kind") == "stop_request")}
    return summary


def redact_home(state, home=None):
    """The state with the person's home directory shown as `~`: results pages are meant to be
    shared, and agents' commands sometimes name paths under it (one listed ~/.claude/projects)."""
    home = str(home or Path.home())
    if not home or home == "/":
        return state
    return json.loads(json.dumps(state).replace(home, "~"))


def build(state, out):
    """Write the results directory for `state` into `out`; return `out`. The home directory is
    redacted from everything written."""
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    state = redact_home(state)
    summary = summarise(state)
    (out / "summary.json").write_text(json.dumps(summary, indent=2))
    (out / "state.json").write_text(json.dumps(state))
    page = (HERE / "dashboard" / "index.html").read_text()
    embed = "<script>window.__STATE__ = " + json.dumps(state).replace("</", "<\\/") + ";</script>\n<script>"
    page = page.replace("<script>", embed, 1).replace("live proving run", "proving run results", 1)
    labels = state.get("labels") or {}
    if labels.get("regen"):
        page = page.replace("</i>make gen</span>", f"</i>{labels['regen']}</span>", 1)
    if labels.get("lookup"):
        page = page.replace("</i>work-item lookup</span>", f"</i>{labels['lookup']}</span>", 1)
    (out / "index.html").write_text(page)
    # Artifact variant: the publisher supplies its own document skeleton.
    art = re.sub(r"<!doctype html>\s*<html[^>]*>\s*<head>\s*", "", page, flags=re.I)
    art = re.sub(r'<meta charset="utf-8">\s*<meta name="viewport"[^>]*>\s*', "", art)
    art = art.replace("</head>\n<body>", "").replace("</body>\n</html>", "")
    (out / "artifact.html").write_text(art)
    return out


def main(argv):
    args = list(argv)
    out = None
    if "--out" in args:
        i = args.index("--out")
        out = Path(args[i + 1])
        del args[i:i + 2]
    root = Path(args[0] if args else "/tmp/adage-demo")
    state = json.loads((root / "state.json").read_text())
    if out is None:
        model = (state.get("model") or "model").replace("/", "-")
        out = HERE / "results" / f"{time.strftime('%Y%m%d-%H%M')}-{model}"
    build(state, out)
    ev = [r for r in state["runs"].values() if r.get("metrics") and r["phase"] == "eval"]
    latest = max((r.get("round", 1) for r in ev), default=1)
    print(table(compare([r for r in ev if r.get("round", 1) == latest], state.get("max_turns"))))
    if state.get("leaky", latest > 1):
        print("\nWARNING: later rounds learned from earlier sessions of the same held-out tasks (leaky: true); "
              "their comparison is biased toward Adage.")
    print(f"\nwrote {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
