"""Oracle ceilings: what removing a whole class of calls would save, in the target metrics.

    from harness.oracle import ceiling
    ceiling(corpus.held_out(model="haiku"), lambda a, c: a.failed)

An oracle ceiling deletes every call the predicate marks, with no directive cost, p_follow = 1,
and no compensation: an upper bound for any lever whose mechanism is "the agent skips these
calls". Predicates get (features.Act, traces.Call). The built-in classes use the hand-written
instrument (`features`), which is fine for a ceiling (measurement, not a learner).
"""

from __future__ import annotations

from . import costmodel as CM
from . import stats as ST

CLASSES = {
    "failed calls (is_error or env signature)": lambda a, c: a.failed,
    "environment friction (U11's W)": lambda a, c: a.env_friction,
    "test-discovery searches": lambda a, c: a.discovery,
    "failed test-runner calls": lambda a, c: a.run is not None and a.failed,
    "every search/listing (Grep, Glob, find, grep, ls)": lambda a, c: a.tool in ("Grep", "Glob") or (
        a.tool == "Bash" and a.run is None and a.cmd.split()[:1] and a.cmd.split()[0] in ("find", "grep", "rg", "ls", "tree")),
}


def ceiling(sessions, pred, level: float = 0.90) -> dict:
    usd, base, calls, n_calls, cl = [], [], [], [], []
    for s in sessions:
        cm = CM.of(s)
        idx = [k for k, (a, c) in enumerate(zip(s.acts, s.calls)) if pred(a, c)]
        u, _ = cm.removal(idx)
        usd.append(u)
        base.append(cm.total)
        calls.append(float(len(idx)))
        n_calls.append(float(s.n_calls))
        cl.append(s.cluster)
    m_u, ci_u = ST.clustered(usd, cl, level=level)
    m_c, ci_c = ST.clustered(calls, cl, level=level)
    sh, sh_ci = ST.clustered_ratio(usd, base, cl, level=level)
    csh, csh_ci = ST.clustered_ratio(calls, n_calls, cl, level=level)
    return {"sessions": len(usd), "usd_per_task": m_u, "usd_ci": ci_u, "usd_share": sh, "usd_share_ci": sh_ci,
            "calls_per_task": m_c, "calls_ci": ci_c, "calls_share": csh, "calls_share_ci": csh_ci}


def table(sessions) -> dict:
    return {name: ceiling(sessions, p) for name, p in CLASSES.items()}
