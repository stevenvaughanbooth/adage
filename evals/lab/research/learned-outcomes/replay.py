"""Downstream test: the learned outcome signal in place of the hand-coded one, scored as
`evals/replay` scores directives (Ŝ, calls/session, net of carry cost; useful rate; harm).

Held out: the RepoBench v2 **control** sessions of the replay's manifests (click, attrs,
marshmallow, vue-core = the 82 Haiku sessions of the replay baseline; plus the Sonnet 5 slices).

The scorer (`score`) is a line-by-line port of `savings_replay::score`, with Sat(d)/W(d) computed by
the hand-coded instrument (`instrument.py`), exactly as the calibrated Rust replay does. It is
validated by re-scoring the Rust replay's own U11 directives (`validate`).

Policies:
  U11 (hand-coded)   the Rust replay's own U11 directives (post-hoc and pre-emptive), re-scored.
  LEARNED            reflexes mined with the learned outcome model only, no hand-written string:
     trigger   a call whose output the learned model scores as a failure (p_fail > θ), whose
               output falls in a k-means output cluster c and whose action is similar (cos ≥ 0.6)
               to the failing actions a reflex was mined from;
     response  the action the agent took in *training* sessions to get out of that dead end: the
               first later call, within 15, with a similar intent (action cos ≥ 0.8 to the failed
               one) that the learned model scores as a success (p_fail < 0.5); kept when ≥ 2
               training sessions support it and its training usefulness rate is ≥ 0.7.
     delivery  post-hoc: after the flagged call (the hook sees the output at PostToolUse whatever
               the exit code, so piped dead ends count); pre-emptive: at PreToolUse of a call that
               would be flagged (hindsight, as the Rust replay's pre-emptive mode).
  Training pools: `train` (the repository's own training sessions, the replay's split) and
  `other-repos` (every session of the other repositories: the leave-one-repo-out reflex).
"""

from __future__ import annotations

import json
from collections import defaultdict
from pathlib import Path

import numpy as np

import features as F
import instrument as I
import load

CACHE = load.CACHE
RDIR = CACHE / "replay"
OUT = Path(__file__).resolve().parent / "results"
SLICES = ["click", "attrs", "marshmallow", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
HAIKU = SLICES[:4]
CACHE_READ = {"haiku": 0.10, "sonnet5": 0.20}  # $/M tokens, adage_core::pricing
DIRECTIVE_TOKENS = 100  # U11's delivered text is 99 tokens


# ------------------------------------------------------------------------------------------------
# Instrument: Sat / W / contradicts for a directive class (port of savings_replay::Class).
# ------------------------------------------------------------------------------------------------

def command_class(cmd: str):
    inv = I.invocation(cmd)
    if inv:
        return ("runner", inv[0], inv[1])
    head = [w for w in I.words(cmd) if not w.startswith("{")][:2]
    return ("command", " ".join(head)) if head else ("unscored",)


def sat(cl, a):
    if cl[0] == "runner":
        return a["run"] is not None and a["run"][1] == cl[2] and a["executed"]
    if cl[0] == "command":
        h = cl[1]
        return not I.failed(a) and any(s == h or s.startswith(h + " ") for s in I.segments(a["cmd"]))
    return False


def waste(cl, a):
    if cl[0] == "runner":
        return I.env_friction(a)
    if cl[0] == "command":
        return I.failed(a)
    return False


def contradicts(cl, a):
    return cl[0] == "runner" and a["run"] is not None and a["run"][1] != cl[2] and a["executed"]


def own(cl, a):
    return cl[0] == "runner" and a["run"] is not None and a["run"][1] == cl[2] and a["executed"]


def score(given, acts, passed, price):
    """given: list of (t, pre, class). Returns per-directive dicts (port of `score`)."""
    n = len(acts)
    out, windows = [], []
    for t, pre, cl in sorted(given, key=lambda g: (g[0], not g[1])):
        frm = max(t + 1, 0)
        t_star = next((k for k in range(frm, n) if sat(cl, acts[k])), None)
        superseded = any(a <= t < b for a, b in windows)
        useful = t_star is not None
        harmful = (not useful and passed is True and any(contradicts(cl, x) for x in acts[frm:])
                   and not any(own(cl, x) for x in acts[frm:]))
        saved = 0.0
        if useful and not superseded:
            saved = sum(1 for x in acts[frm:t_star] if waste(cl, x))
            if pre and t >= 0 and not sat(cl, acts[t]) and I.failed(acts[t]):
                saved += 1
            windows.append((t, t_star))
        remaining_api = max(n - frm, 0) * price["api_per_call"]
        cost = 0.0 if superseded or price["usd_per_call"] <= 0 else (
            DIRECTIVE_TOKENS * price["cache_read"] / 1e6 * remaining_api / price["usd_per_call"])
        out.append({"t": t, "pre": pre, "class": cl, "useful": useful, "harmful": harmful,
                    "superseded": superseded, "saved": saved, "cost": cost})
    return out


# ------------------------------------------------------------------------------------------------
# Data
# ------------------------------------------------------------------------------------------------

def held_out():
    """{slice: [session dict]} for the manifests' control sessions, with the table row offset."""
    a = F.table()
    first = {}
    for k, s in enumerate(a["sid"]):
        first.setdefault(s, k)
    runs = {}
    for s in load.sessions():
        if s["bench"] == "v2" and s["arm"] == "control":
            runs[(s["repo"], s["run"])] = s
    out = {}
    for sl in SLICES:
        m = json.loads((RDIR / f"{sl}.json").read_text())
        rows = []
        for sm in m["sessions"]:
            if sm.get("arm") != "control":
                continue
            s = runs.get((sl, sm["id"]))
            if s is None:
                continue
            s = dict(s)
            s["row0"] = first[s["sid"]]
            s["acts"] = [I.act(c) for c in s["calls"]]
            n = len(s["calls"])
            s["price"] = {"usd_per_call": (s["cost_usd"] or 0) / max(n, 1), "api_per_call": 1.0,
                          "cache_read": CACHE_READ[s["model"]]}
            rows.append(s)
        out[sl] = rows
    return a, out


def rust_given(sl, mode):
    d = json.loads((RDIR / f"rust-{sl}-{mode}.json").read_text())
    return {r["id"]: r for r in d["sessions"]}


def validate(a, ho):
    """Re-score the Rust replay's own U11 directives with this scorer: per-session saved must match."""
    rep = {}
    for mode in ("post-hoc", "pre-emptive"):
        diffs, n, tot_py, tot_rs = 0, 0, 0.0, 0.0
        for sl in SLICES:
            rg = rust_given(sl, mode)
            for s in ho[sl]:
                r = rg.get(s["run"])
                if r is None:
                    continue
                if r["calls"] != len(s["acts"]):
                    diffs += 1
                given = [(d["t"], d["pre"], ("runner", d["interp"], d["runner"])) for d in r["scored"]
                         if d["class"] == "runner"]
                sc = score(given, s["acts"], s["passed"], s["price"])
                py = sum(x["saved"] for x in sc if not x["superseded"])
                tot_py += py
                tot_rs += r["saved"]
                n += 1
                if abs(py - r["saved"]) > 1e-9:
                    diffs += 1
        rep[mode] = {"sessions": n, "sessions_differing": diffs, "saved_py": tot_py, "saved_rust": tot_rs}
    return rep


# ------------------------------------------------------------------------------------------------
# Learned reflexes
# ------------------------------------------------------------------------------------------------

def sessions_of(a, mask):
    sid = a["sid"]
    starts = np.r_[0, np.nonzero(sid[1:] != sid[:-1])[0] + 1, len(sid)]
    for s, e in zip(starts[:-1], starts[1:]):
        if mask[s]:
            yield s, e


def mine(a, calls_text, pfail, cluster, pool, theta, H=15, tau_int=0.8, min_sessions=2, min_rate=0.7):
    """Reflexes (trigger cluster, trigger action centroid, response command) from the pool."""
    EA = a["EA"].astype(np.float32)
    ev = defaultdict(list)  # cluster -> [(sid, trigger row, response text or None)]
    for s, e in sessions_of(a, pool):
        for i in range(s, e):
            if pfail[i] <= theta or a["tool"][i] != "Bash":
                continue
            resp = None
            for j in range(i + 1, min(e, i + 1 + H)):
                if a["tool"][j] == "Bash" and float(EA[i] @ EA[j]) >= tau_int and pfail[j] < 0.5:
                    resp = j
                    break
            ev[cluster[i]].append((a["sid"][i], i, resp))
    reflexes = []
    for c, lst in ev.items():
        with_resp = [x for x in lst if x[2] is not None]
        if not with_resp:
            continue
        # group responses by action similarity; representative = the most supported response
        best, best_sup = None, 0
        for _, _, j in with_resp:
            sup = {sid for sid, _, k in with_resp if float(EA[j] @ EA[k]) >= tau_int}
            if len(sup) > best_sup:
                best, best_sup = j, len(sup)
        rate = np.mean([x[2] is not None and float(EA[best] @ EA[x[2]]) >= tau_int for x in lst])
        sessions = len({x[0] for x in lst})
        if best_sup >= min_sessions and rate >= min_rate:
            trig = np.mean([EA[i] for _, i, _ in lst], 0)
            trig /= np.linalg.norm(trig) + 1e-9
            reflexes.append({"cluster": int(c), "trigger_action": trig, "response_row": int(best),
                             "response": calls_text[best], "support_sessions": best_sup, "rate": float(rate),
                             "trigger_sessions": sessions,
                             "example_trigger": calls_text[lst[0][1]]})
    return reflexes


def learned_given(a, s, pfail, cluster, reflexes, mode, theta, tau_trig=0.6):
    EA = a["EA"].astype(np.float32)
    by_c = defaultdict(list)
    for r in reflexes:
        by_c[r["cluster"]].append(r)
    given = []
    for k in range(len(s["acts"])):
        i = s["row0"] + k
        if pfail[i] <= theta:
            continue
        for r in by_c.get(cluster[i], []):
            if float(EA[i] @ r["trigger_action"]) >= tau_trig:
                cl = command_class(r["response"])
                if cl[0] == "unscored":
                    continue
                if mode == "pre-emptive":
                    given.append((k, True, cl))
                given.append((k, False, cl))
                break
    # at most one directive per event: pre-emptive at k supersedes the post-hoc one at k
    if mode == "pre-emptive":
        given = [g for g in given if g[1]]
    return given


# ------------------------------------------------------------------------------------------------
# Statistics
# ------------------------------------------------------------------------------------------------

def summarise(rows, seed=20260929, n_boot=2000):
    """rows: [(task, net, saved, cost, directives, useful, harmful, calls, usd_per_call)]."""
    net = np.array([r[1] for r in rows])
    tasks = np.array([r[0] for r in rows])
    ut = np.unique(tasks)
    rng = np.random.default_rng(seed)
    by = {t: np.nonzero(tasks == t)[0] for t in ut}
    bs = []
    for _ in range(n_boot):
        idx = np.concatenate([by[t] for t in rng.choice(ut, len(ut))])
        bs.append(net[idx].mean())
    d = sum(r[4] for r in rows)
    u = sum(r[5] for r in rows)
    h = sum(r[6] for r in rows)
    calls = sum(r[7] for r in rows)
    usd = np.array([r[1] * r[8] for r in rows])
    return {"sessions": len(rows), "directives": d, "useful": u, "harmful": h,
            "useful_rate": round(u / d, 3) if d else None, "useful_wilson90_lb": round(wilson_lb(u, d), 3),
            "per_10_calls": round(10 * d / max(calls, 1), 3),
            "s_hat": round(float(net.mean()), 3), "ci95": [round(float(np.percentile(bs, 2.5)), 3),
                                                          round(float(np.percentile(bs, 97.5)), 3)],
            "ci90": [round(float(np.percentile(bs, 5)), 3), round(float(np.percentile(bs, 95)), 3)],
            "usd_per_session": round(float(usd.mean()), 4),
            "calls_per_session": round(calls / max(len(rows), 1), 1)}


def wilson_lb(k, n, z=1.6448536269514722):
    if n == 0:
        return 0.0
    p = k / n
    den = 1 + z * z / n
    c = p + z * z / (2 * n)
    r = z * np.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return (c - r) / den


def session_row(s, sc):
    live = [x for x in sc if not x["superseded"]]
    saved = sum(x["saved"] for x in live)
    cost = sum(x["cost"] for x in live)
    return (s["task"], saved - cost, saved, cost, len(live), sum(x["useful"] for x in live),
            sum(x["harmful"] for x in live), len(s["acts"]), s["price"]["usd_per_call"])
