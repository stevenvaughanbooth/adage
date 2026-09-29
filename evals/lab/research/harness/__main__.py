"""python3 -m harness <command>   (run from evals/lab/research, with nice -n 10 OMP_NUM_THREADS=1)

  corpus      what the loader sees: sessions per slice/phase/arm, duplicates, exclusions
  u11         the example policy's table (post-hoc, hindsight, predictive) → results/u11.json
  calibrate   every calibration check → results/calibration.json
  leakage     the corpus audit → results/leakage.json
  oracle      ceilings: $ and calls if whole classes of calls vanished → results/oracle.json
  parity      Python port vs `adage eval replay-savings` (needs target/debug/adage) → results/parity.json
  all         everything above, then result.json
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

from . import calibrate as K
from .drivers import curve, loro, split
from .leakage import audit
from .policies.u11 import U11
from .replay import score
from .traces import ADAGE, V1, V2, load

OUT = Path(__file__).resolve().parent / "results"
PY = ["click", "attrs", "marshmallow"]


def _dump(name, obj):
    OUT.mkdir(exist_ok=True)
    (OUT / name).write_text(json.dumps(obj, indent=1, default=str) + "\n")


def cmd_corpus(c):
    print(c.summary())


def cmd_u11(c):
    out = {}
    haiku = [s for s in c.held_out(model="haiku")]
    haiku_v2 = [s for s in haiku if s.source == "v2"]
    for label, ss in (("haiku v2 (the Rust baseline's 82 sessions)", haiku_v2), ("haiku v1+v2", haiku),
                      ("all held-out control", c.held_out())):
        for mode in ("post-hoc", "hindsight", "predictive"):
            for pf in (1.0, 0.5):
                rep = score(U11(mode, c.train()), ss, hindsight=(mode == "hindsight"), p_follow=pf)
                key = f"{label} | {mode} | p_follow={pf}"
                out[key] = rep.to_json()
                if pf == 1.0 or mode == "predictive":
                    print(key)
                    print(rep.headline())
                    print()
    # Protocol drivers with the predictive variant (the only one that learns: which spellings fail).
    fit = lambda train: U11("predictive", train)  # noqa: E731
    for name, rep in (("split", split(fit, c, model="haiku")), ("loro", loro(fit, c, model="haiku"))):
        out[f"driver {name}"] = rep.to_json()
        print(f"driver {name}:")
        print(rep.headline())
        print()
    cv = curve(fit, c, sizes=(0, 1, 2, 4, 10), model="haiku")
    out["curve"] = {n: {"calls": r.summary()["calls_saved_per_task"], "calls_ci": r.summary()["calls_saved_ci"],
                        "usd": r.summary()["usd_saved_per_task"], "usd_ci": r.summary()["usd_saved_ci"]} for n, r in cv.items()}
    print("learning curve (predictive U11):", {n: round(v["calls"], 2) for n, v in out["curve"].items()})
    _dump("u11.json", out)
    return out


def cmd_calibrate(c):
    r = K.run_all(c)
    _dump("calibration.json", r)
    m = r["mechanism"]
    print(f"mechanism: live {m['live']['saving']:.2f} {m['live']['ci95']}, replay post-hoc {m['post-hoc']['predicted']:.2f} "
          f"{m['post-hoc']['ci95']} inside={m['post-hoc']['inside_live_ci']}")
    print("residual (adage arm):", r["residual"])
    print("delivery match:", {k: v for k, v in r["delivery_match"].items() if k != "differences"})
    print("cost model:", r["cost_model"])
    print("A/A coverage (v1):", r["aa_coverage"])
    print("DiD:", r["did"])
    return r


def cmd_leakage(c):
    a = audit(c)
    _dump("leakage.json", a)
    print(json.dumps({k: v for k, v in a.items() if k not in ("splits",)}, indent=1, default=str)[:3000])
    return a


def cmd_parity(c):
    """Replay U11 with the Rust binary and compare per session with the Python port."""
    adage = ADAGE / "target" / "debug" / "adage"
    if not adage.exists():
        print("no target/debug/adage: skipped")
        return {}
    res = {}
    with tempfile.TemporaryDirectory() as d:
        subprocess.run([sys.executable, str(ADAGE / "evals" / "replay" / "manifest.py"),
                        *[str(V2 / r) for r in ("click", "attrs", "marshmallow", "vue-core", "click-sonnet5", "vue-core-sonnet5")],
                        "--out", d], check=True, capture_output=True)
        for mode, rmode in (("post-hoc", "post-hoc"), ("hindsight", "pre-emptive")):
            for sl in ("click", "attrs", "marshmallow", "vue-core", "click-sonnet5", "vue-core-sonnet5"):
                ref = json.loads(subprocess.run([str(adage), "eval", "replay-savings", f"{d}/{sl}.json", "--arm", "control",
                                                 "--policy", "instincts", "--only", "U11", "--mode", rmode, "--json"],
                                                capture_output=True, text=True, check=True).stdout)
                ss = [s for s in c.held_out(slice=sl) if s.source == "v2"]
                rep = score(U11(mode), ss, hindsight=(mode == "hindsight"))
                mine = {r.run: [(x.t, x.pre, x.saved, round(x.cost, 6)) for x in r.scored] for r in rep.rows}
                bad = [rs["id"] for rs in ref["sessions"]
                       if mine.get(rs["id"]) != [(x["t"], x["pre"], x["saved"], round(x["cost"], 6)) for x in rs["scored"]]]
                s = rep.summary()
                res[f"{sl} {rmode}"] = {"sessions": len(ref["sessions"]), "rust": ref["summary"]["s_hat"],
                                        "rust_ci": ref["summary"]["s_hat_ci"], "python": round(s["s_hat_rust"], 4),
                                        "python_ci": [round(x, 4) for x in s["s_hat_rust_ci95"]], "mismatched_sessions": bad}
                print(sl, rmode, res[f"{sl} {rmode}"])
    _dump("parity.json", res)
    return res


def cmd_oracle(c):
    from .oracle import table
    t = table([s for s in c.held_out(model="haiku") if s.source == "v2"])
    _dump("oracle.json", t)
    for k, v in t.items():
        print(f"{k:<52} calls {v['calls_per_task']:.2f} ({100 * v['calls_share']:.1f}%)  $ {v['usd_per_task']:.4f} "
              f"({100 * v['usd_share']:.1f}% [{100 * v['usd_share_ci'][0]:.1f}, {100 * v['usd_share_ci'][1]:.1f}])")
    return t


def main(argv=None):
    argv = argv or sys.argv[1:]
    cmd = argv[0] if argv else "corpus"
    c = load()
    if cmd == "all":
        for f in (cmd_corpus, cmd_leakage, cmd_parity, cmd_calibrate, cmd_oracle, cmd_u11):
            f(c)
        return 0
    f = globals().get(f"cmd_{cmd}")
    if f is None:
        print(__doc__)
        return 2
    f(c)
    return 0


if __name__ == "__main__":
    sys.exit(main())
