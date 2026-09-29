#!/usr/bin/env python3
"""Pre-registered analysis of the GO validation ($0, offline; thresholds from prereg.json).

  python3 analyze.py [--family-root R] [--regression-root R] [--gate gate.json] [--out analysis.json]
  python3 analyze.py --interim [--family-root R]      stopping rule R4 after repeat 1 of the family
  python3 analyze.py --synthetic [--out F]            the whole analysis on synthetic runs with known effects

Unit: a held-out session. Pairs: (variant, repeat) with one `control` and one `adage` session, both
finished (status ok or censored at the turn cap; infra and scorer errors are excluded and re-run by
the runner's resume). Only complete pairs enter a contrast.

  primary    $/task: billed `total_cost_usd` per session, adage/control - 1 as the ratio of the paired
             sums; 90% percentile bootstrap resampling VARIANTS (the task is the cluster: both repeats
             of a variant move together), 10,000 resamples, fixed seed
  secondary  tool calls per session (same estimator); pass rate per arm (hidden check, Wilson 90%) and
             its paired difference (same cluster bootstrap); $ per resolved task
  delivery   card delivered in adage sessions (spool route `card-…`, kind directives) and would-have in
             control sessions; the offline probe (gate.json) for false fires on the negatives
  regression the same contrasts on the click slice (6 tasks, 1 repeat), card deliveries (must be 0),
             and the U11 `.venv` rewrite (dead-end instinct routes) still delivered
  sensitivity the primary without pairs in which either session hit the turn cap
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
CRIT = PREREG["criteria"]
SEED = int(PREREG["analysis"]["seed"])
N_BOOT = int(PREREG["analysis"]["n_boot"])
LEVEL = float(PREREG["analysis"]["level"])
ROOTS = PREREG["roots"]
KEEP = ("ok", "censored")


# ------------------------------------------------------------------------------------ loading

def load(root: Path) -> list[dict]:
    """Every finished eval session of a runner root, with the fields the analysis needs."""
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
            "run": rid, "task": r["task"], "rep": int(rep.group(1)) if rep else 1, "arm": r["arm"],
            "cost": float(m.get("cost_usd") or 0.0), "calls": int(m.get("tool_calls") or 0),
            "turns": int(m.get("turns") or 0), "passed": bool(m.get("passed")), "censored": status == "censored",
            "card_delivered": any(x.get("kind") == "directives" for x in card),
            "card_would_have": any(x.get("kind") == "would_have" for x in card),
            "dead_end_directives": sum(1 for x in d if x.get("kind") == "directives"
                                       and str(x.get("route") or "").startswith("dead-end-")),
            "checks": (r.get("score") or {}).get("checks"),
        })
    return out


def pairs(sessions: list[dict], arm: str = "adage", ctrl: str = "control", reps=None) -> list[tuple]:
    by: dict[tuple, dict] = {}
    for s in sessions:
        if reps and s["rep"] not in reps:
            continue
        by.setdefault((s["task"], s["rep"]), {})[s["arm"]] = s
    return [(k[0], v[arm], v[ctrl]) for k, v in sorted(by.items()) if arm in v and ctrl in v]


# --------------------------------------------------------------------------------- statistics

def _pct(xs: list[float], q: float) -> float:
    xs = sorted(xs)
    i = q * (len(xs) - 1)
    lo, hi = math.floor(i), math.ceil(i)
    return xs[lo] + (xs[hi] - xs[lo]) * (i - lo)


def boot_ratio(pr: list[tuple], key: str, n: int | None = None, seed: int = SEED, level: float = LEVEL) -> dict | None:
    """Ratio of paired sums - 1, percentile CI from a bootstrap over clusters (tasks)."""
    n = n or N_BOOT
    if not pr:
        return None
    cl = sorted({t for t, _, _ in pr})
    by = {c: [(a[key], b[key]) for t, a, b in pr if t == c] for c in cl}
    sa, sb = sum(a[key] for _, a, _ in pr), sum(b[key] for _, _, b in pr)
    point = sa / sb - 1 if sb else float("nan")
    rng = random.Random(seed)
    vals = []
    for _ in range(n):
        pick = [cl[rng.randrange(len(cl))] for _ in cl]
        x = sum(u for c in pick for u, _ in by[c])
        y = sum(v for c in pick for _, v in by[c])
        vals.append(x / y - 1 if y else 0.0)
    a = (1 - level) / 2
    return {"ratio_minus_1": round(point, 4), "ci": [round(_pct(vals, a), 4), round(_pct(vals, 1 - a), 4)],
            "pairs": len(pr), "clusters": len(cl), "adage_mean": round(sa / len(pr), 4),
            "control_mean": round(sb / len(pr), 4)}


def boot_diff(pr: list[tuple], key: str, n: int | None = None, seed: int = SEED, level: float = LEVEL) -> dict | None:
    """Mean paired difference adage - control of a 0/1 or numeric field, cluster bootstrap."""
    n = n or N_BOOT
    if not pr:
        return None
    cl = sorted({t for t, _, _ in pr})
    by = {c: [float(a[key]) - float(b[key]) for t, a, b in pr if t == c] for c in cl}
    allv = [v for c in cl for v in by[c]]
    rng = random.Random(seed + 1)
    vals = []
    for _ in range(n):
        xs = [v for c in (cl[rng.randrange(len(cl))] for _ in cl) for v in by[c]]
        vals.append(sum(xs) / len(xs))
    a = (1 - level) / 2
    return {"diff": round(sum(allv) / len(allv), 4), "ci": [round(_pct(vals, a), 4), round(_pct(vals, 1 - a), 4)],
            "pairs": len(pr), "clusters": len(cl)}


def wilson(k: int, n: int, z: float = 1.6449) -> list[float]:
    if n == 0:
        return [0.0, 1.0]
    p = k / n
    den = 1 + z * z / n
    c = (p + z * z / (2 * n)) / den
    r = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / den
    return [round(max(0.0, c - r), 4), round(min(1.0, c + r), 4)]


def card_files(root: Path) -> list[str]:
    """The files and patterns of every card in a root's learned index (`<x>` becomes `*`)."""
    p = Path(root) / "private" / "adage-home" / "index.json"
    if not p.exists():
        return []
    out = []
    for e in json.loads(p.read_text()).get("routes", []):
        step = (e.get("route") or {}).get("step") or {}
        card = step.get("card") or (step.get("Card") or {}).get("card")
        for f in (card or {}).get("files", []):
            out.append(str(f.get("path", "")).replace("<x>", "*"))
    return out


def fix_files(task: str) -> list[str]:
    """The source files a RepoBench task's real fix edits (tasks/<repo>.json)."""
    repo = task.split("-", 1)[0]
    p = HERE.parents[1] / "repobench" / "tasks" / f"{repo}.json"
    if not p.exists():
        return []
    t = next((x for x in json.loads(p.read_text())["tasks"] if x["id"] == task), {})
    return list(t.get("src_files") or []) + list(t.get("test_files") or [])


def classify_deliveries(ss: list[dict], files: list[str]) -> dict:
    """Card deliveries (adage) and would-haves (control) on one-off tasks: in the card's area (the
    real fix edits one of its files) or a false fire."""
    import fnmatch
    out = {"in_area": [], "false_fire": []}
    for s in ss:
        if s["card_delivered"] or s["card_would_have"]:
            hit = any(fnmatch.fnmatch(f, g) for f in fix_files(s["task"]) for g in files)
            out["in_area" if hit else "false_fire"].append(f"{s['task']}/{s['arm']}")
    return out


def arm_stats(ss: list[dict]) -> dict:
    n = len(ss)
    k = sum(s["passed"] for s in ss)
    cost = sum(s["cost"] for s in ss)
    return {"sessions": n, "usd_per_task": round(cost / n, 4) if n else None,
            "calls": round(sum(s["calls"] for s in ss) / n, 2) if n else None,
            "turns": round(sum(s["turns"] for s in ss) / n, 2) if n else None,
            "passed": [k, n], "pass_rate_wilson90": wilson(k, n),
            "usd_per_resolved": round(cost / k, 4) if k else None,
            "censored": sum(s["censored"] for s in ss),
            "card_delivered": sum(s["card_delivered"] for s in ss),
            "card_would_have": sum(s["card_would_have"] for s in ss),
            "dead_end_directives": sum(s["dead_end_directives"] for s in ss)}


def contrast(ss: list[dict], reps=None) -> dict:
    pr = pairs(ss, reps=reps)
    used = [a for _, a, _ in pr] + [b for _, _, b in pr]
    unc = [(t, a, b) for t, a, b in pr if not (a["censored"] or b["censored"])]
    return {
        "arms": {"adage": arm_stats([s for s in used if s["arm"] == "adage"]),
                 "control": arm_stats([s for s in used if s["arm"] == "control"])},
        "usd": boot_ratio(pr, "cost"), "calls": boot_ratio(pr, "calls"), "turns": boot_ratio(pr, "turns"),
        "pass_diff": boot_diff(pr, "passed"),
        "pass_net": sum(a["passed"] for _, a, _ in pr) - sum(b["passed"] for _, _, b in pr),
        "sensitivity_uncensored_usd": boot_ratio(unc, "cost"),
        "per_task_usd": {t: round(sum(a["cost"] for u, a, _ in pr if u == t) /
                                  max(1e-9, sum(b["cost"] for u, _, b in pr if u == t)) - 1, 4)
                         for t in sorted({t for t, _, _ in pr})},
        "unpaired_sessions": len(ss) - len(used) if reps is None else None,
    }


# ------------------------------------------------------------------------------------ verdict

def verdict(fam: dict, reg: dict | None, gate: dict | None) -> dict:
    c = CRIT
    g = {}
    s = (gate or {}).get("summary") or {}
    g["G1_card_formed"] = bool(s.get("gate_R2_card_formed")) if gate else None
    usd = fam.get("usd") or {}
    g["G2_primary_usd"] = (bool(usd) and usd["ratio_minus_1"] <= c["G2_usd_point_max"] and usd["ci"][1] < 0) if usd else None
    adage, ctrl = fam["arms"]["adage"], fam["arms"]["control"]
    pd = fam.get("pass_diff") or {}
    g["G3_pass_noninferior"] = (fam["pass_net"] >= -c["G3_max_net_pass_loss"]) if pd else None
    n = adage["sessions"]
    g["G4a_delivered_live"] = (adage["card_delivered"] >= math.ceil(c["G4_min_live_delivery_share"] * n)) if n else None
    g["G4b_no_false_fire_probe"] = (s.get("false_fires", [None])[0] == 0) if gate else None
    if reg:
        ra, rc = reg["arms"]["adage"], reg["arms"]["control"]
        ru = reg.get("usd") or {}
        g["G4c_no_false_fire_on_regression"] = not (reg.get("card_deliveries") or {}).get("false_fire")
        g["G5_regression_no_harm"] = (reg["pass_net"] >= -c["G5_max_net_pass_loss"]
                                      and (ru.get("ratio_minus_1", 0) <= c["G5_usd_point_max"]))
    else:
        g["G4c_no_false_fire_on_regression"] = g["G5_regression_no_harm"] = None
    required = [k for k in g]
    hard = ["G1_card_formed", "G3_pass_noninferior", "G4b_no_false_fire_probe", "G4c_no_false_fire_on_regression",
            "G5_regression_no_harm"]
    caveats = []
    if usd and usd["ci"][1] >= 0:
        caveats.append("G2's 90% CI includes 0 (underpowered)")
    if g["G4a_delivered_live"] is False:
        caveats.append("G4a: the card reached fewer than 75% of the adage sessions (detection recall; the ITT primary carries the misses)")
    if any(g[k] is None for k in required):
        v = "INCOMPLETE"
    elif all(g.values()):
        v = "GO"
    elif all(g[k] for k in hard) and usd and usd["ratio_minus_1"] <= c["G2_usd_point_max"] and caveats:
        v = "GO-WITH-CAVEATS: " + "; ".join(caveats)
    else:
        v = "NO-GO"
    return {"criteria": g, "verdict": v, "calls_ci_upper_below_0": (fam.get("calls") or {}).get("ci", [0, 1])[1] < 0,
            "note": "criteria and thresholds: prereg.json"}


def interim(fam_root: Path) -> dict:
    """R4, after repeat 1 of every held-out variant: stop for harm, never for efficacy."""
    c = contrast(load(fam_root), reps={1})
    usd = c.get("usd") or {}
    harm_pass = c["pass_net"] <= -CRIT["R4_net_pass_loss_stop"]
    harm_usd = bool(usd) and usd["ratio_minus_1"] > CRIT["R4_usd_point_stop"] and usd["ci"][0] > 0
    return {"pairs": (usd or {}).get("pairs", 0), "pass_net": c["pass_net"], "usd": usd,
            "stop_for_harm": harm_pass or harm_usd, "harm_pass": harm_pass, "harm_usd": harm_usd,
            "rule": "R4 (prereg.json): stop if adage has >= 3 fewer passes than control over the 8 first-repeat "
                    "pairs, or if its $/task is > +25% with the CI lower bound > 0"}


def analyse(fam_root: Path, reg_root: Path | None, gate_path: Path | None) -> dict:
    ss = load(fam_root)
    fam = contrast(ss)
    reg_ss = load(reg_root) if reg_root else []
    reg = contrast(reg_ss) if reg_ss else None
    if reg is not None:
        reg["card_deliveries"] = classify_deliveries(reg_ss, card_files(reg_root))
    gate = json.loads(gate_path.read_text()) if gate_path and gate_path.exists() else None
    delivered = set(((gate or {}).get("summary") or {}).get("delivered_tasks") or [])
    # G2b: the pre-treatment subset the probe delivered the card for (fixed before the eval).
    sub = contrast([s for s in ss if s["task"] in delivered]) if delivered else None
    return {"family": fam, "family_delivered_subset_G2b": sub and {k: sub[k] for k in ("arms", "usd", "calls", "pass_net")},
            "regression": reg, "gate": (gate or {}).get("summary"), "verdict": verdict(fam, reg, gate)}


# ---------------------------------------------------------------------------------- synthetic

def synth_root(d: Path, tasks: list[str], reps: int, effect: float, rng: random.Random, card_p: float,
               base: float = 0.20, pass_c: float = 0.7, pass_a: float = 0.7, censor_p: float = 0.05) -> Path:
    """A runner root whose eval sessions have a known effect: adage cost = control cost x (1+effect),
    with a per-task level and per-session noise (lognormal, CV about 0.3, as live sessions)."""
    runs = {}
    for t in tasks:
        level = base * math.exp(rng.gauss(0, 0.35))
        # Pass/fail is mostly the task (success-uplift: 79% of its variance lies between tasks): each
        # task has its own propensity, shared by both arms, and the arm shifts it.
        p_task = min(0.98, max(0.02, pass_c + rng.gauss(0, 0.35)))
        p_arm = {"control": p_task, "adage": min(0.98, max(0.02, p_task + pass_a - pass_c))}
        for r in range(1, reps + 1):
            for arm in ("control", "adage"):
                mult = (1 + effect) if arm == "adage" else 1.0
                cost = level * mult * math.exp(rng.gauss(0, 0.3))
                calls = max(3, int(round(cost / base * 30 * math.exp(rng.gauss(0, 0.1)))))
                card = arm == "adage" and rng.random() < card_p
                runs[f"eval-{t}-r{r}-{arm}"] = {
                    "phase": "eval", "arm": arm, "task": t, "run_status": "censored" if rng.random() < censor_p else "ok",
                    "metrics": {"session_id": f"s-{t}-{r}-{arm}", "cost_usd": cost, "tool_calls": calls, "turns": calls + 1,
                                "passed": rng.random() < p_arm[arm],
                                "adage": ([{"kind": "directives", "route": "card-0123456789"}] if card else [])
                                + ([{"kind": "would_have", "route": "card-0123456789"}] if arm == "control" and card_p else [])}}
    d.mkdir(parents=True, exist_ok=True)
    (d / "state.json").write_text(json.dumps({"runs": runs}))
    return d


def synthetic(out: Path | None) -> dict:
    rng = random.Random(7)
    ftasks = [f"ufo-util--v{i}" for i in range(8)]
    ctasks = [f"click-t{i}" for i in range(6)]
    res = {"scenarios": {}}
    with tempfile.TemporaryDirectory() as t:
        t = Path(t)
        gate_ok = t / "gate.json"
        gate_ok.write_text(json.dumps({"summary": {"gate_R2_card_formed": True, "false_fires": [0, 8]}}))
        for name, eff, card_p in (("effect_-20pct", -0.20, 0.95), ("null_effect", 0.0, 0.95), ("harm_+20pct", 0.20, 0.95)):
            fr = synth_root(t / name / "fam", ftasks, 2, eff, rng, card_p)
            rr = synth_root(t / name / "reg", ctasks, 1, 0.0, rng, 0.0, base=0.30, pass_c=0.5, pass_a=0.5)
            a = analyse(fr, rr, gate_ok)
            res["scenarios"][name] = {"true_effect": eff, "usd": a["family"]["usd"], "calls": a["family"]["calls"],
                                      "pass_diff": a["family"]["pass_diff"], "verdict": a["verdict"]["verdict"],
                                      "interim": interim(fr)["stop_for_harm"]}
        # Operating characteristics of the whole pre-registered verdict (200 datasets per scenario,
        # 1,000 resamples each): how often each verdict comes out under a known truth.
        global N_BOOT
        saved, N_BOOT = N_BOOT, 1000
        oc = {}
        for name, eff, pa, reg_eff in (("true_-25pct", -0.25, 0.7, 0.0), ("true_-20pct", -0.20, 0.7, 0.0),
                                       ("true_-10pct", -0.10, 0.7, 0.0), ("null", 0.0, 0.7, 0.0),
                                       ("harm_pass_-20pts", -0.20, 0.5, 0.0), ("harm_regression_+30pct", -0.20, 0.7, 0.30)):
            tally = {"GO": 0, "GO-WITH-CAVEATS": 0, "NO-GO": 0}
            g3 = g5 = 0
            for i in range(200):
                fr = synth_root(t / "oc" / name / str(i) / "f", ftasks, 2, eff, rng, 0.95, pass_a=pa)
                rr = synth_root(t / "oc" / name / str(i) / "r", ctasks, 1, reg_eff, rng, 0.0, base=0.30, pass_c=0.5,
                                pass_a=0.5)
                v = analyse(fr, rr, gate_ok)["verdict"]
                tally[v["verdict"].split(":")[0].split(" ")[0]] += 1
                g3 += bool(v["criteria"]["G3_pass_noninferior"])
                g5 += bool(v["criteria"]["G5_regression_no_harm"])
            oc[name] = {**{k: round(x / 200, 3) for k, x in tally.items()}, "G3_holds": round(g3 / 200, 3),
                        "G5_holds": round(g5 / 200, 3)}
        N_BOOT = saved
        res["operating_characteristics"] = oc
        # Coverage of the 90% interval with 8 clusters x 2 repeats (the design), 300 datasets each.
        cov = {}
        for eff in (-0.20, 0.0):
            hit = 0
            for i in range(300):
                fr = synth_root(t / f"cov{eff}" / str(i), ftasks, 2, eff, rng, 0.9)
                b = boot_ratio(pairs(load(fr)), "cost", n=1000, seed=i)
                hit += b["ci"][0] <= eff <= b["ci"][1]
            cov[str(eff)] = round(hit / 300, 3)
        res["ci90_coverage_8_clusters"] = cov
        # Power: how often a true -20% gives CI upper < 0 and point <= -10% (G2) in this design.
        wins = 0
        for i in range(300):
            fr = synth_root(t / "pow" / str(i), ftasks, 2, -0.20, rng, 0.9)
            b = boot_ratio(pairs(load(fr)), "cost", n=1000, seed=i)
            wins += b["ratio_minus_1"] <= CRIT["G2_usd_point_max"] and b["ci"][1] < 0
        res["power_G2_at_true_-20pct"] = round(wins / 300, 3)
    oc = res["operating_characteristics"]
    res["checks"] = {"null_false_GO_rate_le_0.10": oc["null"]["GO"] <= 0.10,
                     "harm_pass_NO-GO_ge_0.5": oc["harm_pass_-20pts"]["NO-GO"] >= 0.5,
                     "harm_regression_NO-GO_ge_0.5": oc["harm_regression_+30pct"]["NO-GO"] >= 0.5,
                     "G3_holds_under_null_ge_0.85": oc["null"]["G3_holds"] >= 0.85,
                     "G5_holds_under_null_ge_0.8": oc["null"]["G5_holds"] >= 0.8}
    ok = all(res["checks"].values())
    res["checks_pass"] = ok
    if out:
        out.write_text(json.dumps(res, indent=1) + "\n")
    return res


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--family-root", default=ROOTS["family"])
    ap.add_argument("--regression-root", default=ROOTS["regression"])
    ap.add_argument("--gate", default=ROOTS["gate"])
    ap.add_argument("--out", default="")
    ap.add_argument("--interim", action="store_true")
    ap.add_argument("--synthetic", action="store_true")
    a = ap.parse_args(argv)
    if a.synthetic:
        res = synthetic(Path(a.out) if a.out else None)
        print(json.dumps(res, indent=1))
        return 0 if res["checks_pass"] else 1
    if a.interim:
        res = interim(Path(a.family_root))
        print(json.dumps(res, indent=1))
        return 4 if res["stop_for_harm"] else 0
    res = analyse(Path(a.family_root), Path(a.regression_root), Path(a.gate))
    text = json.dumps(res, indent=1)
    if a.out:
        Path(a.out).write_text(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
