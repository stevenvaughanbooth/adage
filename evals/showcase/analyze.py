#!/usr/bin/env python3
"""Pre-registered analysis of the Adage showcase ($0, offline; every parameter from prereg.json).

  python3 analyze.py [--out analysis.json]           the live runs (roots from prereg.json)
  python3 analyze.py --interim                       stopping rule R4, after every family's repeat 1
  python3 analyze.py --synthetic [--out F]           the whole analysis on plasmode runs with known effects,
                                                     and the rules' operating characteristics
  python3 analyze.py --power [--out F]               the power analysis behind the design (prereg `power`)
  python3 analyze.py --roots-json F                  analyse other roots (the dry run's synthetic ones)

Unit: a held-out session. Pairs: (variant, repeat) with one `control` and one `adage` session of the same
family and model, both finished (status ok, or censored at the 60-turn cap); infra and scorer errors are
excluded (the runner's resume re-runs them). Only complete pairs enter a contrast.

  primary     $/task, Claude Haiku 4.5, pooled over every held-out pair of the 5 families: billed
              `total_cost_usd`, adage/control - 1 as the ratio of the pooled paired sums; 90% percentile
              bootstrap, STRATIFIED by family and CLUSTERED by variant (each family's variants resampled
              with replacement, both repeats of a variant together), 10,000 resamples, fixed seed
  secondary   tool calls and wall time (same estimator); pass rate per arm (hidden check, Wilson 90%) and
              the paired difference (same bootstrap); card delivery rate (adage sessions whose spool shows a
              card delivered, Wilson 90%); false fires (the $0 probe's 6 negatives per family through the
              installed hook, Wilson 90% upper bound)
  per family  every metric above within each family (8 variant clusters)
  Sonnet arm  the same contrasts on the Claude Sonnet 5 roots (2 families x 6 variants x 1 repeat)
  diagnostic  sensitivity without censored pairs; the probe-delivered subset (pre-treatment); $ per
              resolved task; censored sessions; per-check failures
"""

from __future__ import annotations

import argparse
import json
import math
import random
import re
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
PREREG = json.loads((HERE / "prereg.json").read_text())
AN = PREREG["analysis"]
SEED, N_BOOT, LEVEL = int(AN["seed"]), int(AN["n_boot"]), float(AN["level"])
ORDER = PREREG["design"]["families_in_order"]
SONNET_FAMILIES = PREREG["design"]["sonnet_arm"]["families"]
KEEP = ("ok", "censored")


def default_roots() -> dict:
    r = PREREG["roots"]
    return {"haiku": {f: r["haiku"][f] for f in ORDER}, "sonnet": {f: r["sonnet"][f] for f in SONNET_FAMILIES}}


# ------------------------------------------------------------------------------------ loading

def load(root) -> list[dict]:
    """Every finished eval session of a runner root."""
    p = Path(root) / "state.json"
    if not p.exists():
        return []
    st = json.loads(p.read_text())
    out = []
    for rid, r in st.get("runs", {}).items():
        m = r.get("metrics") or {}
        status = r.get("run_status") or m.get("status")
        if r.get("phase") != "eval" or status not in KEEP or not m.get("session_id"):
            continue
        rep = re.search(r"-r(\d+)-", rid)
        d = m.get("adage") or []
        card = [x for x in d if str(x.get("route") or "").startswith("card-")]
        out.append({
            "run": rid, "task": r["task"], "family": r["task"].split("--", 1)[0], "rep": int(rep.group(1)) if rep else 1,
            "arm": r["arm"], "cost": float(m.get("cost_usd") or 0.0), "calls": int(m.get("tool_calls") or 0),
            "wall": float(m.get("elapsed") or 0.0), "turns": int(m.get("turns") or 0), "passed": bool(m.get("passed")),
            "censored": status == "censored",
            "card_delivered": any(x.get("kind") == "directives" for x in card),
            "card_would_have": any(x.get("kind") == "would_have" for x in card),
            "checks": (r.get("score") or {}).get("checks"),
        })
    return out


def spend(root) -> dict:
    """Everything a root billed, analysed or not (training, infra errors and cut-off runs included)."""
    p = Path(root) / "state.json"
    if not p.exists():
        return {"usd": 0.0, "sessions": 0}
    runs = json.loads(p.read_text()).get("runs", {}).values()
    costs = [float((r.get("metrics") or {}).get("cost_usd") or 0.0) for r in runs]
    return {"usd": round(sum(costs), 4), "sessions": sum(1 for c in costs if c > 0),
            "by_phase": {ph: round(sum(float((r.get("metrics") or {}).get("cost_usd") or 0.0) for r in runs
                                       if r.get("phase") == ph), 4) for ph in ("train", "eval")}}


def harness(root) -> dict:
    p = Path(root) / "state.json"
    if not p.exists():
        return {}
    st = json.loads(p.read_text())
    return {"model": st.get("model"), "claude_code": st.get("harness_version"), "max_turns": st.get("max_turns")}


def pairs(sessions: list[dict], reps=None) -> list[tuple]:
    """(family, task, adage, control) for every complete (task, repeat)."""
    by: dict[tuple, dict] = {}
    for s in sessions:
        if reps and s["rep"] not in reps:
            continue
        by.setdefault((s["family"], s["task"], s["rep"]), {})[s["arm"]] = s
    return [(k[0], k[1], v["adage"], v["control"]) for k, v in sorted(by.items()) if "adage" in v and "control" in v]


# --------------------------------------------------------------------------------- statistics

def _pct(xs: list[float], q: float) -> float:
    xs = sorted(xs)
    i = q * (len(xs) - 1)
    lo, hi = math.floor(i), math.ceil(i)
    return xs[lo] + (xs[hi] - xs[lo]) * (i - lo)


def _strata(pr: list[tuple], f):
    """{family: {task: [f(adage, control) per repeat]}}: the strata and clusters of the bootstrap."""
    out: dict = {}
    for fam, task, a, c in pr:
        out.setdefault(fam, {}).setdefault(task, []).append(f(a, c))
    return {fam: list(tasks.values()) for fam, tasks in out.items()}


def _boot(strata: dict, stat, n: int, seed: int) -> list:
    """Rao-Wu rescaled bootstrap: from each stratum (family) of n_h clusters (variants), draw
    m_h = n_h - 1 with replacement and weight them n_h / m_h (unbiased variance with few clusters;
    with a single cluster, m_h = 1). `stat` gets [(weight, cluster), ...]."""
    rng = random.Random(seed)
    fams = sorted(strata)
    vals = []
    for _ in range(n):
        pick = []
        for fam in fams:
            cl = strata[fam]
            m = max(1, len(cl) - 1)
            w = len(cl) / m
            pick += [(w, cl[rng.randrange(len(cl))]) for _ in range(m)]
        vals.append(stat(pick))
    return vals


def ratio(pr: list[tuple], key: str, n: int | None = None, seed: int = SEED, level: float = LEVEL) -> dict | None:
    """Ratio of paired sums - 1 (adage/control), stratified cluster bootstrap percentile CI."""
    if not pr:
        return None
    n = n or N_BOOT
    st = _strata(pr, lambda a, c: (a[key], c[key]))

    def stat(clusters):
        x = sum(w * u for w, cl in clusters for u, _ in cl)
        y = sum(w * v for w, cl in clusters for _, v in cl)
        return x / y - 1 if y else 0.0

    point = stat([(1.0, cl) for v in st.values() for cl in v])
    vals = _boot(st, stat, n, seed)
    a = (1 - level) / 2
    sa, sc = sum(p[2][key] for p in pr), sum(p[3][key] for p in pr)
    return {"change": round(point, 4), "ci90": [round(_pct(vals, a), 4), round(_pct(vals, 1 - a), 4)],
            "pairs": len(pr), "variants": sum(len(v) for v in st.values()), "families": len(st),
            "adage_mean": round(sa / len(pr), 4), "control_mean": round(sc / len(pr), 4)}


def diff(pr: list[tuple], key: str, n: int | None = None, seed: int = SEED, level: float = LEVEL) -> dict | None:
    """Mean paired difference adage - control (a 0/1 or numeric field), same bootstrap."""
    if not pr:
        return None
    n = n or N_BOOT
    st = _strata(pr, lambda a, c: float(a[key]) - float(c[key]))

    def stat(clusters):
        num = sum(w * v for w, cl in clusters for v in cl)
        den = sum(w * len(cl) for w, cl in clusters)
        return num / den

    point = stat([(1.0, cl) for v in st.values() for cl in v])
    vals = _boot(st, stat, n, seed + 1)
    a = (1 - level) / 2
    return {"diff": round(point, 4), "ci90": [round(_pct(vals, a), 4), round(_pct(vals, 1 - a), 4)],
            "pairs": len(pr), "variants": sum(len(v) for v in st.values())}


def wilson(k: int, n: int, z: float = 1.6449) -> list[float]:
    if n == 0:
        return [0.0, 1.0]
    p = k / n
    den = 1 + z * z / n
    c = (p + z * z / (2 * n)) / den
    r = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / den
    return [round(max(0.0, c - r), 4), round(min(1.0, c + r), 4)]


def rate(k: int, n: int) -> dict:
    return {"k": k, "n": n, "rate": round(k / n, 4) if n else None, "wilson90": wilson(k, n)}


# ------------------------------------------------------------------------------------ contrasts

def arm_stats(ss: list[dict]) -> dict:
    n = len(ss)
    k = sum(s["passed"] for s in ss)
    cost = sum(s["cost"] for s in ss)
    fails: dict = {}
    for s in ss:
        for c, ok in (s.get("checks") or {}).items():
            fails[c] = fails.get(c, 0) + (not ok)
    return {"sessions": n, "usd_per_task": round(cost / n, 4) if n else None,
            "calls": round(sum(s["calls"] for s in ss) / n, 2) if n else None,
            "wall_s": round(sum(s["wall"] for s in ss) / n, 1) if n else None,
            "turns": round(sum(s["turns"] for s in ss) / n, 2) if n else None,
            "pass": rate(k, n), "usd_per_resolved": round(cost / k, 4) if k else None,
            "censored": sum(s["censored"] for s in ss), "card_delivered": sum(s["card_delivered"] for s in ss),
            "card_would_have": sum(s["card_would_have"] for s in ss), "check_failures": fails}


def contrast(pr: list[tuple], n: int | None = None) -> dict:
    used_a = [a for _, _, a, _ in pr]
    used_c = [c for _, _, _, c in pr]
    unc = [p for p in pr if not (p[2]["censored"] or p[3]["censored"])]
    ad = arm_stats(used_a)
    return {
        "arms": {"adage": ad, "control": arm_stats(used_c)},
        "usd": ratio(pr, "cost", n), "calls": ratio(pr, "calls", n), "wall": ratio(pr, "wall", n),
        "pass_diff": diff(pr, "passed", n),
        "pass_net": sum(a["passed"] for _, _, a, _ in pr) - sum(c["passed"] for _, _, _, c in pr),
        "card_delivery": rate(ad["card_delivered"], ad["sessions"]),
        "sensitivity_uncensored_usd": ratio(unc, "cost", n),
        "per_variant_usd": {t: round(sum(a["cost"] for _, u, a, _ in pr if u == t) /
                                     max(1e-9, sum(c["cost"] for _, u, _, c in pr if u == t)) - 1, 4)
                            for t in sorted({t for _, t, _, _ in pr})},
    }


def false_fires(gates: dict) -> dict:
    """Cards delivered to the negatives (at the prompt, or at the first edit of a card file: both
    count), with the split."""
    per, k, n, kp = {}, 0, 0, 0
    for fam, g in gates.items():
        s = (g or {}).get("summary") or {}
        ff = s.get("false_fires")
        if ff:
            per[fam] = {"false_fires": ff[0], "negatives": ff[1], "ids": s.get("false_fire_ids", []),
                        "at_prompt": s.get("false_fires_at_prompt"), "at_edit_only": s.get("false_fires_at_edit_only")}
            k, n, kp = k + ff[0], n + ff[1], kp + int(s.get("false_fires_at_prompt") or 0)
    return {"pooled": rate(k, n), "at_prompt": rate(kp, n), "at_edit_only": k - kp, "per_family": per,
            "source": "sc_detect.py probe through the installed hook"}


def analyse(roots: dict, n: int | None = None) -> dict:
    gates = {f: (json.loads((Path(r) / "gate.json").read_text()) if (Path(r) / "gate.json").exists() else None)
             for f, r in roots["haiku"].items()}
    ss = [s for f in ORDER for s in load(roots["haiku"][f])]
    pr = pairs(ss)
    res = {"primary_model": "claude-haiku-4-5", "pooled": contrast(pr, n), "families": {}, "sonnet": {}}
    for f in ORDER:
        fp = [p for p in pr if p[0] == f]
        g = (gates.get(f) or {}).get("summary") or {}
        c = contrast(fp, n) if fp else None
        delivered = set(g.get("delivered_tasks") or [])
        sub = [p for p in fp if p[1] in delivered]
        res["families"][f] = {"contrast": c, "gate": g or None,
                              "delivered_subset_usd": ratio(sub, "cost", n) if sub else None,
                              "harness": harness(roots["haiku"][f])}
    all_delivered = {t for f in ORDER for t in (((gates.get(f) or {}).get("summary") or {}).get("delivered_tasks") or [])}
    sub = [p for p in pr if p[1] in all_delivered]
    res["pooled"]["delivered_subset_usd"] = ratio(sub, "cost", n) if sub else None
    res["false_fires"] = false_fires(gates)
    sp = []
    for f in SONNET_FAMILIES:
        s = load(roots["sonnet"][f])
        fp = pairs(s)
        sp += fp
        res["sonnet"][f] = {"contrast": contrast(fp, n) if fp else None, "harness": harness(roots["sonnet"][f])}
    res["sonnet"]["pooled"] = contrast(sp, n) if sp else None
    all_roots = {**{f"{f}": r for f, r in roots["haiku"].items()}, **{f"{f}-sonnet": r for f, r in roots["sonnet"].items()}}
    res["spend"] = {k: spend(r) for k, r in all_roots.items()}
    res["spend_total_usd"] = round(sum(x["usd"] for x in res["spend"].values()), 2)
    res["complete"] = completeness(pr, sp)
    return res


def completeness(pr: list[tuple], sp: list[tuple]) -> dict:
    d = PREREG["design"]
    want_h = len(ORDER) * d["held_out_per_family"] * d["repeats"]
    want_s = len(SONNET_FAMILIES) * d["sonnet_arm"]["held_out"] * d["sonnet_arm"]["repeats"]
    return {"haiku_pairs": [len(pr), want_h], "sonnet_pairs": [len(sp), want_s],
            "haiku_complete": len(pr) == want_h, "sonnet_complete": len(sp) == want_s}


def interim(roots: dict, n: int | None = None) -> dict:
    """R4, after repeat 1 of every family: stop for harm, never for efficacy."""
    ss = [s for f in ORDER for s in load(roots["haiku"][f])]
    pr = pairs(ss, reps={1})
    c = contrast(pr, n or int(PREREG["stopping_rules"]["R4_thresholds"]["n_boot"]))
    R = PREREG["stopping_rules"]["R4_thresholds"]
    usd = c.get("usd") or {}
    pd = c.get("pass_diff") or {}
    harm_pass = c["pass_net"] <= -R["net_pass_loss"] and (not R.get("pass_ci_upper_below_0") or
                                                          (bool(pd) and pd["ci90"][1] < 0))
    harm_usd = bool(usd) and usd["change"] > R["usd_point"] and usd["ci90"][0] > 0
    return {"pairs": len(pr), "pass_net": c["pass_net"], "usd": usd, "stop_for_harm": harm_pass or harm_usd,
            "harm_pass": harm_pass, "harm_usd": harm_usd, "rule": PREREG["stopping_rules"]["R4"]}


# --------------------------------------------------------------------------- plasmode synthetic

def calibration() -> list[list[list[tuple]]]:
    """The research's real pairs, per family, grouped by variant, each family's card effect removed
    (card cost, calls and wall scaled so that the family's ratio of sums is 0): noise without effect."""
    cal = json.loads((HERE / "calibration.json").read_text())["pairs"]
    fams = []
    for _, ps in sorted(cal.items()):
        k = {key: sum(p["control"][key] for p in ps) / sum(p["card"][key] for p in ps) for key in ("cost", "calls", "elapsed")}
        by: dict = {}
        for p in ps:
            a = {"cost": p["card"]["cost"] * k["cost"], "calls": p["card"]["calls"] * k["calls"],
                 "wall": p["card"]["elapsed"] * k["elapsed"], "passed": p["card"]["passed"],
                 "censored": p["card"]["censored"], "turns": p["card"]["turns"]}
            c = {"cost": p["control"]["cost"], "calls": p["control"]["calls"], "wall": p["control"]["elapsed"],
                 "passed": p["control"]["passed"], "censored": p["control"]["censored"], "turns": p["control"]["turns"]}
            by.setdefault(p["task"], []).append((a, c))
        fams.append(list(by.values()))
    return fams


def synth_sessions(fam_id: str, src: list, n_var: int, reps: int, eff: float, p_det: float, rng: random.Random,
                   pass_shift: float = 0.0, scale: float = 1.0, fam_sd: float = 0.06) -> list[dict]:
    """Plasmode held-out sessions of one family: real variants resampled (with their repeats), a family
    effect around `eff` applied to the adage arm of the variants the card reaches (probability
    p_det per variant, fixed before treatment), zero effect elsewhere (pair noise kept)."""
    fe = eff + rng.gauss(0, fam_sd)
    out = []
    for h in range(n_var):
        v = rng.choice(src)
        det = rng.random() < p_det
        task = f"{fam_id}--v{h}"
        order = rng.sample(range(len(v)), len(v))  # which real repeat plays repeat 1 is random
        p_var = sum(a["passed"] + c["passed"] for a, c in v) / (2 * len(v))
        for r in range(1, reps + 1):
            j = order[r - 1] if reps == len(v) else rng.randrange(len(v))
            a, c = v[j]
            # pass under no pass effect: a fresh draw from the variant's observed pass rate (both arms, all
            # repeats), so the arms share the variant's propensity but not its outcome
            base_pass = rng.random() < p_var
            if det:
                a2 = {k: a[k] * (1 + fe * {"cost": 1.0, "calls": 0.9, "wall": 0.8}[k]) for k in ("cost", "calls", "wall")}
            else:  # no card: the adage session is a control-like draw (same variant, pair noise)
                z = math.exp(rng.gauss(-0.24 ** 2 / 2, 0.24))  # mean 1: no effect
                a2 = {k: c[k] * z for k in ("cost", "calls", "wall")}
            pa = base_pass if rng.random() >= abs(pass_shift) else (pass_shift > 0)
            for arm, x, passed, cen in (("adage", a2, pa, a["censored"]), ("control", c, c["passed"], c["censored"])):
                out.append({"run": f"eval-{task}-r{r}-{arm}", "task": task, "family": fam_id, "rep": r, "arm": arm,
                            "cost": x["cost"] * scale, "calls": int(round(x["calls"])), "wall": x["wall"],
                            "turns": int(round(x["calls"])) + 1, "passed": bool(passed), "censored": bool(cen),
                            "card_delivered": arm == "adage" and det, "card_would_have": arm == "control" and det,
                            "checks": {"tests": bool(passed)}})
    return out


def write_root(d: Path, sessions: list[dict], model: str) -> Path:
    runs = {}
    for s in sessions:
        spool = ([{"kind": "directives" if s["card_delivered"] else "would_have", "route": "card-0123456789"}]
                 if s["card_delivered"] or s["card_would_have"] else [])
        runs[s["run"]] = {"phase": "eval", "arm": s["arm"], "task": s["task"],
                          "run_status": "censored" if s["censored"] else "ok", "score": {"checks": s["checks"]},
                          "metrics": {"session_id": s["run"], "cost_usd": s["cost"], "tool_calls": s["calls"],
                                      "elapsed": s["wall"], "turns": s["turns"], "passed": s["passed"], "adage": spool}}
    d.mkdir(parents=True, exist_ok=True)
    (d / "state.json").write_text(json.dumps({"runs": runs, "model": model, "harness_version": "synthetic",
                                              "max_turns": 60}))
    return d


SCALE = {"click-type": 1.2, "mm-validator": 0.9, "go-flag": 1.0, "http-header": 1.1, "chi-middleware": 0.9}


def synth_roots(base: Path, eff: float, rng: random.Random, p_det: float = 0.75, pass_shift: float = 0.0,
                reps: int | None = None, n_var: int | None = None, sonnet_eff: float = -0.10,
                gates: bool = True) -> dict:
    """Plasmode roots for every family and the Sonnet arm. A null (eff 0) has no family effects at all;
    otherwise each family's effect varies around eff (sd 6 points)."""
    fam_sd = 0.0 if eff == 0 else 0.06
    src = calibration()
    d = PREREG["design"]
    roots = {"haiku": {}, "sonnet": {}}
    for i, f in enumerate(ORDER):
        ss = synth_sessions(f, src[i % len(src)], n_var or d["held_out_per_family"], reps or d["repeats"], eff,
                            p_det, rng, pass_shift, SCALE[f], fam_sd)
        r = write_root(base / f, ss, "claude-haiku-4-5-20251001")
        if gates:
            delivered = sorted({s["task"] for s in ss if s["card_delivered"]})
            (r / "gate.json").write_text(json.dumps({"summary": {
                "card_formed": True, "held_out_delivered": [len(delivered), n_var or d["held_out_per_family"]],
                "delivered_tasks": delivered, "false_fires": [0, 6], "false_fire_ids": [],
                "false_fires_at_prompt": 0, "false_fires_at_edit_only": 0}}))
        roots["haiku"][f] = str(r)
    for i, f in enumerate(SONNET_FAMILIES):
        ss = synth_sessions(f, src[(i + 1) % len(src)], d["sonnet_arm"]["held_out"], d["sonnet_arm"]["repeats"],
                            sonnet_eff, p_det, rng, 0.0, SCALE[f] * 1.4)
        roots["sonnet"][f] = str(write_root(base / f"{f}-sonnet", ss, "claude-sonnet-5"))
    return roots


def power(out: Path | None = None, datasets: int = 200, n_boot: int = 1000) -> dict:
    """Plasmode power of the primary for candidate designs: the median 90% CI half-width (points),
    P(half-width <= target), P(CI upper < 0) at true -20% and -10% (card effect where delivered,
    ITT-diluted by a 75% delivery rate), coverage of the ITT truth, and per-family half-widths."""
    src = calibration()
    rng = random.Random(SEED)
    target = PREREG["power"]["target_half_width_points"]
    res = {"method": PREREG["power"]["method"], "datasets": datasets, "n_boot": n_boot, "designs": {}}
    designs = [(5, 8, 2), (5, 8, 1), (5, 9, 1), (5, 6, 2), (5, 10, 2), (4, 8, 2)]
    for F_, H, R in designs:
        hw, fam_hw, pts, cis, sig20, sig10 = [], [], [], [], 0, 0
        for i in range(datasets):
            for eff, tally in ((-0.20, "20"), (-0.10, "10")):
                ss = []
                for fi in range(F_):
                    ss += synth_sessions(ORDER[fi], src[fi % len(src)], H, R, eff, 0.75, rng, 0.0, SCALE[ORDER[fi]])
                pr = pairs(ss)
                b = ratio(pr, "cost", n_boot, seed=i)
                if tally == "20":
                    hw.append((b["ci90"][1] - b["ci90"][0]) / 2 * 100)
                    pts.append(b["change"])
                    cis.append(b["ci90"])
                    sig20 += b["ci90"][1] < 0
                    if i < 40:
                        f0 = [p for p in pr if p[0] == ORDER[0]]
                        fb = ratio(f0, "cost", n_boot, seed=i)
                        fam_hw.append((fb["ci90"][1] - fb["ci90"][0]) / 2 * 100)
                else:
                    sig10 += b["ci90"][1] < 0
        target_mean = sum(pts) / len(pts)  # what the estimator targets under this design (ITT, cost-weighted)
        res["designs"][f"{F_}x{H}x{R}"] = {
            "families": F_, "held_out": H, "repeats": R, "held_out_sessions": F_ * H * R * 2,
            "half_width_median": round(_pct(hw, 0.5), 2), "half_width_p90": round(_pct(hw, 0.9), 2),
            "p_half_width_le_target": round(sum(h <= target for h in hw) / len(hw), 3),
            "p_ci_upper_below_0_at_true_-20pct": round(sig20 / datasets, 3),
            "p_ci_upper_below_0_at_true_-10pct": round(sig10 / datasets, 3),
            "mean_estimate_at_true_-20pct": round(target_mean, 4),
            "ci90_coverage_of_mean_estimate": round(sum(lo <= target_mean <= hi for lo, hi in cis) / len(cis), 3),
            "per_family_half_width_median": round(_pct(fam_hw, 0.5), 2)}
    # CI calibration of the chosen design with a known truth: every variant reached (p_det = 1), no
    # family heterogeneity, true -20%: coverage of the pooled and of a per-family 90% interval.
    d = PREREG["design"]
    hit = hit_f = 0
    for i in range(datasets):
        ss = []
        for fi, f in enumerate(ORDER):
            ss += synth_sessions(f, src[fi % len(src)], d["held_out_per_family"], d["repeats"], -0.20, 1.0, rng, 0.0,
                                 SCALE[f], fam_sd=0.0)
        pr = pairs(ss)
        b = ratio(pr, "cost", n_boot, seed=i)["ci90"]
        fb = ratio([p for p in pr if p[0] == ORDER[i % len(ORDER)]], "cost", n_boot, seed=i)["ci90"]
        hit += b[0] <= -0.20 <= b[1]
        hit_f += fb[0] <= -0.20 <= fb[1]
    res["ci90_coverage_known_truth"] = {"pooled": round(hit / datasets, 3), "per_family": round(hit_f / datasets, 3),
                                        "design": f"{len(ORDER)}x{d['held_out_per_family']}x{d['repeats']}"}
    # the Sonnet arm: 2 families x 6 variants x 1 repeat
    hw = []
    for i in range(datasets):
        ss = []
        for fi, f in enumerate(SONNET_FAMILIES):
            ss += synth_sessions(f, src[fi % len(src)], 6, 1, -0.10, 0.75, rng, 0.0, SCALE[f])
        b = ratio(pairs(ss), "cost", n_boot, seed=i)
        hw.append((b["ci90"][1] - b["ci90"][0]) / 2 * 100)
    res["sonnet_2x6x1"] = {"half_width_median": round(_pct(hw, 0.5), 2), "half_width_p90": round(_pct(hw, 0.9), 2)}
    if out:
        out.write_text(json.dumps(res, indent=1) + "\n")
    return res


def synthetic(out: Path | None = None, n_sets: int = 200) -> dict:
    """The whole analysis on plasmode runs with known effects, and the operating characteristics of
    the interim harm rule (R4) and of the reporting rules."""
    rng = random.Random(SEED + 7)
    res = {"scenarios": {}, "operating_characteristics": {}}
    with tempfile.TemporaryDirectory() as t:
        t = Path(t)
        for name, eff in (("effect_-20pct", -0.20), ("null", 0.0)):
            roots = synth_roots(t / name, eff, rng)
            a = analyse(roots)
            res["scenarios"][name] = {"true_card_effect": eff, "usd": a["pooled"]["usd"],
                                      "calls": a["pooled"]["calls"], "wall": a["pooled"]["wall"],
                                      "pass_diff": a["pooled"]["pass_diff"], "delivery": a["pooled"]["card_delivery"],
                                      "interim_stop": interim(roots)["stop_for_harm"]}
        oc = {}
        for name, eff, ps in (("null", 0.0, 0.0), ("true_-20pct", -0.20, 0.0), ("harm_pass_-25pts", -0.20, -0.25),
                              ("harm_pass_-35pts", -0.20, -0.35), ("harm_usd_+25pct", 0.25, 0.0)):
            stops = sig_save = sig_harm = 0
            for i in range(n_sets):
                roots = synth_roots(t / "oc" / name / str(i), eff, rng, pass_shift=ps, gates=False)
                it = interim(roots, 300)
                stops += it["stop_for_harm"]
                ss = [s for f in ORDER for s in load(roots["haiku"][f])]
                b = ratio(pairs(ss), "cost", 500, seed=i)
                sig_save += b["ci90"][1] < 0
                sig_harm += b["ci90"][0] > 0
            oc[name] = {"interim_stop": round(stops / n_sets, 3), "reports_saving": round(sig_save / n_sets, 3),
                        "reports_increase": round(sig_harm / n_sets, 3)}
        res["operating_characteristics"] = oc
    res["checks"] = {"null_interim_false_stop_le_0.05": oc["null"]["interim_stop"] <= 0.05,
                     "null_false_saving_le_0.10": oc["null"]["reports_saving"] <= 0.10,
                     "true_-20pct_interim_false_stop_le_0.05": oc["true_-20pct"]["interim_stop"] <= 0.05,
                     "harm_pass_-35pts_stops_ge_0.4": oc["harm_pass_-35pts"]["interim_stop"] >= 0.4,
                     "harm_usd_+25pct_stops_ge_0.6": oc["harm_usd_+25pct"]["interim_stop"] >= 0.6}
    res["checks_pass"] = all(res["checks"].values())
    if out:
        out.write_text(json.dumps(res, indent=1) + "\n")
    return res


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default="")
    ap.add_argument("--roots-json", default="")
    ap.add_argument("--interim", action="store_true")
    ap.add_argument("--synthetic", action="store_true")
    ap.add_argument("--power", action="store_true")
    a = ap.parse_args(argv)
    out = Path(a.out) if a.out else None
    if a.power:
        res = power(out)
        print(json.dumps(res, indent=1))
        return 0
    if a.synthetic:
        res = synthetic(out)
        print(json.dumps(res, indent=1))
        return 0 if res["checks_pass"] else 1
    roots = json.loads(Path(a.roots_json).read_text()) if a.roots_json else default_roots()
    if a.interim:
        res = interim(roots)
        print(json.dumps(res, indent=1))
        return 4 if res["stop_for_harm"] else 0
    res = analyse(roots)
    text = json.dumps(res, indent=1)
    if out:
        out.write_text(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
