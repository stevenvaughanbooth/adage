"""Does the harness predict live effects? Five checks against what RepoBench measured live.

  mechanism()        U11's dead-end recovery (the one sharp live measurement, 21 tasks): replay
                     prediction vs live 1.85 calls [1.17, 2.56]  (evals/replay/README.md)
  arm_deltas()       whole-session adage − control deltas per runner slice (v1, v2, Sonnet 5):
                     calls, $ and pass rate, task-paired; next to the replay's prediction
  residual()         replaying U11 over the *adage* arm, where it was delivered and followed:
                     what is left to save should be ~0
  delivery_match()   the Python U11 fires in the adage arm where the live hook delivered it
  cost_model_check() the cumulative-context cost model's session total vs the harness's
                     total_cost_usd, and its marginal $ per call vs the cross-sectional slope
  power()            what a live micro-slice of n task pairs can detect (for the correction protocol)
"""

from __future__ import annotations

import json
import statistics
from collections import defaultdict

from . import costmodel as CM
from . import stats as ST
from .policies.u11 import U11
from .replay import score
from .traces import ADAGE, Corpus

DEADENDS = ADAGE / "evals" / "launch" / "data" / "deadends-v2.jsonl"
LIVE = (1.85, 1.17, 2.56)
PY = ("click", "attrs", "marshmallow")


def _deadend_rows():
    return [json.loads(x) for x in DEADENDS.read_text().splitlines() if x.strip()]


def mechanism(corpus: Corpus) -> dict:
    """calibrate.py's statistic, from the Python port: Σ S per control session that hit the dead
    end, mean per task, mean over the 21 tasks live measured (95% bootstrap, 5000 draws, seed 7)."""
    rows = _deadend_rows()
    by = defaultdict(lambda: {"adage": [], "control": []})
    for r in rows:
        repo = r["root"].split("/")[-1]
        if repo in PY and r["arm"] in ("adage", "control") and r["u11_hits"] and r["recovery_calls"] is not None:
            by[(repo, r["task"])][r["arm"]].append(float(r["recovery_calls"]))
    live = {k: statistics.mean(v["control"]) - statistics.mean(v["adage"]) for k, v in by.items() if v["adage"] and v["control"]}
    out = {"live_tasks": len(live)}
    for mode, hs in (("post-hoc", False), ("hindsight", True), ("predictive", False)):
        pol = U11(mode, corpus.train())
        rep = score(pol, [s for s in corpus.held_out(slice=list(PY)) if s.source == "v2"], hindsight=hs)
        pred = defaultdict(list)
        for r in rep.rows:
            if r.dead_end:
                pred[(r.repo, r.task)].append(r.saved)
        pm = {k: statistics.mean(v) for k, v in pred.items() if k in live}
        m, ci, n = _boot(pm)
        out[mode] = {"tasks": n, "predicted": m, "ci95": ci, "inside_live_ci": LIVE[1] <= m <= LIVE[2]}
    lm, lci, ln = _boot(live)
    out["live"] = {"tasks": ln, "saving": lm, "ci95": lci}
    per = {}
    rep = score(U11("post-hoc"), [s for s in corpus.held_out(slice=list(PY)) if s.source == "v2"])
    pred = defaultdict(list)
    for r in rep.rows:
        if r.dead_end:
            pred[(r.repo, r.task)].append(r.saved)
    pm = {k: statistics.mean(v) for k, v in pred.items()}
    for repo in PY + ("all",):
        ks = [k for k in live if repo == "all" or k[0] == repo]
        per[repo] = {"tasks": len(ks), "live": _boot({k: live[k] for k in ks})[:2],
                     "replay": _boot({k: pm.get(k, 0.0) for k in ks})[:2]}
    out["per_repo"] = per
    return out


def _boot(d: dict, seed: int = 7, level: float = 0.95):
    import random
    vals = list(d.values())
    if not vals:
        return 0.0, [0.0, 0.0], 0
    rng = random.Random(seed)
    b = sorted(statistics.mean(rng.choice(vals) for _ in vals) for _ in range(5000))
    lo, hi = int((1 - level) / 2 * 5000) - 1, int((1 + level) / 2 * 5000) - 1
    return statistics.mean(vals), [b[lo], b[hi]], len(vals)


def _delta_row(ad, co, pred_rows, level):
    row = {"sessions": [len(ad), len(co)], "delivered_live": sum(bool(s.steered_routes) for s in ad),
           "u11_delivered_live": sum(any(r.startswith("dead-end") for r in s.steered_routes) for s in ad)}
    for metric, f in (("calls", lambda s: s.n_calls), ("usd", lambda s: s.cost_usd or 0.0),
                      ("pass", lambda s: 1.0 if s.passed else 0.0)):
        a, c = defaultdict(list), defaultdict(list)
        for s in ad:
            a[s.cluster].append(f(s))
        for s in co:
            c[s.cluster].append(f(s))
        d, ci, n = ST.paired_task_diff(a, c, level=level)
        row[metric] = {"adage": statistics.mean(f(s) for s in ad), "control": statistics.mean(f(s) for s in co),
                       "live_diff": d, "live_ci": ci, "tasks": n}
    # Prediction of adage − control: −(what the replay says the live policy saves on the control arm).
    pc, pu = defaultdict(list), defaultdict(list)
    for r in pred_rows:
        pc[r.cluster].append(-r.net_calls)
        pu[r.cluster].append(-r.net_usd)
    keys = [k for k in pc]
    row["calls"]["replay_pred"] = statistics.mean(statistics.mean(pc[k]) for k in keys) if keys else 0.0
    row["usd"]["replay_pred"] = statistics.mean(statistics.mean(pu[k]) for k in keys) if keys else 0.0
    row["pass"]["replay_pred"] = 0.0
    for m in ("calls", "usd", "pass"):
        lo, hi = row[m]["live_ci"]
        row[m]["pred_inside_live_ci"] = lo <= row[m]["replay_pred"] <= hi
        row[m]["live_ci_excludes_0"] = not (lo <= 0.0 <= hi)
    return row


def arm_deltas(corpus: Corpus, level: float = 0.90) -> dict:
    """Live adage − control per slice and source, plus pools: calls/session, $/session, pass rate
    (task-paired, bootstrap over tasks), and the replay's prediction of the same with the policy
    that was live (v2: U11 post-hoc; v1: none: U11 was not bound in v1, which delivered only 2
    advisory directives, so v1 adage-vs-control is effectively an A/A test of the design).
    The replay is consistent when its prediction lies in the live CI; the CI width is the
    smallest whole-session effect such a live A/B can see."""
    out = {}
    u11 = U11("post-hoc")
    pools = defaultdict(lambda: ([], [], []))
    for src in ("v2", "v1"):
        for sl in corpus.slices():
            ad = [s for s in corpus.held_out(slice=sl, arm="adage") if s.source == src]
            co = [s for s in corpus.held_out(slice=sl, arm="control") if s.source == src]
            if not ad or not co:
                continue
            pred = score(u11, co).rows if src == "v2" else []
            out[f"{src}:{sl}"] = _delta_row(ad, co, pred, level)
            keys = [f"{src}:all"]
            if src == "v2" and sl in PY:
                keys.append("v2:haiku-python")
            if src == "v2" and "sonnet" not in sl:
                keys.append("v2:haiku-all")
            for k in keys:
                pools[k][0].extend(ad)
                pools[k][1].extend(co)
                pools[k][2].extend(pred)
    for k, (ad, co, pred) in pools.items():
        out[f"pooled {k}"] = _delta_row(ad, co, pred, level)
    return out


def aa_coverage(deltas: dict) -> dict:
    """v1 per-slice rows are an A/A test (nothing U11 delivered): how often does a 90% task-paired
    CI exclude 0? Nominal 10%; more means the task bootstrap with 6–8 tasks under-covers."""
    rows = [(k, m) for k, v in deltas.items() if k.startswith("v1:") for m in ("calls", "usd", "pass")
            if v[m]["tasks"] >= 3 and v[m]["live_ci"] != [0.0, 0.0]]
    ex = [f"{k} {m}" for k, m in rows if deltas[k][m]["live_ci_excludes_0"]]
    return {"tests": len(rows), "excluding_0": len(ex), "rate": len(ex) / len(rows) if rows else 0.0, "which": ex}


def did(corpus: Corpus, level: float = 0.90) -> dict:
    """Whole-session difference-in-differences, v2 Haiku Python: (adage sessions that got U11 −
    control sessions the replay says would have) − (adage sessions that did not − control sessions
    that would not). Removes the arm-level shift the plain adage − control delta carries (adage
    sessions with *no* delivery also ran ~1.9 calls longer). Prediction: the replay's saving per
    firing session. Bootstrap over (slice, task) clusters."""
    import random
    ad = [s for s in corpus.held_out(slice=list(PY), arm="adage") if s.source == "v2"]
    co = [s for s in corpus.held_out(slice=list(PY), arm="control") if s.source == "v2"]
    rep = score(U11("post-hoc"), co)
    fire = {r.run: r.directives > 0 for r in rep.rows}
    pred_calls = statistics.mean(r.net_calls for r in rep.rows if r.directives)
    pred_usd = statistics.mean(r.net_usd for r in rep.rows if r.directives)
    tasks = sorted({s.cluster for s in ad + co})
    by = defaultdict(list)
    for s in ad:
        by[s.cluster].append(("ad1" if s.steered_routes else "ad0", s))
    for s in co:
        by[s.cluster].append(("co1" if fire[s.run] else "co0", s))
    out = {"sessions": {"adage_delivered": sum(bool(s.steered_routes) for s in ad), "adage_not": sum(not s.steered_routes for s in ad),
                        "control_would": sum(fire.values()), "control_would_not": sum(not v for v in fire.values())}}
    for metric, f, pred in (("calls", lambda s: s.n_calls, -pred_calls), ("usd", lambda s: s.cost_usd or 0.0, -pred_usd)):
        def stat(keys):
            g = defaultdict(list)
            for k in keys:
                for grp, s in by[k]:
                    g[grp].append(f(s))
            if any(not g[x] for x in ("ad1", "ad0", "co1", "co0")):
                return None
            m = statistics.mean
            return (m(g["ad1"]) - m(g["co1"])) - (m(g["ad0"]) - m(g["co0"]))
        obs = stat(tasks)
        rng = random.Random(7)
        boots = sorted(x for x in (stat([rng.choice(tasks) for _ in tasks]) for _ in range(2000)) if x is not None)
        a = (1 - level) / 2
        ci = [boots[int(a * len(boots))], boots[int((1 - a) * len(boots)) - 1]]
        out[metric] = {"did": obs, "ci": ci, "replay_pred_per_firing_session": pred, "pred_inside": ci[0] <= pred <= ci[1]}
    return out


def residual(corpus: Corpus) -> dict:
    """U11 replayed over the adage arm (delivered, followed live): what remains to save."""
    rep = score(U11("post-hoc"), [s for s in corpus.held_out(slice=list(PY), arm="adage") if s.source == "v2"])
    s = rep.summary()
    return {"sessions": s["sessions"], "directives": s["directives"], "useful": s["useful"],
            "s_hat_rust": s["s_hat_rust"], "ci95": s["s_hat_rust_ci95"]}


def delivery_match(corpus: Corpus) -> dict:
    """Where the live hook delivered U11 (adage arm) vs where the Python U11 fires, route and call."""
    ss = [s for s in corpus.held_out(slice=list(PY), arm="adage") if s.source == "v2"]
    rep = score(U11("post-hoc"), ss)
    same = live_n = py_n = 0
    diffs = []
    for s, r in zip(sorted(ss, key=lambda s: s.run), rep.rows):
        live = {(rt, t) for rt, t in s.delivered_at if rt.startswith("dead-end")}
        py = {(x.route, x.t) for x in r.scored}
        live_n += len(live)
        py_n += len(py)
        same += len(live & py)
        if live != py:
            diffs.append({"run": s.run, "live": sorted(live), "python": sorted(py)})
    return {"sessions": len(ss), "live_deliveries": live_n, "python_directives": py_n, "same_route_and_call": same,
            "differences": diffs[:10]}


def cost_model_check(corpus: Corpus) -> dict:
    """The model's session total vs the harness's billed total, and its marginal $ of one call vs
    the within-task cross-sectional slope of $ on calls."""
    ratios, marg = [], []
    by_task = defaultdict(list)
    for s in corpus.sessions:
        cm = CM.of(s)
        if s.cost_usd and cm.total:
            ratios.append(cm.total / s.cost_usd)
            by_task[(s.slice, s.task)].append((s.n_calls, s.cost_usd))
        if s.n_calls >= 4 and s.phase == "eval" and s.arm == "control":
            m = [cm.removal([k])[0] for k in range(0, s.n_calls, max(1, s.n_calls // 8))]
            marg.append(statistics.mean(m))
    # within-task slope: demean calls and $ per task, OLS through the origin
    xs, ys = [], []
    for v in by_task.values():
        if len(v) < 2:
            continue
        mx = statistics.mean(x for x, _ in v)
        my = statistics.mean(y for _, y in v)
        for x, y in v:
            xs.append(x - mx)
            ys.append(y - my)
    slope = sum(x * y for x, y in zip(xs, ys)) / sum(x * x for x in xs) if xs else 0.0
    ratios.sort()
    return {"sessions": len(ratios), "model_over_billed_median": statistics.median(ratios),
            "p05": ratios[len(ratios) // 20], "p95": ratios[19 * len(ratios) // 20],
            "marginal_usd_per_removed_call_model": statistics.mean(marg) if marg else 0.0,
            "marginal_usd_per_call_within_task_slope": slope,
            "note": "the slope includes the extra output and reasoning longer sessions carry; the model's removal value is "
                    "the carried context only, so it should be at or below the slope"}


def power(level: float = 0.80) -> dict:
    """Minimum detectable effect of a paired live micro-slice on the local outcome (calls from the
    trigger to Sat), from the live U11 dead-end sessions' spread."""
    rows = _deadend_rows()
    by = defaultdict(lambda: {"adage": [], "control": []})
    for r in rows:
        if r["arm"] in ("adage", "control") and r["u11_hits"] and r["recovery_calls"] is not None:
            by[(r["root"], r["task"])][r["arm"]].append(float(r["recovery_calls"]))
    # one session per arm per pair: the per-pair difference's SD from session-level spreads
    sd_c = statistics.pstdev([x for v in by.values() for x in v["control"]])
    sd_a = statistics.pstdev([x for v in by.values() for x in v["adage"]])
    sd = (sd_c ** 2 + sd_a ** 2) ** 0.5
    from statistics import NormalDist
    z = NormalDist().inv_cdf(0.95) + NormalDist().inv_cdf(level)  # one-sided 5%, power `level`
    return {"sd_control": sd_c, "sd_adage": sd_a, "sd_pair": sd,
            "mde_calls": {n: z * sd / n ** 0.5 for n in (5, 8, 10, 12, 20)},
            "note": "local outcome (recovery calls), one-sided alpha 0.05, power %.0f%%" % (100 * level)}


def run_all(corpus: Corpus) -> dict:
    deltas = arm_deltas(corpus)
    return {"mechanism": mechanism(corpus), "arm_deltas": deltas, "aa_coverage": aa_coverage(deltas), "did": did(corpus), "residual": residual(corpus),
            "delivery_match": delivery_match(corpus), "cost_model": cost_model_check(corpus), "power": power()}
