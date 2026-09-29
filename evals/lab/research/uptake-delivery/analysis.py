"""Uptake and delivery: p_follow, behaviour change vs the matched control, the literal judge's error,
the realised-vs-offered gap, and off-policy estimates of alternative delivery policies.

Run `judges_eval.py` first (it fits the learned judge's threshold leave-one-repo-out and caches the
unit profiles). Writes results/analysis.json; README.md quotes it.

Measurement instruments that are hand-written (never shipped, stated in the README):
  * `judge.instrument_match` scores the learned follow judge;
  * `gtest` (a test run that ran tests: `N passed/failed`, `collected N items`, runner summaries) is
    the local goal event, as in the outcome-optimisation report's G_test. A learned goal variant is
    reported as a sensitivity.
"""
from __future__ import annotations

import collections
import json
import math
import pickle
import re
from pathlib import Path

import numpy as np

import data
import judge
import value
from judges_eval import CACHE, PRE
from units import action_text

HERE = Path(__file__).resolve().parent
RNG = np.random.default_rng(20260929)
B = 2000

# ------------------------------------------------------------------ instruments & stats -------

_GT = re.compile(r"(\d+ (passed|failed|errors?)\b)|collected [1-9]\d* items?|Tests:\s+\d|^ok\s+\S+\s+[\d.]+s|test result:",
                 re.M)


def gtest(c) -> bool:
    """INSTRUMENT (hand-written): the call ran tests (a failing test run counts)."""
    o = c.out or ""
    return c.name == "Bash" and bool(_GT.search(o)) and "no tests ran" not in o


def wilson(k, n, z=1.645):
    if n == 0:
        return [None, None]
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return [round(c - h, 3), round(c + h, 3)]


def cluster_boot(groups: list, stat, b=B):
    """90% percentile bootstrap resampling clusters (tasks). `groups`: list of per-cluster payloads."""
    vals = []
    n = len(groups)
    for _ in range(b):
        idx = RNG.integers(0, n, n)
        v = stat([groups[i] for i in idx])
        if v is not None and not (isinstance(v, float) and math.isnan(v)):
            vals.append(v)
    if not vals:
        return [None, None]
    return [round(float(np.percentile(vals, 5)), 3), round(float(np.percentile(vals, 95)), 3)]


def diff_by_task(rows, key):
    """rows: dicts with task, treated, key. Returns (mean_t - mean_c, CI) clustering by task."""
    by = collections.defaultdict(lambda: ([], []))
    for r in rows:
        by[r["task"]][0 if r["treated"] else 1].append(r[key])
    groups = list(by.values())

    def stat(gs):
        t = [x for g in gs for x in g[0]]
        c = [x for g in gs for x in g[1]]
        if not t or not c:
            return None
        return float(np.mean(t) - np.mean(c))
    t = [r[key] for r in rows if r["treated"]]
    c = [r[key] for r in rows if not r["treated"]]
    if not t or not c:
        return None, [None, None], len(t), len(c)
    return round(float(np.mean(t) - np.mean(c)), 3), cluster_boot(groups, stat), len(t), len(c)


# ------------------------------------------------------------------ follow judgement ----------

OBLIGATION_KINDS = {"run", "obligation", "sequence", "stop"}


def primary_window(u):
    """Where a follow can happen: 5 calls after a post-failure avoid (the need is now); the rest of
    the session for pre-emptive and after-edit obligations (they are due before finishing)."""
    if u.hook in PRE or u.kind in OBLIGATION_KINDS:
        return None
    return 5


def judged(U, P, taus):
    rows = []
    for u, p in zip(U, P):
        tau = taus.get(u.s.repo, 0.86)
        prof_all = p["containment"] if (u.hook in PRE) else None
        texts, calls = judge.lags(u, None)
        con_all = CON.sims(u.directed, texts) if prof_all is None else prof_all
        lag_l = judge.learned_lag(con_all, tau)
        lag_i = judge.instrument_lag(u, None)
        w = primary_window(u)
        f_l = lag_l is not None and (w is None or lag_l < w)
        f_i = lag_i is not None and (w is None or lag_i < w)
        wording = u.text
        rows.append({
            "sid": u.s.sid, "task": u.s.task, "repo": u.s.repo, "pool": u.s.pool, "model": u.s.model,
            "arm": u.s.arm, "treated": u.treated, "family": u.family, "kind": u.kind, "hook": u.hook,
            "route": u.route, "pos": u.pos, "n_calls": len(u.s.calls),
            "words": len(wording.split()), "explains": int(bool(re.search(r"\bfails?\b|\(|Agents in this", wording))),
            "must": int("must" in wording), "timing": "pre-emptive" if u.hook in PRE else "post-hoc",
            "lag_learned": lag_l, "lag_instr": lag_i,
            "follow": int(f_l), "follow_instr": int(f_i), "follow5": int(lag_l is not None and lag_l < 5),
            "literal": 1 if u.literal == 1 else 0, "literal_logged": u.literal is not None,
        })
    return rows


def rate_table(rows, keys, follow="follow"):
    out = []
    grp = collections.defaultdict(list)
    for r in rows:
        grp[tuple(r[k] for k in keys)].append(r)
    for k, rs in sorted(grp.items(), key=str):
        t = [r for r in rs if r["treated"]]
        c = [r for r in rs if not r["treated"]]
        kt, kc = sum(r[follow] for r in t), sum(r[follow] for r in c)
        d, ci, _, _ = diff_by_task(rs, follow) if t and c else (None, [None, None], 0, 0)
        out.append({**dict(zip(keys, k)), "n_delivered": len(t), "p_follow": round(kt / len(t), 3) if t else None,
                    "p_follow_ci": wilson(kt, len(t)), "n_withheld": len(c),
                    "p_self": round(kc / len(c), 3) if c else None, "p_self_ci": wilson(kc, len(c)),
                    "uplift": d, "uplift_ci": ci,
                    "literal_p_follow": round(sum(r["literal"] for r in t) / len(t), 3) if t else None})
    return out


# ------------------------------------------------------------------ local value (dead end) ----

def goal_after(s, t, horizon=value.H):
    for i in range(t + 1, min(len(s.calls), t + 1 + horizon)):
        if gtest(s.calls[i]):
            return i
    return None


def first_need(s, trig, con):
    """Earliest call at or before the trigger with the trigger's intent and no test run in its
    output since the last goal: where a pre-emptive directive would have acted."""
    tt = action_text(s.calls[trig], s.cwd)
    j = trig
    for i in range(trig - 1, -1, -1):
        c = s.calls[i]
        if gtest(c):
            break
        if c.name == "Bash" and con.one(tt, action_text(c, s.cwd)) >= value.TAU_INTENT:
            j = i
    return j


def local_row(s, trig, con, first=None):
    g = goal_after(s, trig)
    reached = g is not None
    end = g if reached else min(len(s.calls) - 1, trig + value.H)
    a = trig + 1 if first is None else first
    n, usd = value.window_cost(s, a, end)
    if not reached:
        n += 1
    return n, usd, reached


# ------------------------------------------------------------------ main ----------------------

CON = None


def main():
    global CON
    U, P, _ = pickle.load(open(CACHE / "units_profiles.pkl", "rb"))
    A = pickle.load(open(CACHE / "train_actions.pkl", "rb"))
    CON = judge.Containment(A)
    taus = json.load(open(HERE / "results" / "judges.json"))["judges"]["containment"]["tau_by_heldout_repo"]
    rows = judged(U, P, taus)
    R = {"n_units": len(rows)}

    # clean randomised pairs: adage vs control (agents-md is not a control: it carries an AGENTS.md)
    clean = [r for r in rows if r["arm"] in ("adage", "control")]
    R["uptake_by_family_hook"] = rate_table(clean, ["pool", "family", "kind", "hook"])
    R["uptake_by_kind"] = rate_table(clean, ["kind"])
    R["uptake_by_hook"] = rate_table(clean, ["hook"])
    R["uptake_by_model"] = rate_table(clean, ["model", "family"])
    R["uptake_by_timing"] = rate_table(clean, ["timing"])
    R["uptake_by_wording"] = rate_table(clean, ["explains", "must"])
    R["uptake_vs_agentsmd"] = rate_table([r for r in rows if r["arm"] in ("adage", "agents-md")],
                                         ["family", "kind", "hook"])
    allt = [r for r in clean if r["treated"]]
    allc = [r for r in clean if not r["treated"]]
    R["pooled"] = {
        "p_follow": round(np.mean([r["follow"] for r in allt]), 3), "n_delivered": len(allt),
        "p_follow_ci": wilson(sum(r["follow"] for r in allt), len(allt)),
        "p_self": round(np.mean([r["follow"] for r in allc]), 3), "n_withheld": len(allc),
        "p_self_ci": wilson(sum(r["follow"] for r in allc), len(allc)),
    }
    d, ci, _, _ = diff_by_task(clean, "follow")
    R["pooled"]["uplift"], R["pooled"]["uplift_ci"] = d, ci
    # share of deliveries that changed behaviour: uplift-weighted
    changed = 0.0
    for t in R["uptake_by_family_hook"]:
        if t["uplift"] is not None and t["n_delivered"]:
            changed += max(t["uplift"], 0) * t["n_delivered"]
    R["pooled"]["deliveries_that_changed_behaviour"] = round(changed / max(1, len(allt)), 3)

    # literal vs learned vs instrument, delivered units
    tre = [r for r in rows if r["treated"]]
    conf = collections.Counter((r["literal"], r["follow"], r["follow_instr"]) for r in tre)
    lit_wrong = sum(r["literal"] != r["follow_instr"] for r in tre)
    lrn_wrong = sum(r["follow"] != r["follow_instr"] for r in tre)
    R["judge_error"] = {
        "delivered_units": len(tre),
        "literal_misjudged": lit_wrong, "literal_misjudged_rate": round(lit_wrong / len(tre), 3),
        "literal_misjudged_ci": wilson(lit_wrong, len(tre)),
        "learned_misjudged": lrn_wrong, "learned_misjudged_rate": round(lrn_wrong / len(tre), 3),
        "learned_misjudged_ci": wilson(lrn_wrong, len(tre)),
        "literal_p_follow": round(np.mean([r["literal"] for r in tre]), 3),
        "learned_p_follow": round(np.mean([r["follow"] for r in tre]), 3),
        "instrument_p_follow": round(np.mean([r["follow_instr"] for r in tre]), 3),
        "by_family": {},
        "confusion_literal_learned_instr": {str(k): v for k, v in conf.items()},
    }
    for fam in sorted({r["family"] for r in tre}):
        rs = [r for r in tre if r["family"] == fam]
        R["judge_error"]["by_family"][fam] = {
            "n": len(rs), "literal": round(np.mean([r["literal"] for r in rs]), 3),
            "learned": round(np.mean([r["follow"] for r in rs]), 3),
            "instrument": round(np.mean([r["follow_instr"] for r in rs]), 3),
            "literal_logged": round(np.mean([r["literal_logged"] for r in rs]), 3)}

    # ---------------- local value of the dead-end avoid (primary, clean arms) ------------------
    Ud = [(u, r) for u, r in zip(U, rows) if r["family"] == "dead-end" and r["arm"] in ("adage", "control")
          and u.pos > 0 and r["pool"] == "primary"]
    L = []
    for u, r in Ud:
        s, trig = u.s, u.pos - 1
        n, usd, ok = local_row(s, trig, CON)
        fn = first_need(s, trig, CON)
        n_pre, usd_pre, _ = local_row(s, trig, CON, first=fn)
        gcall = goal_after(s, trig)
        goal_usd = value.window_cost(s, gcall, gcall)[1] if gcall is not None else 0.0
        nl, usdl, okl = value.local(s, trig, CON)
        L.append({**r, "calls": n, "usd": usd, "reached": ok, "calls_from_first_need": n_pre,
                  "usd_from_first_need": usd_pre, "hits_before_trigger": trig - fn,
                  "preempt_saved_calls": n_pre - 1.0, "preempt_saved_usd": usd_pre - goal_usd,
                  "calls_learned_goal": nl, "usd_learned_goal": usdl})
    # the headline is the Python U11 situation; vuejs/core's two v1 `pnpm test` triggers are reported apart
    R["local_dead_end_vue_core"] = [{k: x[k] for k in ("treated", "calls", "follow", "lag_learned")}
                                    for x in L if x["repo"] == "vue-core"]
    L = [x for x in L if x["repo"] != "vue-core"]
    R["local_dead_end"] = {}
    for key in ["calls", "usd", "calls_learned_goal", "calls_from_first_need", "usd_from_first_need"]:
        d, ci, nt, nc = diff_by_task(L, key)
        R["local_dead_end"][key] = {
            "treated_mean": round(np.mean([x[key] for x in L if x["treated"]]), 4),
            "control_mean": round(np.mean([x[key] for x in L if not x["treated"]]), 4),
            "diff": d, "diff_ci90": ci, "n_treated": nt, "n_control": nc}
    Lt = [x for x in L if x["treated"]]
    Lc = [x for x in L if not x["treated"]]
    fol0 = [x for x in Lt if x["lag_learned"] == 0]
    q_null = np.mean([x["calls"] for x in Lc])
    q_null_usd = np.mean([x["usd"] for x in Lc])
    q_del = np.mean([x["calls"] for x in Lt])
    q_sat = np.mean([x["calls"] for x in fol0]) if fol0 else float("nan")
    q_oracle = 1.0  # the directed call itself is the goal
    # control agents who did Sat(d) on their own: what following is worth without a directive
    self_sat = [x for x in Lc if x["lag_learned"] == 0]
    R["gap"] = {
        "Q_null_calls": round(q_null, 3), "Q_delivered_calls": round(q_del, 3),
        "Q_followed_immediately_calls": round(q_sat, 3), "Q_oracle_calls": q_oracle,
        "Q_control_self_followed_calls": round(np.mean([x["calls"] for x in self_sat]), 3) if self_sat else None,
        "realised_calls_per_trigger": round(q_null - q_del, 3),
        "offered_calls_per_trigger_posthoc": round(q_null - q_sat, 3),
        "oracle_calls_per_trigger_posthoc": round(q_null - q_oracle, 3),
        "offered_calls_per_trigger_preemptive": round(float(np.mean([x["preempt_saved_calls"] for x in Lc])), 3),
        "offered_calls_per_trigger_preemptive_ci90": mean_ci(Lc, "preempt_saved_calls"),
        "offered_usd_per_trigger_preemptive": round(float(np.mean([x["preempt_saved_usd"] for x in Lc])), 4),
        "offered_usd_per_trigger_preemptive_ci90": mean_ci(Lc, "preempt_saved_usd"),
        "realised_usd_per_trigger": round(q_null_usd - np.mean([x["usd"] for x in Lt]), 4),
        "n_followed_immediately": len(fol0), "n_treated": len(Lt),
    }
    g = R["gap"]
    g["realised_over_offered_posthoc"] = round(g["realised_calls_per_trigger"] / g["offered_calls_per_trigger_posthoc"], 3) \
        if g["offered_calls_per_trigger_posthoc"] else None
    g["realised_over_preemptive_ceiling"] = round(g["realised_calls_per_trigger"] / g["offered_calls_per_trigger_preemptive"], 3)

    # --------------- dead-end hits Adage never saw (piped exit-0, launch bug 2) -----------------
    R["unseen_need"] = unseen_need(U, rows)

    # --------------- policies ------------------------------------------------------------------
    R["policies"] = policies(U, rows, L, R)
    R["p_follow_model"] = pfollow_model(clean)
    json.dump(R, open(HERE / "results" / "analysis.json", "w"), indent=1, default=float)
    pickle.dump((rows, L), open(CACHE / "rows.pkl", "wb"))
    print(json.dumps({k: R[k] for k in ["pooled", "judge_error", "local_dead_end", "gap", "unseen_need"]},
                     indent=1, default=float))
    print(json.dumps(R["policies"], indent=1, default=float))


def unseen_need(U, rows):
    """Sessions in the primary Python repositories whose test runs hit the environment dead end
    without Adage's trigger firing. A dead-end hit (learned): a Bash call with the intent of a
    logged trigger command in that repository (containment >= TAU_INTENT) whose output is within
    TAU_SAME_OUT of a logged trigger's output from ANOTHER session, and which ran no tests."""
    trig = [(u.s.repo, u.s.sid, action_text(u.s.calls[u.pos - 1], u.s.cwd), value.out_text(u.s.calls[u.pos - 1]))
            for u in U if u.family == "dead-end" and u.pos > 0 and u.s.pool == "primary"]
    pot = value.pot()
    TV = pot.vec([t[3] for t in trig])
    # v2 and the slice only: v1 ran before U11 was bound, so its trigger could never fire
    S = [s for s in data.load() if s.pool == "primary" and s.arm in ("adage", "control")
         and ("adage-rb2" in s.root or "slice" in s.root)
         and s.repo in ("attrs", "click", "marshmallow", "click-slice", "click-sonnet5")]
    logged = {(r["sid"]) for r in rows if r["family"] == "dead-end"}
    out = collections.Counter()
    per = []
    for s in S:
        cmds = [t for t in trig if t[0] == s.repo or (s.repo.startswith("click") and t[0].startswith("click"))]
        hit = None
        for c in s.calls:
            if c.name != "Bash" or gtest(c):
                continue
            a = action_text(c, s.cwd)
            if not any(CON.one(t[2], a) >= value.TAU_INTENT for t in cmds):
                continue
            v = pot.vec([value.out_text(c)])[0]
            others = np.array([t[1] != s.sid for t in trig])
            if others.any() and float((TV[others] @ v).max()) >= value.TAU_SAME_OUT:
                hit = c.idx
                break
        seen = s.sid in logged
        out[(s.model, s.arm, "hit" if hit is not None else "no-hit", "trigger" if seen else "no-trigger")] += 1
        per.append((s.sid, s.model, s.arm, hit, seen))
    res = {"|".join(k): v for k, v in sorted(out.items())}
    for model in ("haiku", "sonnet"):
        hits = [p for p in per if p[3] is not None and p[1] == model]
        res[f"{model}_hit_sessions"] = len(hits)
        res[f"{model}_sessions"] = sum(1 for p in per if p[1] == model)
        res[f"{model}_share_hit_without_trigger"] = round(sum(1 for p in hits if not p[4]) / max(1, len(hits)), 3)
        res[f"{model}_share_hit_without_trigger_ci"] = wilson(sum(1 for p in hits if not p[4]), len(hits))
    hits = [p for p in per if p[3] is not None]
    res["share_of_hit_sessions_without_trigger"] = round(sum(1 for p in hits if not p[4]) / max(1, len(hits)), 3)
    res["n_hit_sessions"] = len(hits)
    res["n_sessions"] = len(per)
    # INSTRUMENT check of the learned hit detector: `No module named|command not found` in a pytest call
    inst = re.compile(r"No module named|command not found|ModuleNotFoundError")
    agree = 0
    for sid, model, arm, hit, seen in per:
        s = next(x for x in S if x.sid == sid)
        ih = any(c.name == "Bash" and "pytest" in str(c.inp.get("command", "")) and inst.search(c.out or "")
                 and not gtest(c) for c in s.calls)
        agree += int(ih == (hit is not None))
    res["learned_hit_detector_agreement_with_instrument"] = round(agree / max(1, len(per)), 3)
    return res


def policies(U, rows, L, R):
    """Off-policy value of delivery policies.

    Logged policy: deliver at the trigger in `adage` sessions, withhold in `control` (session-level
    randomisation, propensity 0.5 each, by design of the A/B). For policies inside that support
    (deliver vs withhold per directive family) we use IPS, SNIPS and doubly-robust estimates with the
    known propensity; the outcome model for DR is the task's mean in each arm (leave-one-out).
    Policies outside the support (pre-emptive, every-time, rewrite) have propensity 0 in the logs:
    only a direct-method (model-based) estimate is possible, and it is labelled as such.
    """
    out = {}
    # ---- (1) IPS / SNIPS / DR over the dead-end triggers, local reward = -calls to goal ---------
    for key in ["calls", "usd"]:
        rs = [x for x in L]
        ests = {}
        for pol in ["deliver", "withhold"]:
            a_pi = pol == "deliver"
            ests[pol] = ope(rs, lambda r: a_pi, key)
        out[f"dead_end_local_{key}"] = ests
    # ---- (2) session-level targeting: which directive families to deliver at all --------------
    S = {}
    for r in rows:
        if r["arm"] not in ("adage", "control") or r["pool"] != "primary":
            continue
        S.setdefault(r["sid"], {"treated": r["treated"], "task": r["task"], "fams": set(), "repo": r["repo"],
                                "pool": r["pool"]})["fams"].add(r["family"])
    sess = {s.sid: s for s in data.load()}
    srows = []
    for sid, v in S.items():
        s = sess[sid]
        srows.append({"sid": sid, "task": v["task"], "treated": v["treated"], "fams": v["fams"],
                      "calls": float(len(s.calls)), "usd": s.cost or sum(c.turn_usd for c in s.calls),
                      "passed": float(bool(s.passed))})
    pols = {
        "deliver_all (logged adage arm)": lambda r: True,
        "withhold_all (logged control arm)": lambda r: False,
        "deliver_only_if_uplift>0 (dead-end avoid only)": lambda r: "dead-end" in r["fams"],
    }
    for key in ["calls", "usd", "passed"]:
        out[f"session_{key}"] = {name: ope(srows, f, key) for name, f in pols.items()}
        out[f"session_{key}_DR_diff_vs_withhold"] = {
            name: ope_diff(srows, f, lambda r: False, key) for name, f in pols.items() if "withhold" not in name}
    out["session_n"] = len(srows)

    # ---- (3) direct-method estimates for policies outside the logged support ------------------
    g = R["gap"]
    Lc = [x for x in L if not x["treated"]]
    Lt = [x for x in L if x["treated"]]
    pf_post = np.mean([x["follow"] for x in Lt])
    ps_post = np.mean([x["follow"] for x in Lc])
    pre = preemptive_evidence(rows)
    # pre-emptive advice at prompt time, p_follow measured on the skill slices' UPS avoid units
    pf_pre = pre["p_first_attempt_directed_preemptive"]
    base_first = pre["p_first_attempt_directed_control"]
    need = np.mean([x["preempt_saved_calls"] for x in Lc])
    need_usd = np.mean([x["preempt_saved_usd"] for x in Lc])
    q_sat_first = 0.0  # a pre-empted dead end costs nothing extra: the first attempt is the goal
    un = R["unseen_need"]
    n_hit = un["n_hit_sessions"]
    share_unseen = un["share_of_hit_sessions_without_trigger"]
    realised = g["realised_calls_per_trigger"]
    share_unseen = un["haiku_share_hit_without_trigger"]
    out["direct_method"] = {
        "note": "calls saved per session that hits the dead end, vs no directive; DM = model-based",
        "post_hoc_once (logged, realised)": {"calls": round(realised * (1 - share_unseen), 3),
                                            "why": "realised per trigger x share of hit sessions where the trigger fires"},
        "post_hoc_once + exit-0 signature scan (bug 2 fixed)": {"calls": round(realised, 3)},
        "pre-emptive advice at UserPromptSubmit (DM)": {
            "calls": round(pf_pre * need, 3),
            "calls_lower": round(pre["preemptive (skl-click adage, UPS advice)"]["ci"][0] * need, 3),
            "usd": round(pf_pre * need_usd, 4),
            "p_follow_first_attempt": pf_pre, "p_self_first_attempt": base_first,
            "cost_from_first_need_control": round(need, 3)},
        "pre-emptive PreToolUse rewrite (DM, p_follow=1 by construction)": {
            "calls": round(need, 3), "usd": round(need_usd, 4)},
        "every_time vs once (DM)": every_time(U, rows),
    }
    out["preemptive_evidence"] = pre
    return out


def ope(rs, pi, key, p=0.5):
    """IPS, SNIPS and DR for a deterministic policy pi(row)->deliver?, logged propensity p=0.5.
    Reward = the row's `key` (lower is better for calls/usd). Cluster bootstrap by task."""
    def est(sample):
        w, r, dm, a_match = [], [], [], []
        # outcome model q(task, a): task mean in that arm, leave-one-out; fall back to arm mean
        by = collections.defaultdict(list)
        for j, x in enumerate(sample):
            by[(x["task"], x["treated"])].append((j, x[key]))
        arm_mean = {a: np.mean([x[key] for x in sample if x["treated"] == a] or [np.nan]) for a in (True, False)}
        ips, snw, sn, dr = 0.0, 0.0, 0.0, 0.0
        for j, x in enumerate(sample):
            a = pi(x)
            vals = [v for (i, v) in by[(x["task"], a)] if i != j]  # leave this row out (cross-fit)
            q = np.mean(vals) if vals else arm_mean[a]
            if np.isnan(q):
                q = np.nanmean(list(arm_mean.values()))
            m = 1.0 if x["treated"] == a else 0.0
            ips += m * x[key] / p
            snw += m / p
            sn += m * x[key] / p
            dr += q + m * (x[key] - q) / p
        n = len(sample)
        return ips / n, (sn / snw if snw else np.nan), dr / n
    point = est(rs)
    by = collections.defaultdict(list)
    for x in rs:
        by[x["task"]].append(x)
    groups = list(by.values())
    boots = []
    for _ in range(600):
        idx = RNG.integers(0, len(groups), len(groups))
        boots.append(est([x for i in idx for x in groups[i]]))
    boots = np.array(boots)
    ci = lambda j: [round(float(np.nanpercentile(boots[:, j], 5)), 4), round(float(np.nanpercentile(boots[:, j], 95)), 4)]
    return {"IPS": round(point[0], 4), "SNIPS": round(point[1], 4), "DR": round(point[2], 4), "DR_ci90": ci(2),
            "n": len(rs), "n_matched": int(sum(1 for x in rs if x["treated"] == pi(x)))}


def mean_ci(rows, key):
    by = collections.defaultdict(list)
    for x in rows:
        by[x["task"]].append(x[key])
    return cluster_boot(list(by.values()), lambda gs: float(np.mean([v for g in gs for v in g])))


def ope_diff(rs, pi1, pi0, key):
    """DR(pi1) - DR(pi0) with a task-cluster bootstrap 90% CI."""
    def dr(sample, pi):
        by = collections.defaultdict(list)
        for j, x in enumerate(sample):
            by[(x["task"], x["treated"])].append((j, x[key]))
        am = {a: np.mean([x[key] for x in sample if x["treated"] == a] or [np.nan]) for a in (True, False)}
        tot = 0.0
        for j, x in enumerate(sample):
            a = pi(x)
            vals = [v for (i, v) in by[(x["task"], a)] if i != j]
            q = np.mean(vals) if vals else am[a]
            tot += q + (1.0 if x["treated"] == a else 0.0) * (x[key] - q) / 0.5
        return tot / len(sample)
    point = dr(rs, pi1) - dr(rs, pi0)
    by = collections.defaultdict(list)
    for x in rs:
        by[x["task"]].append(x)
    groups = list(by.values())
    bs = []
    for _ in range(600):
        idx = RNG.integers(0, len(groups), len(groups))
        smp = [x for i in idx for x in groups[i]]
        bs.append(dr(smp, pi1) - dr(smp, pi0))
    return {"diff": round(float(point), 4), "ci90": [round(float(np.nanpercentile(bs, 5)), 4),
                                                   round(float(np.nanpercentile(bs, 95)), 4)]}


def preemptive_evidence(rows):
    """First test attempt in the session: did it already use the directed form?

    Pre-emptive: the skill slices deliver `.venv/bin/python -m pytest`-type avoid advice at
    UserPromptSubmit (click: same four tasks as RepoBench; acme: a generated repo). Post-hoc and
    no-directive baselines: RepoBench click adage / control sessions on the same four tasks.
    The first attempt = the first Bash call whose learned containment of the directed command's
    intent (the directive minus its environment prefix is not separable without hand rules, so we
    use the trigger/directed pair: containment >= TAU_INTENT of the directed command) is high."""
    sess = {s.sid: s for s in data.load()}
    D = ".venv/bin/python -m pytest"
    tasks = {r["task"] for r in rows if r["repo"] == "skl-click"}

    def friction(s, d):
        """Calls from the first test-intent attempt to the first call that ran tests (0 = the first
        attempt ran tests), censored at H."""
        for c in s.calls:
            if c.name == "Bash" and CON.one(d, action_text(c, s.cwd)) >= value.TAU_INTENT:
                if gtest(c):
                    return 0
                g = goal_after(s, c.idx)
                return (g - c.idx) if g is not None else value.H + 1
        return None

    def first_attempt(s, d):
        for c in s.calls:
            if c.name == "Bash":
                a = action_text(c, s.cwd)
                sc = CON.one(d, a)
                if sc >= value.TAU_INTENT:
                    return sc >= 0.86, gtest(c)
        return None, None
    res = {}
    groups = {
        "preemptive (skl-click adage, UPS advice)": [s for s in sess.values() if s.repo == "skl-click" and s.arm == "adage"],
        "agents-md (skl-click, AGENTS.md)": [s for s in sess.values() if s.repo == "skl-click" and s.arm == "agents-md"],
        "post-hoc (RepoBench click adage, same tasks)": [s for s in sess.values() if s.repo == "click" and s.arm == "adage"
                                                         and s.task in tasks and s.pool == "primary"],
        "control (RepoBench click control, same tasks)": [s for s in sess.values() if s.repo == "click" and s.arm == "control"
                                                          and s.task in tasks and s.pool == "primary"],
        "control (RepoBench click control, all tasks)": [s for s in sess.values() if s.repo == "click" and s.arm == "control"
                                                         and s.pool == "primary"],
    }
    for name, ss in groups.items():
        fa = [first_attempt(s, D) for s in ss]
        fa = [f for f in fa if f[0] is not None]
        k = sum(1 for f in fa if f[0])
        fr = [f for f in (friction(s, D) for s in ss) if f is not None]
        res[name] = {"n_sessions": len(ss), "n_with_test_attempt": len(fa), "first_attempt_directed": k,
                     "friction_calls_mean": round(float(np.mean(fr)), 2) if fr else None,
                     "rate": round(k / len(fa), 3) if fa else None, "ci": wilson(k, len(fa)),
                     "mean_calls": round(float(np.mean([len(s.calls) for s in ss])), 2) if ss else None,
                     "mean_usd": round(float(np.mean([s.cost for s in ss])), 4) if ss else None,
                     "pass_rate": round(float(np.mean([bool(s.passed) for s in ss])), 3) if ss else None}
    res["p_first_attempt_directed_preemptive"] = res["preemptive (skl-click adage, UPS advice)"]["rate"]
    res["p_first_attempt_directed_control"] = res["control (RepoBench click control, all tasks)"]["rate"]
    # acme: adage (UPS advice) vs off (hooks off), generated repo
    acme = collections.defaultdict(list)
    for s in sess.values():
        if s.repo == "skl-acme":
            acme[s.arm].append(s)
    res["acme"] = {a: {"n": len(v), "mean_calls": round(float(np.mean([len(s.calls) for s in v])), 2),
                       "mean_usd": round(float(np.mean([s.cost for s in v])), 4),
                       "pass_rate": round(float(np.mean([bool(s.passed) for s in v])), 3)} for a, v in acme.items()}
    return res


def every_time(U, rows):
    """Relapses after the first post-hoc delivery: later calls in the same treated session that
    repeat the failing form (containment of the trigger >= TAU_INTENT, of the directed command
    < tau) and ran no tests. `every time` would re-deliver there; its value is the relapse's cost
    to the next goal times p_follow (DM)."""
    n_s, rel, cost = 0, 0, []
    for u, r in zip(U, rows):
        if not (r["treated"] and r["family"] == "dead-end" and r["pool"] == "primary" and u.pos > 0):
            continue
        n_s += 1
        s = u.s
        tt = action_text(s.calls[u.pos - 1], s.cwd)
        for c in s.calls[u.pos + 1:]:
            if c.name != "Bash" or gtest(c):
                continue
            a = action_text(c, s.cwd)
            failing = c.err or float(value.pot().vec([value.out_text(c)])[0] @
                                     value.pot().vec([value.out_text(s.calls[u.pos - 1])])[0]) >= value.TAU_SAME_OUT
            if CON.one(tt, a) >= value.TAU_INTENT and max(CON.one(d, a) for d in u.directed) < 0.86 and failing:
                rel += 1
                g = goal_after(s, c.idx)
                cost.append((g - c.idx) if g else value.H + 1)
    pf = np.mean([r["follow"] for r in rows if r["treated"] and r["family"] == "dead-end"])
    return {"treated_sessions": n_s, "relapses": rel, "relapse_rate_per_session": round(rel / max(1, n_s), 3),
            "mean_relapse_cost_calls": round(float(np.mean(cost)), 2) if cost else 0.0,
            "calls_saved_per_session_DM": round(rel / max(1, n_s) * (np.mean(cost) - 1 if cost else 0) * pf, 3)}


def pfollow_model(clean):
    """Hierarchical (empirical-Bayes) Beta model of p_follow and p_self by directive kind x hook,
    pooled toward the global rate; kappa by method of moments. Also a logistic regression of
    follow on hook, kind, wording and model among delivered units, to show what is identifiable."""
    out = {}
    for arm_name, arm in (("p_follow", True), ("p_self", False)):
        rs = [r for r in clean if r["treated"] == arm]
        cells = collections.defaultdict(list)
        for r in rs:
            cells[(r["kind"], r["hook"])].append(r["follow"])
        m = np.mean([r["follow"] for r in rs])
        ps = np.array([np.mean(v) for v in cells.values()])
        ns = np.array([len(v) for v in cells.values()])
        var = np.average((ps - m) ** 2, weights=ns) if len(ps) > 1 else 0.0
        kappa = max(1.0, m * (1 - m) / var - 1) if var > 0 else 50.0
        out[arm_name] = {"global": round(float(m), 3), "kappa": round(float(kappa), 2), "cells": {
            f"{k[0]}@{k[1]}": {"n": len(v), "raw": round(float(np.mean(v)), 3),
                               "posterior_mean": round(float((sum(v) + kappa * m) / (len(v) + kappa)), 3),
                               "beta": [round(float(sum(v) + kappa * m), 2), round(float(len(v) - sum(v) + kappa * (1 - m)), 2)]}
            for k, v in sorted(cells.items())}}
    # uplift per cell (what enters A(s,d))
    up = {}
    for k in out["p_follow"]["cells"]:
        pf = out["p_follow"]["cells"][k]["posterior_mean"]
        ps = out["p_self"]["cells"].get(k, {}).get("posterior_mean")
        up[k] = round(pf - ps, 3) if ps is not None else None
    out["uplift_posterior"] = up
    # logistic regression among delivered
    try:
        from sklearn.linear_model import LogisticRegression
        rs = [r for r in clean if r["treated"]]
        feats = sorted({f"hook={r['hook']}" for r in rs} | {f"kind={r['kind']}" for r in rs} | {"explains", "must", "sonnet", "words"})
        X = np.array([[1.0 if f == f"hook={r['hook']}" or f == f"kind={r['kind']}" else
                       (r["explains"] if f == "explains" else r["must"] if f == "must" else
                        float(r["model"] == "sonnet") if f == "sonnet" else r["words"] / 50.0 if f == "words" else 0.0)
                       for f in feats] for r in rs])
        y = np.array([r["follow"] for r in rs])
        if len(set(y)) > 1:
            lr = LogisticRegression(C=1.0, max_iter=1000).fit(X, y)
            out["logistic_delivered"] = {"features": feats, "coef": [round(float(c), 3) for c in lr.coef_[0]],
                                         "n": len(y), "positives": int(y.sum())}
        else:
            out["logistic_delivered"] = {"note": "no variation", "n": len(y)}
    except Exception as e:  # pragma: no cover
        out["logistic_delivered"] = {"error": str(e)}
    return out


if __name__ == "__main__":
    main()
