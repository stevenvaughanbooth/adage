"""Model economics: is Adage's value proposition "make the cheap model good enough"?

Offline, $0. Uses the common harness loader (`harness.load`) and its cumulative-context cost model
(`harness.costmodel`), which reads the cache TTL each response actually paid from its usage record.

    cd evals/lab/research && OMP_NUM_THREADS=1 nice -n 10 python3 -m model-economics.analysis
    (or: python3 model-economics/analysis.py)

Writes model-economics/results.json. Everything is on the RepoBench tasks where both Haiku 4.5 and
Sonnet 5 sessions exist: click and vue-core held-out test tasks, control arm (directives withheld).

HAND-CODED PARTS (measurement instruments only, declared): the failure taxonomy in `classify()`
(reads the scorer's `state.json`: changed files, whether the run hit the turn cap, whether it touched
the gold file, the hidden-test tail), and the harness's hand-written Sat/W features used by U11 and
the oracle ceilings. Nothing here is a learner.
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from harness import load, score  # noqa: E402
from harness import costmodel as CM  # noqa: E402
from harness import stats as ST  # noqa: E402
from harness.oracle import CLASSES, ceiling  # noqa: E402
from harness.policies.u11 import U11  # noqa: E402
from harness.traces import V1, V2  # noqa: E402

REPOS = ["click", "vue-core"]
DRAWS = 5000
SEED = 20260929
LEVEL = 0.90

# --------------------------------------------------------------------------------------------------
# Prices. Checked against the claude-api reference (cached 2026-09-25) and pricing.toml (as_of
# 2026-06-24): Haiku 4.5 $1 in / $5 out; Sonnet 5 $2 / $10 (Sonnet 5.5, the current Sonnet, is the
# same price). Cache reads 0.1x input; cache writes 1.25x input (5-minute TTL), 2x (1-hour TTL).
PRICES = {
    "claude-haiku-4-5": {"input": 1.0, "output": 5.0, "cache_read": 0.10, "cache_write_5m": 1.25, "cache_write_1h": 2.0},
    "claude-sonnet-5": {"input": 2.0, "output": 10.0, "cache_read": 0.20, "cache_write_5m": 2.50, "cache_write_1h": 4.0},
}


def cost(s, ttl: str = "actual") -> float:
    """Session $ under the cumulative-context model. ttl: 'actual' (what the usage record says was
    written: here ~all 1h), '5m' (every write at 1.25x input), '1h' (every write at 2x input)."""
    cm = CM.of(s)
    if ttl == "actual":
        return cm.total
    tot = 0.0
    for r, rt in zip(s.responses, cm.rate):
        w = rt[2] if ttl == "5m" else 2 * rt[0]
        tot += r.cache_read * rt[3] + r.cache_write * w + r.input * rt[0] + r.output * rt[1]
    return tot


def mkey(s) -> str:
    return "haiku" if "haiku" in s.model else "sonnet"


# --------------------------------------------------------------------------------------------------
# Scorer state (for the failure taxonomy)
def load_state():
    st = {}
    for src, root in (("v2", V2), ("v1", V1)):
        for sl in ("click", "vue-core", "click-sonnet5", "vue-core-sonnet5"):
            p = root / sl / "state.json"
            if p.exists():
                for k, v in json.loads(p.read_text())["runs"].items():
                    st[(src, sl, k)] = v
    return st


def test_counts(tail: str):
    f = re.search(r"(\d+) failed", tail or "")
    p = re.search(r"(\d+) passed", tail or "")
    return (int(f.group(1)) if f else None, int(p.group(1)) if p else None)


def classify(s, v) -> dict:
    """Instrument (hand-coded, declared): why did a failed session fail?"""
    m, sc = v.get("metrics") or {}, v.get("score") or {}
    changed = [f for f in (sc.get("changed_files") or []) if not re.search(r"(CHANGE|\.md$|\.rst$|\.txt$)", f, re.I)]
    capped = m.get("result_subtype") == "error_max_turns"
    tail = sc.get("tail") or ""
    nf, npass = test_counts(tail)
    collect_err = bool(re.search(r"error(s)? during collection|ImportError|SyntaxError|ModuleNotFoundError", tail)) or (sc.get("n_tests") == 0)
    if not changed:
        cat = "no_source_change"
    elif collect_err:
        cat = "breaks_build_or_import"
    elif capped:
        cat = "turn_cap_with_edit"
    else:
        cat = "wrong_fix_finished"  # the agent stopped on its own, believing it was done
    return {"cat": cat, "touched_gold": bool(sc.get("touched_gold")), "capped": capped,
            "tests_failed": nf, "tests_passed": npass, "n_tests": sc.get("n_tests")}


# --------------------------------------------------------------------------------------------------
# Bootstrap over tasks (paired: both models are resampled on the same drawn tasks)
def paired_boot(by_task: dict, fn, draws=DRAWS, seed=SEED):
    """by_task: {task: {"haiku": [(usd, calls, passed)], "sonnet": [...]}}. fn(list of task entries) -> dict of stats.
    Returns point, and per-stat [lo, hi] (90%)."""
    tasks = sorted(by_task)
    point = fn([by_task[t] for t in tasks])
    rng = np.random.default_rng(seed)
    boots = defaultdict(list)
    for _ in range(draws):
        draw = [by_task[tasks[i]] for i in rng.integers(0, len(tasks), len(tasks))]
        for k, v in fn(draw).items():
            boots[k].append(v)
    a = (1 - LEVEL) / 2
    out = {}
    for k, v in point.items():
        arr = np.array(boots[k], dtype=float)
        arr = arr[np.isfinite(arr)]
        out[k] = {"value": v, "ci90": [float(np.quantile(arr, a)), float(np.quantile(arr, 1 - a))],
                  "finite_draws": int(len(arr))}
    return out


def stats_fn(scale=None, dp=0.0):
    """scale: {"haiku": f, "sonnet": g} multiplies each model's $ (a lever's share removed); dp: pass-rate
    points added to Haiku (a pass-rate lever). Task-balanced: per-task means, then mean over tasks."""
    scale = scale or {}

    def fn(entries):
        r = {}
        for m in ("haiku", "sonnet"):
            usd = np.mean([np.mean([x[0] for x in e[m]]) for e in entries]) * scale.get(m, 1.0)
            calls = np.mean([np.mean([x[1] for x in e[m]]) for e in entries])
            pr = np.mean([np.mean([x[2] for x in e[m]]) for e in entries]) + (dp if m == "haiku" else 0.0)
            r[f"{m}_usd"] = usd
            r[f"{m}_calls"] = calls
            r[f"{m}_pass"] = pr
            r[f"{m}_usd_per_resolved"] = usd / pr if pr > 0 else float("inf")
            # session-weighted (pooled) ratio estimator as a check
            su = sum(x[0] for e in entries for x in e[m]) * scale.get(m, 1.0)
            sp = sum(x[2] for e in entries for x in e[m]) + (dp * sum(len(e[m]) for e in entries) if m == "haiku" else 0)
            r[f"{m}_usd_per_resolved_pooled"] = su / sp if sp > 0 else float("inf")
        r["sonnet_over_haiku_usd"] = r["sonnet_usd"] / r["haiku_usd"]
        r["pass_gap_pts"] = 100 * (r["sonnet_pass"] - r["haiku_pass"])
        r["sonnet_over_haiku_usd_per_resolved"] = (r["sonnet_usd_per_resolved"] / r["haiku_usd_per_resolved"]
                                                   if np.isfinite(r["haiku_usd_per_resolved"]) and np.isfinite(r["sonnet_usd_per_resolved"])
                                                   else float("nan"))
        r["haiku_saving_vs_sonnet_per_resolved"] = r["sonnet_usd_per_resolved"] - r["haiku_usd_per_resolved"]
        # the Haiku pass rate at which Haiku's $/resolved equals Sonnet's
        r["breakeven_haiku_pass"] = r["sonnet_pass"] * r["haiku_usd"] / r["sonnet_usd"]
        return r
    return fn


def per_model_cluster(sessions, f):
    m, ci = ST.clustered([f(s) for s in sessions], [s.task for s in sessions], level=LEVEL)
    return {"value": m, "ci90": ci, "n": len(sessions)}


def main():
    corpus = load()
    state = load_state()
    out = {"prices_usd_per_mtok": PRICES,
           "pricing_check": "claude-api reference (cached 2026-09-25): Haiku 4.5 $1/$5, Sonnet 5 $2/$10 (Sonnet 5.5 same); "
                            "cache write 1.25x (5m) / 2x (1h), read 0.1x. Matches crates/adage-core/src/pricing.toml "
                            "(which carries only the 5m write rate); the harness bills the 1h writes the usage records show."}

    # ---------- the matched sets
    sets = {}
    for src in ("v2", "v1"):
        S = corpus.held_out(repo=REPOS, source=src)
        sets[src] = {m: [s for s in S if mkey(s) == m] for m in ("haiku", "sonnet")}
    # sensitivity: v2, all eval arms (adage/agents-md arms moved pass rate ~0 in success-uplift)
    Sall = [s for s in corpus.select(repo=REPOS, phase="eval", split="test", source="v2")]
    sets["v2_all_arms"] = {m: [s for s in Sall if mkey(s) == m] for m in ("haiku", "sonnet")}

    # ---------- 1h vs actual TTL check
    ttl_share = {}
    for m in ("haiku", "sonnet"):
        ss = sets["v2"][m]
        w = sum(r.cache_write for s in ss for r in s.responses)
        w1 = sum(r.cache_write_1h for s in ss for r in s.responses)
        billed = sum((s.cost_usd or 0) for s in ss)
        ttl_share[m] = {"cache_write_tokens_1h_share": w1 / w if w else None,
                        "model_total_over_billed": sum(cost(s) for s in ss) / billed if billed else None}
    out["ttl_check_v2"] = ttl_share

    # ---------- Q1: $, calls, pass rate, $/resolved per model
    q1 = {}
    for name, ms in sets.items():
        block = {}
        for m, ss in ms.items():
            k = sum(bool(s.passed) for s in ss)
            p, lo, hi = ST.wilson(k, len(ss), LEVEL)
            block[m] = {
                "sessions": len(ss), "tasks": len({s.task for s in ss}), "passed": k,
                "usd_per_task_1h_actual": per_model_cluster(ss, lambda s: cost(s)),
                "usd_per_task_5m": per_model_cluster(ss, lambda s: cost(s, "5m")),
                "calls_per_task": per_model_cluster(ss, lambda s: s.n_calls),
                "pass_rate_cluster": per_model_cluster(ss, lambda s: float(bool(s.passed))),
                "pass_rate_wilson": [p, lo, hi],
            }
            for ttl in ("actual", "5m"):
                num, den = [cost(s, ttl) for s in ss], [float(bool(s.passed)) for s in ss]
                v, ci = ST.clustered_ratio(num, den, [s.task for s in ss], level=LEVEL)
                block[m][f"usd_per_resolved_pooled_{ttl}"] = {"value": v, "ci90": ci}
        # paired, task-balanced
        for ttl in ("actual", "5m"):
            bt = defaultdict(lambda: {"haiku": [], "sonnet": []})
            for m, ss in ms.items():
                for s in ss:
                    bt[s.task][m].append((cost(s, ttl), s.n_calls, float(bool(s.passed))))
            bt = {t: e for t, e in bt.items() if e["haiku"] and e["sonnet"]}
            block[f"paired_task_balanced_{ttl}"] = paired_boot(bt, stats_fn())
            block[f"paired_task_balanced_{ttl}"]["n_tasks"] = len(bt)
        q1[name] = block
    out["q1_per_model"] = q1

    # ---------- Q2: procedural waste per model (instrument ceilings) + the measured U11 lever on these sessions
    q2 = {"oracle_ceilings": {}, "u11": {}}
    for m in ("haiku", "sonnet"):
        ss = sets["v2"][m]
        q2["oracle_ceilings"][m] = {name: ceiling(ss, p) for name, p in CLASSES.items()}
        # failed ∪ friction: all procedural failure-and-recovery calls
        q2["oracle_ceilings"][m]["failed or friction (union)"] = ceiling(ss, lambda a, c: a.failed or a.env_friction)
    per_session_u11 = {}
    for mode in ("post-hoc", "predictive"):
        for pf in (1.0, 0.5):
            for m in ("haiku", "sonnet"):
                pol = U11(mode, train_sessions=corpus.train(repo=REPOS)) if mode == "predictive" else U11(mode)
                rep = score(pol, sets["v2"][m], name=f"U11 {mode} {m}", p_follow=pf)
                s = rep.summary()
                q2["u11"][f"{mode}_p{pf}_{m}"] = {k: s[k] for k in ("sessions", "tasks", "test_set_id", "baseline",
                                                                     "usd_saved_per_task", "usd_saved_ci", "usd_saved_share",
                                                                     "usd_saved_share_ci", "calls_saved_per_task",
                                                                     "calls_saved_ci", "directives", "useful")}
                if mode == "predictive" and pf == 1.0:
                    for r in rep.rows:
                        per_session_u11[r.sid] = r.net_usd

    # ---------- lever stack on Haiku (shares of $; cited from the other sprint results, per repository where given)
    trim_loro = {"click": 0.10, "vue-core": 0.063}  # waste-anatomy leave-one-repo-out, learned output budget (Haiku)
    stacks = {
        # name: (description, per-repo trimming share, other shares (multiplicative), use measured U11)
        "conservative": ("U11 predictive (measured here, p_follow 0.5) + output shaping at token-cost's central "
                         "estimate (+1%)", {"click": 0.01, "vue-core": 0.01}, [], 0.5),
        "central": ("U11 predictive (measured, p_follow 1) + half of waste-anatomy's learned output cap (per repo) + "
                    "file-pointer hint at p_follow 0.5 (context-prefetch +1.9%) + half the commit habit (1.5%)",
                    {k: v / 2 for k, v in trim_loro.items()}, [0.019, 0.015], 1.0),
        "optimistic": ("U11 predictive (measured, p_follow 1) + waste-anatomy's learned output cap in full (click 10%, "
                       "vue-core 6.3%, LORO) + file-pointer hint at p_follow 1 (+4.0%) + commit habit (3%)",
                       trim_loro, [0.040, 0.03], 1.0),
        "procedural_oracle": ("every failed or environment-friction call deleted (oracle, not a lever)", None, [], None),
    }

    def haiku_adage_cost(s, stack, ttl):
        base = cost(s, ttl)
        desc, trim, others, u11pf = stacks[stack]
        if trim is None:
            return None
        u = per_session_u11.get(s.sid, 0.0) * (u11pf or 0.0) * (base / cost(s))  # U11's $ scaled to the TTL
        c = base - u
        c *= (1 - trim.get(s.repo, 0.0))
        for f in others:
            c *= (1 - f)
        return c

    # oracle: failed-or-friction calls removed, per session
    orc = {}
    for s in sets["v2"]["haiku"]:
        cm = CM.of(s)
        idx = [k for k, a in enumerate(s.acts) if a.failed or a.env_friction]
        orc[s.sid] = cm.removal(idx)[0] / cm.total  # share

    q4 = {}
    for ttl in ("actual", "5m"):
        rows = {}
        for stack in stacks:
            bt = defaultdict(lambda: {"haiku": [], "sonnet": []})
            for s in sets["v2"]["haiku"]:
                c = (cost(s, ttl) * (1 - orc[s.sid])) if stack == "procedural_oracle" else haiku_adage_cost(s, stack, ttl)
                bt[s.task]["haiku"].append((c, s.n_calls, float(bool(s.passed))))
            for s in sets["v2"]["sonnet"]:
                bt[s.task]["sonnet"].append((cost(s, ttl), s.n_calls, float(bool(s.passed))))
            r = paired_boot(dict(bt), stats_fn())
            base_h = q1["v2"][f"paired_task_balanced_{ttl}"]["haiku_usd"]["value"]
            r["haiku_usd_share_removed"] = 1 - r["haiku_usd"]["value"] / base_h
            r["description"] = stacks[stack][0]
            rows[stack] = r
        # pass-rate sensitivity: an (unmeasured) +3.3-point pass-rate gain, success-uplift's CI upper bound
        bt = defaultdict(lambda: {"haiku": [], "sonnet": []})
        for s in sets["v2"]["haiku"]:
            bt[s.task]["haiku"].append((haiku_adage_cost(s, "optimistic", ttl), s.n_calls, float(bool(s.passed))))
        for s in sets["v2"]["sonnet"]:
            bt[s.task]["sonnet"].append((cost(s, ttl), s.n_calls, float(bool(s.passed))))
        rows["optimistic_plus_3.3pts"] = paired_boot(dict(bt), stats_fn(dp=0.033))
        q4[ttl] = rows
    # Sonnet + Adage (for symmetry): U11 predictive measured on Sonnet only; trimming ~0 on Sonnet (waste-anatomy)
    out["q2_waste"] = q2
    out["q4_routing"] = q4

    # ---------- Q3: failure taxonomy (Haiku and Sonnet), v2 control, v1 control, v2 all arms
    q3 = {}
    for name, ms in sets.items():
        src = "v1" if name == "v1" else "v2"
        blk = {}
        for m, ss in ms.items():
            cats = defaultdict(int)
            gold = defaultdict(int)
            partial = []
            waste_failed, waste_passed = [], []
            for s in ss:
                v = state.get((src, s.slice, s.run))
                cm = CM.of(s)
                share = cm.removal([k for k, a in enumerate(s.acts) if a.failed or a.env_friction])[0] / cm.total
                (waste_passed if s.passed else waste_failed).append(share)
                if s.passed or v is None:
                    continue
                c = classify(s, v)
                cats[c["cat"]] += 1
                gold[(c["cat"], c["touched_gold"])] += 1
                if c["tests_failed"] is not None and c["tests_passed"] is not None:
                    partial.append(c["tests_failed"] / (c["tests_failed"] + c["tests_passed"]))
            nf = sum(cats.values())
            blk[m] = {"failed_sessions": nf, "categories": dict(cats),
                      "category_shares": {k: v / nf for k, v in cats.items()} if nf else {},
                      "touched_gold_by_category": {f"{k[0]}|gold={k[1]}": v for k, v in gold.items()},
                      "median_hidden_test_fail_fraction": float(np.median(partial)) if partial else None,
                      "procedural_waste_share_of_usd": {"failed_sessions": float(np.mean(waste_failed)) if waste_failed else None,
                                                        "passed_sessions": float(np.mean(waste_passed)) if waste_passed else None}}
        q3[name] = blk
    # task-level: where does Haiku fail but Sonnet pass (same task)?
    tl = []
    for t in sorted({s.task for s in sets["v2"]["haiku"]}):
        h = [s for s in sets["v2"]["haiku"] if s.task == t]
        so = [s for s in sets["v2"]["sonnet"] if s.task == t]
        h1 = [s for s in sets["v1"]["haiku"] if s.task == t]
        s1 = [s for s in sets["v1"]["sonnet"] if s.task == t]
        tl.append({"task": t, "haiku_v2": f"{sum(bool(x.passed) for x in h)}/{len(h)}",
                   "sonnet_v2": f"{sum(bool(x.passed) for x in so)}/{len(so)}",
                   "haiku_v1": f"{sum(bool(x.passed) for x in h1)}/{len(h1)}",
                   "sonnet_v1": f"{sum(bool(x.passed) for x in s1)}/{len(s1)}"})
    q3["per_task"] = tl
    out["q3_failures"] = q3

    # ---------- turn-cap natural experiment: v1 (30 turns) -> v2 (60 turns), Haiku, same tasks
    out["turn_cap_natural_experiment"] = {
        m: {"v1_cap30": q1["v1"][m]["pass_rate_wilson"], "v2_cap60": q1["v2"][m]["pass_rate_wilson"],
            "v1_calls": q1["v1"][m]["calls_per_task"]["value"], "v2_calls": q1["v2"][m]["calls_per_task"]["value"]}
        for m in ("haiku", "sonnet")}

    (HERE / "results.json").write_text(json.dumps(out, indent=1, default=float))
    return out


if __name__ == "__main__":
    o = main()
    b = o["q1_per_model"]["v2"]
    for m in ("haiku", "sonnet"):
        x = b[m]
        print(m, x["sessions"], x["tasks"], "usd", x["usd_per_task_1h_actual"], "5m", x["usd_per_task_5m"]["value"],
              "calls", x["calls_per_task"], "pass", x["pass_rate_wilson"], "usd/res", x["usd_per_resolved_pooled_actual"])
    for k, v in b["paired_task_balanced_actual"].items():
        print(" ", k, v)
