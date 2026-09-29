"""Assemble result.json: paired comparisons, uptake-calibrated estimates, LORO table."""
import json, pickle
from collections import defaultdict
import numpy as np
import kd_load, os
RES = "results/strict" if os.environ.get("KD_STRICT") == "1" else "results"
from stats import boot
import calibrate

P = pickle.load(open(kd_load.CACHE / ("per-strict.pkl" if os.environ.get("KD_STRICT") == "1" else "per.pkl"), "rb"))
H = json.load(open(RES + "/heldout.json"))
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
S = kd_load.load()
ctrl = {r: [s for s in S if s.repo == r and s.arm == "control"] for r in REPOS}


def rows(meth):
    out = []
    for r in REPOS:
        p = H["chosen"].get(meth, {}).get(r)
        key = f"{r}|{meth}|{tuple(p) if p else ()}"
        for s, x in zip(ctrl[r], P["per"][key]):
            out.append(dict(x, repo=r, cost=s.cost, n=len(s.calls)))
    return out


def paired(a, b, key):
    ra, rb = rows(a), rows(b)
    d = [x[key] - y[key] for x, y in zip(ra, rb)]
    m, ci = boot(d, [x["task"] for x in ra])
    return dict(diff=round(m, 4), ci90=[round(ci[0], 4), round(ci[1], 4)])


def frac(meth, key="usd"):
    r = rows(meth)
    m, ci = boot(np.array([x[key] for x in r]) / np.mean([x["cost"] for x in r]), [x["task"] for x in r])
    return round(m, 4), [round(ci[0], 4), round(ci[1], 4)]


# joint bootstrap: uptake (calibration pairs, by task) x savings (held-out sessions, by task)
def calibrated(meth, seed=11, n=4000):
    cal = calibrate.rows
    cg = defaultdict(list)
    for i, x in enumerate(cal):
        cg[x["task"]].append(i)
    cidx = list(cg.values())
    ctl = np.array([x["control_cov"] for x in cal], float)
    amd = np.array([x["agents-md_cov"] for x in cal], float)
    R = rows(meth)
    gross = np.array([x["usd"] + x["carry_usd"] for x in R])
    car = np.array([x["carry_usd"] for x in R])
    obv = np.array([x["calls"] for x in R], float)
    cost = np.array([x["cost"] for x in R])
    uc = cost / np.array([x["n"] for x in R])
    hg = defaultdict(list)
    for i, x in enumerate(R):
        hg[x["task"]].append(i)
    hidx = list(hg.values())
    rng = np.random.default_rng(seed)
    out = []
    for _ in range(n):
        cs = np.concatenate([cidx[p] for p in rng.integers(0, len(cidx), len(cidx))])
        u = 1 - amd[cs].sum() / ctl[cs].sum()
        hs = np.concatenate([hidx[p] for p in rng.integers(0, len(hidx), len(hidx))])
        usd = (u * gross[hs] - car[hs]).mean()
        out.append((u, usd, usd / cost[hs].mean(), (u * obv[hs] - car[hs] / uc[hs]).mean()))
    out = np.array(out)
    u0 = 1 - amd.sum() / ctl.sum()
    pt = ((u0 * gross - car).mean(), (u0 * gross - car).mean() / cost.mean(), (u0 * obv - car / uc).mean())
    q = lambda j: [round(float(np.percentile(out[:, j], 5)), 4), round(float(np.percentile(out[:, j], 95)), 4)]
    return dict(uptake=round(u0, 3), uptake_ci90=q(0), usd_net=round(pt[0], 4), usd_ci90=q(1), frac_usd=round(pt[1], 4),
                frac_ci90=q(2), calls_net=round(pt[2], 3), calls_ci90=q(3))


if __name__ == "__main__":
    out = {}
    for m in ["card", "ctx-sel-pre", "ctx-pre", "agents-md", "filemap", "agents-md+filemap", "card+filemap", "oracle-train", "oracle-all"]:
        out[m] = dict(frac_usd=frac(m), **{k: H["pooled"][m][k] for k in ("calls", "calls_net", "usd")})
    out["paired"] = {f"card-vs-{b}": {k: paired("card", b, k) for k in ("calls", "calls_net", "usd", "frac_usd")}
                     for b in ("agents-md", "filemap", "ctx-sel-pre", "agents-md+filemap")}
    out["calibrated"] = {m: calibrated(m) for m in ("card", "agents-md", "filemap", "ctx-sel-pre", "card+filemap")}
    json.dump(out, open(RES + "/summary.json", "w"), indent=1)
    print(json.dumps(out, indent=1))
