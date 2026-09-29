"""Downstream table: learned dead-end reflexes vs U11, scored by the ported replay (replay.py).

  python3 downstream.py   (after evaluate.py) -> results/downstream.json
"""

from __future__ import annotations

import json

import numpy as np

import features as F
import replay as R

THETA = 0.5


def main():
    a, ho = R.held_out()
    calls_text = [c["arg"] for s in F.load.sessions() for c in s["calls"]]
    pf_main = np.load(R.CACHE / "pfail2_main.npy")
    cl_main = np.load(R.CACHE / "cluster2_main.npy")
    base = {sl: sl.replace("-sonnet5", "") for sl in R.SLICES}
    reflex_log, rows = {}, {}
    for sl in R.SLICES:
        br = base[sl]
        pf_lo = np.load(R.CACHE / f"pfail2_loro-{br}.npy")
        cl_lo = np.load(R.CACHE / f"cluster2_loro-{br}.npy")
        pools = {
            "train": (pf_main, cl_main, (a["base_repo"] == br) & (a["arm"] == "train")),
            "other-repos": (pf_lo, cl_lo, a["base_repo"] != br),
        }
        refl = {k: R.mine(a, calls_text, pf, cl, pool, THETA) for k, (pf, cl, pool) in pools.items()}
        reflex_log[sl] = {k: [{x: r[x] for x in ("cluster", "response", "support_sessions", "rate",
                                                 "trigger_sessions", "example_trigger")} for r in v]
                          for k, v in refl.items()}
        ru = {m: R.rust_given(sl, m) for m in ("post-hoc", "pre-emptive")}
        for s in ho[sl]:
            u11 = {m: [(d["t"], d["pre"], ("runner", d["interp"], d["runner"])) for d in ru[m][s["run"]]["scored"]
                       if d["class"] == "runner"] for m in ru}
            pol = {"U11 post-hoc (hand-coded, as live)": u11["post-hoc"],
                   "U11 pre-emptive (hand-coded)": u11["pre-emptive"]}
            for k, (pf, cl, _) in pools.items():
                for m in ("post-hoc", "pre-emptive"):
                    pol[f"learned {m}, reflexes from {k}"] = R.learned_given(a, s, pf, cl, refl[k], m, THETA)
            pol["U11 pre-emptive + learned pre-emptive (train)"] = (
                u11["pre-emptive"] + [g for g in pol["learned pre-emptive, reflexes from train"]
                                      if g[0] not in {x[0] for x in u11["pre-emptive"]}])
            pol["U11 post-hoc + learned post-hoc (train)"] = (
                u11["post-hoc"] + [g for g in pol["learned post-hoc, reflexes from train"]
                                   if g[0] not in {x[0] for x in u11["post-hoc"]}])
            for k, given in pol.items():
                sc = R.score(given, s["acts"], s["passed"], s["price"])
                rows.setdefault(k, {}).setdefault(sl, []).append(R.session_row(s, sc))
    table = {}
    for k, by in rows.items():
        table[k] = {sl: R.summarise(v) for sl, v in by.items()}
        table[k]["pooled haiku"] = R.summarise([r for sl in R.HAIKU for r in by[sl]])
        table[k]["pooled sonnet5"] = R.summarise([r for sl in R.SLICES[4:] for r in by[sl]])
    res = {"table": table, "reflexes": reflex_log, "validate": R.validate(a, ho)}
    (R.OUT / "downstream.json").write_text(json.dumps(res, indent=1, default=str))
    for k, v in table.items():
        p = v["pooled haiku"]
        q = v["pooled sonnet5"]
        print(f"{k:55s} haiku {p['s_hat']:+.2f} {p['ci95']} d={p['directives']} u={p['useful']} h={p['harmful']} | "
              f"sonnet {q['s_hat']:+.2f} {q['ci95']} d={q['directives']} u={q['useful']}")


if __name__ == "__main__":
    main()
