"""Scoring: next-action top-1 accuracy and useful-directive precision / recall / turns saved.

Useful directive (the lab's definition): at decision point i a directive names action `a` (at its
`directive_key` level). It is

- **useful** if the agent later took `a` (first occurrence t' >= i) and the steps in [i, t') hold at
  least one failed command or at least two exploratory steps: the directive would have saved
  those turns;
- **redundant** if the agent took `a` with no waste in between (correct, saves nothing);
- **wrong** if the agent never took `a` in the rest of the session.

A directive is issued at most once per session per action (the hook would not repeat itself).
Turns saved in a session: the union, over its useful directives, of the wasted steps (failed or
exploratory) in each [i, t') window; an upper bound, since some of those reads may have been
needed for something else. `saved_fail` counts only failed commands in those windows (the
conservative estimate), and `saved_discovery` failed commands plus the exploratory steps that were
about *how to work in this repository* rather than the bug: reading manifests and tool config
(package.json, pyproject.toml, setup.cfg, tox.ini, Makefile, go.mod, conftest.py, *.config.*),
probing the environment (which, pip list/show, ls .venv, --version). `precision_strict` counts a
directive as useful only when a failed command sat in its window (a dead end it would have headed
off).

Opportunities (for recall): the distinct directive-eligible actions of a session for which some
occurrence was preceded, since the session start or that action's previous occurrence, by a failed
command or two exploratory steps. Recall = useful distinct actions / opportunities.
"""

from __future__ import annotations

import collections
import re

from predict import LEVELS


DISCOVERY = re.compile(r"(package\.json|pyproject\.toml|setup\.cfg|setup\.py|tox\.ini|noxfile|Makefile|go\.mod|"
                       r"conftest\.py|\.config\.[jt]s|requirements|CONTRIBUTING|\.venv|\bwhich\b|pip (list|show|freeze)|"
                       r"--version|uv\.lock|pnpm-workspace|\.github/workflows)")


def discovery(st) -> bool:
    return st.failed or (st.action.explore and bool(DISCOVERY.search(st.action.full + " " + st.action.text)))


def wasted(st) -> bool:
    return st.failed or st.action.explore


def window_waste(steps, i, t):
    idx = [j for j in range(i, t) if wasted(steps[j])]
    fails = [j for j in idx if steps[j].failed]
    explore = [j for j in idx if steps[j].action.explore]
    return idx, fails, len(fails) >= 1 or len(explore) >= 2


def opportunities(session) -> set:
    steps = session.steps
    out, last = set(), {}
    for t, st in enumerate(steps):
        a = st.action
        if not a.directive_eligible:
            continue
        k = a.directive_key
        start = last.get(k, 0)
        _, _, ok = window_waste(steps, start, t)
        if ok:
            out.add(k)
        last[k] = t + 1
    return out


def accuracy(sits, preds, only=None) -> dict:
    hit = collections.Counter()
    n = 0
    for s, (pred, _) in zip(sits, preds):
        steps = s.session.steps
        if s.i >= len(steps):
            continue
        a = steps[s.i].action
        if only and not only(a):
            continue
        n += 1
        for lv in LEVELS:
            hit[lv] += pred.get(lv) == getattr(a, lv)
    return {"n": n, **{lv: hit[lv] / n if n else 0.0 for lv in LEVELS}}


def directives(sits, preds, theta: float) -> dict:
    """Issue directives at confidence >= theta and score them."""
    by_sess = collections.defaultdict(list)
    for s, (_, cand) in zip(sits, preds):
        by_sess[s.session.sid].append((s, cand))
    tot = collections.Counter()
    per_session_saved = []
    examples = []
    for sid, rows in by_sess.items():
        session = rows[0][0].session
        steps = session.steps
        issued = set()
        saved, saved_fail, saved_disc, useful_keys = set(), set(), set(), set()
        for s, cand in rows:
            if not cand or cand[1] < theta or cand[0] in issued:
                continue
            key = cand[0]
            issued.add(key)
            tot["issued"] += 1
            kt = "run" if key.startswith("Bash:") else key.split(" ")[0].lower()
            tot[f"issued:{kt}"] += 1
            t = next((j for j in range(s.i, len(steps)) if steps[j].action.directive_eligible
                      and steps[j].action.directive_key == key), None)
            if t is None:
                tot["wrong"] += 1
                kind = "wrong"
            else:
                idx, fails, ok = window_waste(steps, s.i, t)
                if ok:
                    tot["useful"] += 1
                    tot[f"useful:{kt}"] += 1
                    useful_keys.add(key)
                    saved.update(idx)
                    saved_fail.update(fails)
                    saved_disc.update(j for j in idx if discovery(steps[j]))
                    if fails:
                        tot["useful_strict"] += 1
                    kind = "useful"
                else:
                    tot["redundant"] += 1
                    kind = "redundant"
            if len(examples) < 400:
                examples.append({"session": sid[:8], "at": s.i, "directive": key, "conf": round(cand[1], 3),
                                 "support": cand[2], "kind": kind})
        opp = opportunities(session)
        tot["opportunities"] += len(opp)
        tot["hit_opportunities"] += len(useful_keys & opp)
        tot["saved"] += len(saved)
        tot["saved_fail"] += len(saved_fail)
        tot["saved_disc"] += len(saved_disc)
        tot["steps"] += len(steps)
        tot["sessions"] += 1
        per_session_saved.append(len(saved))
    n = tot["issued"]
    return {
        "theta": theta,
        "sessions": tot["sessions"],
        "issued": n,
        "issued_per_session": n / max(tot["sessions"], 1),
        "useful": tot["useful"],
        "redundant": tot["redundant"],
        "wrong": tot["wrong"],
        "precision": tot["useful"] / n if n else None,
        "correct_rate": (tot["useful"] + tot["redundant"]) / n if n else None,
        "recall": tot["hit_opportunities"] / tot["opportunities"] if tot["opportunities"] else 0.0,
        "saved_per_session": tot["saved"] / max(tot["sessions"], 1),
        "saved_fail_per_session": tot["saved_fail"] / max(tot["sessions"], 1),
        "saved_discovery_per_session": tot["saved_disc"] / max(tot["sessions"], 1),
        "precision_strict": tot["useful_strict"] / n if n else None,
        "steps_per_session": tot["steps"] / max(tot["sessions"], 1),
        "by_kind": {k.split(":", 1)[1]: {"issued": v, "useful": tot["useful:" + k.split(":", 1)[1]],
                                         "precision": tot["useful:" + k.split(":", 1)[1]] / v}
                    for k, v in tot.items() if k.startswith("issued:")},
        "examples": examples,
    }


def sweep(sits, preds, thetas) -> list[dict]:
    return [directives(sits, preds, t) for t in thetas]


def operating_point(curve: list[dict], target: float = 0.7, min_issued: int = 10) -> dict | None:
    """The lowest threshold (most directives) whose precision is >= target."""
    ok = [c for c in curve if c["precision"] is not None and c["precision"] >= target and c["issued"] >= min_issued]
    return min(ok, key=lambda c: c["theta"]) if ok else None
