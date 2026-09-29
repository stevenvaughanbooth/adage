"""Measurement instrument (hand-written parts live here and only here: the lab's exploratory-step
flag and discovery regex from `metrics.py`, and "edit tool" from `normalize.py`).

Run directive d = (i, key) delivered before call i ("do `key`"):
  t1 = first call >= i with that key (any outcome); the key must succeed at some call >= t1
       (the command works here), else the directive is WRONG.
  window [i, t1): the calls a skip-ahead would have jumped.
  USEFUL if the window holds a failed call or >= 2 exploratory steps (the lab's definition), else
  REDUNDANT (correct, saves nothing).
  calls saved (Ŝ "other command" rule, tightened): failed COMMANDS in the window (failed calls
  that are directive-eligible and not edits: a failed search or a failed Edit is not something a
  run directive heads off).
  discovery saved (lab secondary): failed calls + exploratory steps about how to work here.
Avoid directive d = (i, key) ("`key` fails here"):
  first call >= i with that key failed -> USEFUL; saved = failed calls of that key from i until
  it first succeeds; succeeded -> HARMFUL (it would have steered away from a working command);
  never taken -> UNUSED.
Per session, savings are the union of windows (a later directive inside an earlier useful one's
window is superseded). Harm-adjusted (the headline): each WRONG or HARMFUL directive, if
followed, costs one call. $ saved = sum of the saved calls' marginal $ (data.py) minus each
directive's carrying cost C(d) = tokens(d) x cache-read rate x API turns left.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent))
from metrics import discovery  # noqa: E402
from data import rates  # noqa: E402

WRAP_TOKENS = 30  # the hook's wrapper around a directive (render::wrap_directive), approx.


def dtokens(C, kid: int) -> int:
    return WRAP_TOKENS + len(C.keys[kid]) // 4 + 8


class Scorer:
    def __init__(self, C):
        self.C = C
        self.cache = {}

    def run_outcome(self, s: int, i: int, kid: int):
        key = (s, i, kid, "run")
        if key in self.cache:
            return self.cache[key]
        C = self.C
        kk, su = C.kid[s], C.succ[s]
        steps = C.sessions[s].steps
        n = len(kk)
        t1 = next((t for t in range(i, n) if kk[t] == kid), None)
        if t1 is None or not su[t1:][kk[t1:] == kid].any():
            r = ("wrong", ())
        else:
            win = range(i, t1)
            fails = [j for j in win if steps[j].failed and steps[j].action.directive_eligible
                     and not steps[j].action.edit]
            nexp = sum(steps[j].action.explore for j in win)
            if fails or nexp >= 2:
                disc = [j for j in win if discovery(steps[j]) and not steps[j].action.edit]
                loose = [j for j in win if (steps[j].failed or steps[j].action.explore) and not steps[j].action.edit]
                r = ("useful", tuple(fails), tuple(disc), bool(fails), tuple(loose))
            else:
                r = ("redundant", ())
        self.cache[key] = r
        return r

    def avoid_outcome(self, s: int, i: int, kid: int):
        C = self.C
        kk, su = C.kid[s], C.succ[s]
        occ = [t for t in range(i, len(kk)) if kk[t] == kid]
        if not occ:
            return ("unused", ())
        if su[occ[0]]:
            return ("harmful", ())
        f = []
        for t in occ:
            if su[t]:
                break
            f.append(t)
        return ("useful", tuple(f), tuple(f), True, tuple(f))

    def cost_usd(self, s: int, i: int, tokens: int) -> float:
        sess = self.C.sessions[s]
        rr = rates(sess.model)[3]
        # API turns left ~ (calls left) x (API turns per call)
        left = (len(sess.steps) - i) * sess.api_turns / max(len(sess.steps), 1)
        return tokens * rr * left / 1e6

    def session(self, s: int, directives):
        """directives: [(i, kid, kind, tokens)] kind in run|avoid. Returns per-session stats."""
        C = self.C
        steps = C.sessions[s].steps
        saved, disc, loose = set(), set(), set()
        st = {"issued": 0, "useful": 0, "strict": 0, "redundant": 0, "wrong": 0, "harmful": 0, "unused": 0,
              "cost_usd": 0.0, "cost_calls": 0.0}
        covered_until = -1
        per_call = C.sessions[s].usd_total / max(len(steps), 1)
        for i, kid, kind, tok in sorted(directives):
            if i < covered_until:
                continue  # superseded: the agent would already be past this point
            st["issued"] += 1
            c = self.cost_usd(s, i, tok)
            st["cost_usd"] += c
            st["cost_calls"] += c / max(per_call, 1e-9)
            r = self.run_outcome(s, i, kid) if kind == "run" else self.avoid_outcome(s, i, kid)
            st[r[0]] += 1
            if r[0] == "useful":
                saved.update(r[1])
                disc.update(r[2])
                loose.update(r[4])
                st["strict"] += r[3]
                if kind == "run":
                    t1 = next(t for t in range(i, len(steps)) if C.kid[s][t] == kid)
                    covered_until = max(covered_until, t1)
        st["calls_saved"] = len(saved)
        st["disc_saved"] = len(disc | saved)
        st["usd_saved"] = sum(steps[j].usd for j in saved)
        st["net_calls"] = st["calls_saved"] - st["cost_calls"]
        st["net_usd"] = st["usd_saved"] - st["cost_usd"]
        # harm: a followed directive that was wrong (run: the session never ran that command
        # successfully) or harmful (avoid: it steered away from a command that worked) costs one call
        st["harm_calls"] = st["wrong"] + st["harmful"]
        st["net_harm"] = st["net_calls"] - st["harm_calls"]
        st["net_usd_harm"] = st["net_usd"] - st["harm_calls"] * per_call
        st["loose_saved"] = len(loose | saved)
        st["calls"] = len(steps)
        st["usd"] = C.sessions[s].usd_total
        return st


def aggregate(rows: list[dict], boot: int = 2000, seed: int = 7) -> dict:
    """Pooled stats; 90% percentile bootstrap over sessions for per-session means; Wilson 90% for
    precision."""
    if not rows:
        return {}
    keys = ["calls_saved", "disc_saved", "loose_saved", "net_calls", "net_harm", "usd_saved", "net_usd",
            "net_usd_harm", "issued", "useful", "calls", "usd"]
    A = {k: np.array([r[k] for r in rows], float) for k in keys}
    n = len(rows)
    rng = np.random.default_rng(seed)
    B = rng.integers(0, n, (boot, n))
    out = {"sessions": n}
    for k in ("calls_saved", "disc_saved", "loose_saved", "net_calls", "net_harm", "net_usd", "net_usd_harm",
              "issued", "useful"):
        m = A[k][B].mean(1)
        out[k] = [float(A[k].mean()), float(np.percentile(m, 5)), float(np.percentile(m, 95))]
    # relative: ratio of means, bootstrapped
    for num, den, name in (("net_calls", "calls", "pct_calls"), ("net_usd", "usd", "pct_usd"),
                           ("net_harm", "calls", "pct_calls_harm"), ("net_usd_harm", "usd", "pct_usd_harm")):
        r = A[num][B].sum(1) / A[den][B].sum(1)
        out[name] = [float(A[num].sum() / A[den].sum()), float(np.percentile(r, 5)), float(np.percentile(r, 95))]
    iss = sum(r["issued"] for r in rows)
    use = sum(r["useful"] for r in rows)
    out["issued_total"] = iss
    out["precision"] = [use / iss if iss else None, *wilson(use, iss)]
    out["strict_precision"] = sum(r["strict"] for r in rows) / iss if iss else None
    out["harmful_rate"] = sum(r["harmful"] + r["wrong"] for r in rows) / iss if iss else None
    return out


def wilson(k: int, n: int, z: float = 1.645):
    if n == 0:
        return (None, None)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * ((p * (1 - p) / n + z * z / (4 * n * n)) ** 0.5) / d
    return (c - h, c + h)
