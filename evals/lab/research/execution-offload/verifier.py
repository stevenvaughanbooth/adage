"""The hook runs the tests after each edit and hands the agent the result (the "test-after-edit"
offload), plus the trajectory question: would earlier failure detection shorten dead ends?

Policy (learned, generic):
  - a command template's *verifier score* is learned from the pool: P(a call of this template
    comes within W calls after an edit) (learner.Pool.tpl_after), and in-session evidence;
  - the session's verifier V is the agent's own most recent non-inline command whose template
    score ≥ τ (τ learned by cross-fitting on the pool). No verifier until the agent has run one;
  - after every model turn that edited a file, the hook runs V and delivers its result: in full,
    or compacted to its last line when V exits 0 ("compact" variant);
  - optionally gated by a learned P(the agent runs a test before its next edit | this point)
    (a logistic model on generic features) so the hook does not run tests mid-way through a
    multi-edit change.

Scoring (harness cost model; HAND-WRITTEN INSTRUMENT used for *measurement only*: the harness's
`features.Act.run/targets` decide whether the agent's own next test run is covered by V):
  exact   the agent's next call before its next edit that is a test run has V's key
  cover   ...has V's runner and its test files ⊆ V's test files (selectors ignored), or V's key
A covered run's turn is removed (p_skip), V's result is injected after the edit turn. A delivery
with no covered run before the next edit is a miss: its result is carried unused.
"""
from __future__ import annotations

import json
import sys
from collections import defaultdict

import numpy as np
from sklearn.linear_model import LogisticRegression

from data import corpus, records
from learner import W, Pool, authoring, inline_script, template
from offload import TOK_PER_CHAR, WRAPPER, aggregate, fmt, hit_tokens, session_value

sys.path.insert(0, "/home/user/adage/evals/lab/research")
from harness.leakage import assert_clean  # noqa: E402

TAUS = (0.2, 0.3, 0.4, 0.5)


def files_of(a):
    return {t.split("::")[0] for t in a.targets}


def covered(s, V_i, k, mode):
    """Is the agent's call k covered by re-running call V_i (instrument)?"""
    R = records(s)
    if R[k].key == R[V_i].key:
        return True
    if mode == "exact":
        return False
    a, v = s.acts[k], s.acts[V_i]
    if a.run is None or v.run is None or a.run.runner != v.run.runner:
        return False
    fa, fv = files_of(a), files_of(v)
    if not fv:  # V runs the whole suite
        return True
    return bool(fa) and fa <= fv


def v_tokens(s, R, V_i, model, compact, passed):
    if compact and passed:
        return int(60 * 1 + len(R[V_i].text) / 4 + WRAPPER)
    return int(R[V_i].out_chars * TOK_PER_CHAR[model] + len(R[V_i].text) / 4 + WRAPPER)


def gate_features(R, t, V_i, n_edits_since_v):
    r = R[t]
    return [np.log1p(t), n_edits_since_v, np.log1p(t - V_i), float(R[V_i].failed),
            float(sum(1 for x in R[max(0, t - 4):t + 1] if authoring(x))), float(r.tool == "Write")]


class Verifier:
    def __init__(self, pool_sessions, tau):
        self.pool = Pool(pool_sessions)
        self.tau = tau
        self.gate = None
        X, y = [], []
        for s in pool_sessions:
            for t, V_i, ne in self._points(s):
                R = records(s)
                X.append(gate_features(R, t, V_i, ne))
                y.append(self._next_is_rerun(s, t, V_i))
        if len(set(y)) == 2:
            self.gate = LogisticRegression(max_iter=500).fit(np.array(X), np.array(y))

    def score(self, r):
        return self.pool.tpl_after.get(template(r), 0.0)

    def _points(self, s):
        """(t, V_i, edits since V) at every edit turn end once a verifier exists."""
        R = records(s)
        V_i, ne = None, 0
        for t, r in enumerate(R):
            if authoring(r):
                ne += 1
                last_of_turn = t + 1 >= len(R) or R[t + 1].resp != r.resp
                if V_i is not None and last_of_turn and t + 1 < len(R):
                    yield t, V_i, ne
            elif r.tool == "Bash" and not inline_script(r) and self.score(r) >= self.tau:
                V_i, ne = t, 0

    def _next_is_rerun(self, s, t, V_i):
        """Learned-label for the gate: the agent re-runs V's template before its next edit (pool)."""
        R = records(s)
        tv = template(R[V_i])
        for k in range(t + 1, len(R)):
            if authoring(R[k]):
                return 0
            if template(R[k]) == tv:
                return 1
        return 0

    def p_run(self, s, t, V_i, ne):
        if self.gate is None:
            return 1.0
        return float(self.gate.predict_proba(np.array([gate_features(records(s), t, V_i, ne)]))[0, 1])


def evaluate_session(ver, s, model, mode="cover", compact=False, gate=0.0, p_skip=1.0, harm_calls=0.0):
    R = records(s)
    hits, misses, removed = [], [], set()
    for t, V_i, ne in ver._points(s):
        if gate > 0 and ver.p_run(s, t, V_i, ne) < gate:
            continue
        # the agent's next test run (instrument) before its next edit
        k_hit = None
        for k in range(t + 1, len(R)):
            if authoring(R[k]):
                break
            if s.acts[k].run is not None or R[k].key == R[V_i].key:
                if covered(s, V_i, k, mode):
                    k_hit = k
                break
        passed_now = (not R[k_hit].failed) if k_hit is not None else (not R[V_i].failed)
        tok = v_tokens(s, R, V_i, model, compact, passed_now)
        if k_hit is not None and k_hit not in removed:
            removed.add(k_hit)
            hits.append((t, k_hit, max(tok, hit_tokens(s, R[k_hit])) if not (compact and passed_now) else tok))
        else:
            misses.append((t, tok))
    return session_value(s, hits, misses, model, p_skip=p_skip, harm_calls=harm_calls)


def pick_tau(pool_sessions, model, folds=4, seed=0):
    tasks = sorted({(s.repo, s.task) for s in pool_sessions})
    rng = np.random.default_rng(seed)
    rng.shuffle(tasks)
    fold = {t: i % folds for i, t in enumerate(tasks)}
    best, tab = None, {}
    for tau in TAUS:
        num = den = 0.0
        for f in range(folds):
            tr = [s for s in pool_sessions if fold[(s.repo, s.task)] != f]
            te = [s for s in pool_sessions if fold[(s.repo, s.task)] == f]
            v = Verifier(tr, tau)
            for s in te:
                r = evaluate_session(v, s, model)
                num += r["usd"]
                den += r["base_usd"]
        tab[tau] = num / max(den, 1e-9)
    best = max(TAUS, key=lambda x: tab[x])
    return best, tab


VARIANTS = {
    "cover, full output": dict(mode="cover"),
    "cover, compact on pass": dict(mode="cover", compact=True),
    "exact, full output": dict(mode="exact"),
    "exact, compact on pass": dict(mode="exact", compact=True),
    "cover, compact, gated p>=0.5": dict(mode="cover", compact=True, gate=0.5),
    "cover, compact, gated p>=0.5, p_skip=0.5": dict(mode="cover", compact=True, gate=0.5, p_skip=0.5),
    "cover, compact, gated p>=0.5, harm 1 call/miss": dict(mode="cover", compact=True, gate=0.5, harm_calls=1.0),
    "cover, compact, gated p>=0.7": dict(mode="cover", compact=True, gate=0.7),
}


def main():
    c = corpus()
    out = {}
    for model in ("haiku", "sonnet"):
        test = c.held_out(model=model)
        repos = sorted({s.repo for s in test})
        out[model] = {}
        for proto in ("split", "loro"):
            rows, cl, taus = defaultdict(list), defaultdict(list), []
            for r in repos:
                te = [s for s in test if s.repo == r]
                pool = c.train() if proto == "split" else [s for s in c.sessions if s.repo != r]
                assert_clean(pool, te)
                tau, _ = pick_tau(pool, "haiku")
                taus.append(tau)
                v = Verifier(pool, tau)
                for name, kw in VARIANTS.items():
                    for s in te:
                        rows[name].append(evaluate_session(v, s, model, **kw))
                        cl[name].append(s.cluster)
            res = {name: aggregate(rows[name], cl[name]) for name in VARIANTS}
            res["taus"] = taus
            out[model][proto] = res
            print(f"== {model} {proto} taus={taus}")
            for name in VARIANTS:
                print("  ", name, "|", fmt(res[name]), flush=True)
    json.dump(out, open("results/verifier.json", "w"), indent=1)


if __name__ == "__main__":
    main()
