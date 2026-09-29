"""Obligation-reflex uplift, instrumental-variable check, per-repo learnability and power.

1. Obligation uplift. For a behaviour X ("before finishing, do X"), the pass-rate uplift if Adage
   made X happen in every session where it did not happen naturally is
       U = P(not X) x RD_within(X) x p_follow,
   with RD_within the task-stratified MH risk difference (task x model x bench strata) and
   p_follow = 1 (an upper bound: v2 measured 41/41 for U11). P(not X) is taken on v2 control
   sessions (the product-relevant population); RD on all eval sessions. Joint task-cluster
   bootstrap, 90%.
2. IV check. The arms were randomised. If an arm shifted X, the Wald ratio dPass/dX bounds X's
   causal effect without the observational confounding of (1).
3. Per-repo learnability: mixed strata per repo and the CI half-width of RD for the prespecified
   behaviours using only that repo's sessions.
4. Power: sessions per arm to detect each uplift, unstratified and task-stratified.
"""

from __future__ import annotations

import json
import math
from collections import Counter, defaultdict

import numpy as np

import clusters as CL
from levers import gold, hyp_features, stratum
from load import load
from stats_util import boot_mh, mh_rd, n_per_arm, n_per_arm_stratified

OBLIG = [
    "H1 test run after the last edit (before finishing)",
    "H1b last test run after the last edit exited 0",
    "H2b targeted test run after the last edit [gold instrument]",
    "H3b ad-hoc script after the last edit",
    "H4 kept going after the first green test run",
    "H5b broad suite after the last edit [hand instrument]",
    "E0 made at least one edit (learned cluster k12:c3 / k40:c2)",
]


def main():
    ALL = [s for s in load() if s["phase"] == "eval"]
    cnt = Counter(stratum(s) for s in ALL)
    S = [s for s in ALL if cnt[stratum(s)] >= 2]
    H, mapping = hyp_features(S, gold())
    L = load()
    lab40 = CL.assign(L, CL.fit(L, 40))
    edit40 = [i for i, n in enumerate(CL.names(L, lab40, 40)) if n.startswith("Edit")]
    mapping["edit_clusters_k40"] = edit40
    for s in S:
        H[s["sid"]]["E0 made at least one edit (learned cluster k12:c3 / k40:c2)"] = int(
            any(c in edit40 for c in lab40[s["sid"]]))
    out = {"mapping": mapping, "obligation": {}, "iv": {}, "per_repo": {}, "power": {}}

    rng = np.random.default_rng(7)
    tasks = sorted({s["task"] for s in S})
    bytask = defaultdict(list)
    for s in S:
        bytask[s["task"]].append(s)
    v2c = [s for s in S if s["bench"] == "v2" and s["arm"] == "control"]
    v2c_ids = {s["sid"] for s in v2c}
    sig_w = within_var(S)
    p0 = np.mean([s["passed"] for s in v2c])

    v1c_ids = {s["sid"] for s in S if s["bench"] == "v1" and s["arm"] == "control"}
    for key in OBLIG:
        rows = [(stratum(s), H[s["sid"]][key], int(s["passed"])) for s in S]
        rd, _ = mh_rd(rows)
        nc = [s for s in S if not s["censored"]]
        rd_nc, _ = mh_rd([(stratum(s), H[s["sid"]][key], int(s["passed"])) for s in nc])
        entry = dict(rd_within=round(rd, 3), rd_within_uncensored=round(rd_nc, 3))
        for pop, ids in (("v2_control_60turns", v2c_ids), ("v1_control_30turns", v1c_ids)):
            pnot = 1 - np.mean([H[x][key] for x in ids if x in H])
            bs = []
            r2 = np.random.default_rng(7)
            for _ in range(2000):
                pick = [tasks[i] for i in r2.integers(0, len(tasks), len(tasks))]
                rr, px = [], []
                for j, t in enumerate(pick):
                    for s in bytask[t]:
                        rr.append(((j,) + stratum(s), H[s["sid"]][key], int(s["passed"])))
                        if s["sid"] in ids:
                            px.append(H[s["sid"]][key])
                e, _ = mh_rd(rr)
                if not math.isnan(e) and px:
                    bs.append((1 - np.mean(px)) * e)
            u = pnot * rd
            lo, hi = np.quantile(bs, [0.05, 0.95])
            entry[pop] = dict(
                p_not_x=round(pnot, 3), uplift_pp=round(100 * u, 2), uplift_ci90_pp=[round(100 * lo, 2), round(100 * hi, 2)],
                sessions_per_arm_point=(n_per_arm(p0, u) if u > 0.001 else None),
                sessions_per_arm_upper=(n_per_arm(p0, hi) if hi > 0.001 else None),
                stratified_sessions_per_arm_upper=(n_per_arm_stratified(sig_w, hi) if hi > 0.001 else None))
            print(f"{key[:55]:55} {pop[:2]} P(¬X)={pnot:.2f} RD={rd:+.3f} (uncens {rd_nc:+.3f}) U={100*u:+.2f}pp "
                  f"[{100*lo:+.2f}, {100*hi:+.2f}]")
        out["obligation"][key] = entry

    # IV: randomised arms as instruments for X
    for bench, arm in (("v1", "agents-md"), ("v1", "adage"), ("v2", "adage")):
        sub = [s for s in S if s["bench"] == bench and s["arm"] in ("control", arm)]
        z = lambda s: int(s["arm"] == arm)
        res = {}
        for key in OBLIG + ["H2 ran the task's existing test module (targeted) [gold instrument]",
                            "H3 ad-hoc reproduction script (python -c / heredoc / script) "]:
            dx, lx, hx, _ = boot_mh([((s["task"], s["model"]), z(s), H[s["sid"]][key]) for s in sub],
                                    cluster_of=lambda r: r[0][0], B=1000)
            res[key] = dict(dX=round(dx, 3), dX_ci90=[round(lx, 3), round(hx, 3)])
        dy, ly, hy, _ = boot_mh([((s["task"], s["model"]), z(s), int(s["passed"])) for s in sub],
                                cluster_of=lambda r: r[0][0], B=1000)
        out["iv"][f"{bench}:{arm}"] = dict(dPass=round(dy, 3), dPass_ci90=[round(ly, 3), round(hy, 3)], dX=res)
        print(bench, arm, "dPass", round(dy, 3), [round(ly, 3), round(hy, 3)])
        for k, v in res.items():
            if abs(v["dX"]) >= 0.05:
                print("   ", k[:60], v)

    # per repo learnability
    for r in sorted({s["repo"] for s in S}):
        sub = [s for s in S if s["repo"] == r]
        by = defaultdict(list)
        for s in sub:
            by[stratum(s)].append(s["passed"])
        mixed = [k for k, v in by.items() if 0 < sum(v) < len(v)]
        widths = {}
        for key in OBLIG[:2]:
            e, lo, hi, w = boot_mh([(("r", stratum(s)), H[s["sid"]][key], int(s["passed"])) for s in sub],
                                   cluster_of=lambda q: q[0][1][0], B=1000)
            widths[key] = dict(rd=round(e, 3), ci90=[round(lo, 3), round(hi, 3)])
        out["per_repo"][r] = dict(sessions=len(sub), strata=len(by), mixed_strata=len(mixed),
                                  sessions_in_mixed=sum(len(by[k]) for k in mixed), rd=widths)
        print(r, out["per_repo"][r])

    # power for arm-level effects
    out["power"] = dict(
        p0_v2_control=round(float(p0), 3), sigma2_within=round(sig_w, 4),
        table=[dict(delta_pp=d, unstratified_per_arm=n_per_arm(p0, d / 100),
                    task_stratified_per_arm=n_per_arm_stratified(sig_w, d / 100)) for d in (2, 3, 5, 10, 15)])
    print(json.dumps(out["power"]))
    json.dump(out, open("report.json", "w"), indent=1, default=float)


def within_var(S):
    by = defaultdict(list)
    for s in S:
        by[stratum(s)].append(s["passed"])
    n = sum(len(v) for v in by.values())
    return float(sum(np.var(v) * len(v) for v in by.values()) / n)


if __name__ == "__main__":
    main()
