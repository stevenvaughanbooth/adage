"""Downstream utility: plug each action representation into one simple, outcome-optimising reflex
learner and count what it saves on held-out control-arm sessions, scored by the replay metric's
Sat/W/S rules (instrument.py, a port of crates/adage-eval/src/savings_replay.rs).

Every representation R gives two levels: `fine` (its equivalence) and `coarse` (one level up: a
lower cut of the same dendrogram, or the next shorter anti-unifier; for the taxonomy role|runner).

Failure, as the learner sees it (learned, no error strings): the harness's is_error flag, or an
output whose Drain-style first-line template was, in the fit corpus, an is_error output in >= 80%
of >= 3 sessions (this is what catches `python -m pytest ... | head` exiting 0).

The learner (identical for every R):
  fail(X)     share of the sessions' first X calls that failed, over the learning sessions
  fix X -> Y  a failed X call followed (within 6 calls) by a successful call of the same coarse
              class but another fine class Y: "a variant of what failed that worked"
  reflex      Y = the most frequent fix of X, seen in >= 3 learning sessions, and
              fail(Y) <= fail(X) / 2 (outcomes, not frequency). Directive: `run <c>`, c = Y's most
              common successful command.
Delivery on held-out control sessions (at most once per reflex per session):
  post-hoc     after an X call that failed (learned failure; the hook sees the output)
  pre-learned  at PreToolUse of the first X call when fail(X) >= 0.5: a rewrite from the command
               alone, no hindsight
  pre-hindsight at PreToolUse of an X call that would fail (the replay's pre-emptive convention)

Learning sets (evaluation is always on held-out control sessions):
  train-split  the repository's `train` sessions (7-10), as the product sees today
  grown        task-grouped 5-fold cross-fitting: the repository's train sessions plus the control
               sessions of the other folds' tasks (~40-60 sessions: a repository worked in for weeks)
  loro         every session (train + control) of the other four repositories; R fitted without
               the held-out repository: a brand-new repository
"""
from __future__ import annotations

import json
import sys
from collections import Counter, defaultdict

import numpy as np

import common as C
import data
import feats as FE
import instrument as I
import methods as M

OUT = data.HERE / "results"
WIN = 6


class FailModel:
    def fit(self, S):
        n, e, ss = Counter(), Counter(), defaultdict(set)
        for s in S:
            for r in s.recs:
                if r.tool == "Bash":
                    t = FE.out_template(r.out)
                    n[t] += 1
                    e[t] += r.is_error
                    ss[t].add(s.key)
        self.bad = {t for t in n if len(ss[t]) >= 3 and e[t] / n[t] >= 0.8 and t != "<empty>"}
        return self

    def failed(self, r) -> bool:
        return r.is_error or FE.out_template(r.out) in self.bad


def learn(sessions, lab, coarse, fm):
    first_fail, first_seen = Counter(), Counter()
    fixes, succ = defaultdict(Counter), defaultdict(Counter)
    for s in sessions:
        bash = [r for r in s.recs if r.tool == "Bash" and r.cmd.strip()]
        if not bash:
            continue
        cmds = [r.cmd for r in bash]
        L, K = lab(cmds), coarse(cmds)
        F = [fm.failed(r) for r in bash]
        seen, fx = set(), defaultdict(set)
        for i, x in enumerate(L):
            if not F[i]:
                succ[x][cmds[i]] += 1
            if x not in seen:
                seen.add(x)
                first_seen[x] += 1
                first_fail[x] += F[i]
            if F[i]:
                for j in range(i + 1, min(len(bash), i + 1 + WIN)):
                    if not F[j] and K[j] == K[i]:
                        if L[j] != x:
                            fx[x].add(L[j])
                        break
        for x, ys in fx.items():
            for y in ys:
                fixes[x][y] += 1
    fail = {x: first_fail[x] / first_seen[x] for x in first_seen}
    out = {}
    for x, c in fixes.items():
        for y, k in c.most_common():
            if k >= 3 and fail.get(y, 1.0) <= fail[x] / 2 and succ[y]:
                cmd = succ[y].most_common(1)[0][0]
                out[x] = {"y": y, "cmd": cmd, "support": k, "fail_x": fail[x], "fail_y": fail.get(y), "dc": I.dclass(cmd)}
                break
    return out


def fire(sess, lab, reflexes, mode, fm):
    acts = [I.act(r) for r in sess.recs]
    idx = [i for i, r in enumerate(sess.recs) if r.tool == "Bash" and r.cmd.strip()]
    L = dict(zip(idx, lab([sess.recs[i].cmd for i in idx]))) if idx else {}
    given, done, seen = [], set(), set()
    for i in idx:
        x = L[i]
        first = x not in seen
        seen.add(x)
        rf = reflexes.get(x)
        if rf is None or x in done or rf["dc"][0] == "unscored":
            continue
        r = sess.recs[i]
        rejected = r.out.lstrip().startswith("<tool_use_error>")
        if mode == "post-hoc" and fm.failed(r) and not rejected:
            given.append((i, rf["dc"], False))
        elif mode == "pre-learned" and first and rf["fail_x"] >= 0.5:
            given.append((i, rf["dc"], True))
        elif mode == "pre-hindsight" and acts[i].failed and not rejected:
            given.append((i, rf["dc"], True))
        else:
            continue
        done.add(x)
    return I.score(given, acts, sess.passed), I.score(given, acts, sess.passed, "strict")


def oracle(sess):
    """Ceiling for a recovery lever: at every call that fails, pre-emptively direct exactly the
    fix the agent eventually found (the next successful call with the same runner, or the same
    first two words)."""
    acts = [I.act(r) for r in sess.recs]
    given = []
    for t, (r, a) in enumerate(zip(sess.recs, acts)):
        if r.tool != "Bash" or not a.failed:
            continue
        dc = I.dclass(r.cmd)
        if dc[0] != "unscored" and any(I.sat(dc, b) for b in acts[t + 1:]):
            given.append((t, dc, True))
    return I.score(given, acts, sess.passed), I.score(given, acts, sess.passed, "strict")


def row(sess, both):
    scored, strict = both
    return {"key": sess.key, "repo": sess.repo, "task": sess.task, "calls": len(sess.recs), "v2haiku": sess.key.startswith("v2/") and sess.model == "haiku",
            "usd_per_call": (sess.cost_usd or 0) / max(1, len(sess.recs)),
            "directives": len(scored), "useful": sum(s.useful for s in scored),
            "harmful": sum(s.harmful for s in scored), "saved": sum(s.saved for s in scored),
            "net": sum(s.saved - s.cost for s in scored),
            "net_strict": sum(s.saved - s.cost for s in strict), "useful_strict": sum(s.useful for s in strict)}


def summarise(rows):
    if not rows:
        return {}
    m, lo, hi = C.boot([r["net"] for r in rows])
    mu, ulo, uhi = C.boot([r["net"] * r["usd_per_call"] for r in rows])
    mp, plo, phi = C.boot([r["net"] / r["calls"] for r in rows])
    ms, slo, shi = C.boot([r["net_strict"] for r in rows])
    d = sum(r["directives"] for r in rows)
    u = sum(r["useful"] for r in rows)
    h = sum(r["harmful"] for r in rows)
    return {"sessions": len(rows), "directives": d, "useful": u, "harmful": h,
            "useful_rate": u / d if d else 0.0, "useful_wilson90": C.wilson(u, d),
            "harmful_rate": h / d if d else 0.0,
            "S_calls": m, "S_ci90": [lo, hi], "S_usd": mu, "S_usd_ci90": [ulo, uhi],
            "S_pct_calls": mp, "S_pct_ci90": [plo, phi],
            "S_strict_calls": ms, "S_strict_ci90": [slo, shi], "useful_strict_rate": sum(r["useful_strict"] for r in rows) / d if d else 0.0,
            "per10calls": 10 * d / max(1, sum(r["calls"] for r in rows))}


MODES = ("post-hoc", "pre-learned", "pre-hindsight")
COARSE_DROP = 0.2


def representations(S, exclude_repo=None, taus=None):
    """name -> (fine labeller, coarse labeller), fitted on the non-control sessions (minus a repo)."""
    calls = C.fit_calls(S, exclude_repo)
    tabs = C.baseline_tables(S)
    rule = lambda k: C.table_rule(k, tabs[k]).labeller()
    tax = rule("taxonomy")
    role_runner = lambda cmds: ["|".join(x.split("|")[:2]) for x in tax(cmds)]
    role = lambda cmds: [x.split("|")[0] for x in tax(cmds)]
    R = {k: r.labeller() for k, r in M.RULES.items()}
    reps = {"taxonomy": (tax, role_runner), "intent": (rule("intent"), role),
            "lgg-loose": (rule("lgg-loose"), R["headsub"]),
            "literal": (R["literal"], R["headsub"]), "headsub": (R["headsub"], R["head"])}
    L = M.LearnedLGG("L").fit(calls)
    H = M.LearnedLGG("H").fit(calls)
    reps["lgg-L"] = (L.labeller(), H.labeller())
    reps["lgg-H"] = (H.labeller(), R["head"])
    for name in M.EMBEDDERS:
        if taus and name in taus:
            m = C.clustered(name, S, exclude_repo)
            t = taus[name]
            reps[name] = (m.labeller(t), m.labeller(t - COARSE_DROP if t >= 0.4 else t / 2))
    return reps


def folds(sessions, k=5):
    tasks = sorted({s.task for s in sessions})
    rng = np.random.default_rng(5)
    rng.shuffle(tasks)
    return [set(tasks[i::k]) for i in range(k)]


def run(part, only=None):
    S = C.sessions()
    taus = json.load(open(OUT / "taus.json"))
    held = [s for s in S if s.arm == "control"]
    nonctl = [s for s in S if s.arm != "control"]
    res = {"oracle": {}, "reflexes": {}, "train-split": {}, "grown": {}, "loro": {}}
    orows = [row(s, oracle(s)) for s in held]
    res["oracle"] = summarise(orows)
    res["oracle_v2haiku"] = summarise([r for r in orows if r["v2haiku"]])
    res["waste_share"] = C.boot([sum(1 for a in map(I.act, s.recs) if a.failed or a.env_friction) / len(s.recs) for s in held])
    res["calls_per_session"] = float(np.mean([len(s.recs) for s in held]))
    rows = defaultdict(list)
    # in-repo settings: R fitted on all non-control sessions
    fm = FailModel().fit(nonctl)
    reps = representations(S, taus=taus) if part == "in" else {}
    for name, (lab, coarse) in reps.items():
        if only and name not in only:
            continue
        for repo in data.REPOS:
            train = [s for s in S if s.repo == repo and s.arm == "train"]
            rf = learn(train, lab, coarse, fm)
            res["reflexes"].setdefault(name, {})[repo] = {x: {k: v for k, v in r.items() if k != "dc"} for x, r in rf.items()}
            hr = [s for s in held if s.repo == repo]
            for s in hr:
                for mode in MODES:
                    rows[("train-split", name, mode)].append(row(s, fire(s, lab, rf, mode, fm)))
            for k, fold in enumerate(folds(hr)):
                rf = learn(train + [s for s in hr if s.task not in fold], lab, coarse, fm)
                if k == 0:
                    res["reflexes"].setdefault(name + "@grown", {})[repo] = {x: {kk: v for kk, v in r.items() if kk != "dc"} for x, r in rf.items()}
                for s in hr:
                    if s.task in fold:
                        for mode in MODES:
                            rows[("grown", name, mode)].append(row(s, fire(s, lab, rf, mode, fm)))
        print("in-repo", name, flush=True)
    for repo in data.REPOS if part == "loro" else []:
        fm = FailModel().fit([s for s in nonctl if s.repo != repo])
        reps = representations(S, exclude_repo=repo, taus=taus)
        learnset = [s for s in S if s.repo != repo and s.arm in ("train", "control")]
        for name, (lab, coarse) in reps.items():
            if only and name not in only:
                continue
            rf = learn(learnset, lab, coarse, fm)
            res["reflexes"].setdefault(name + "@loro", {})[repo] = {x: {k: v for k, v in r.items() if k != "dc"} for x, r in rf.items()}
            for s in held:
                if s.repo == repo:
                    for mode in MODES:
                        rows[("loro", name, mode)].append(row(s, fire(s, lab, rf, mode, fm)))
        print("loro", repo, flush=True)
    for (st, name, mode), rr in rows.items():
        d = res[st].setdefault(name, {})
        d[mode] = summarise(rr)
        d[mode]["v2haiku"] = summarise([x for x in rr if x["v2haiku"]])
        d[mode]["by_repo"] = {r: summarise([x for x in rr if x["repo"] == r]) for r in data.REPOS}
    json.dump(res, open(OUT / f"downstream_{part}.json", "w"), indent=1, default=str)
    return res


def table(res, setting, mode):
    rows = ["| representation | directives | useful (Wilson90 LB) | harmful | Ŝ calls/session [90% CI] | % of calls [90% CI] | $/session |",
            "|---|---:|---:|---:|---|---|---:|"]
    for k, v in sorted(res[setting].items(), key=lambda kv: -kv[1][mode]["S_calls"]):
        a = v[mode]
        rows.append(f"| {k} | {a['directives']} | {a['useful_rate']:.2f} ({a['useful_wilson90'][0]:.2f}) | {a['harmful_rate']:.2f} | "
                    f"{a['S_calls']:+.2f} [{a['S_ci90'][0]:+.2f}, {a['S_ci90'][1]:+.2f}] | {100 * a['S_pct_calls']:+.1f}% [{100 * a['S_pct_ci90'][0]:+.1f}, {100 * a['S_pct_ci90'][1]:+.1f}] | {a['S_usd']:+.4f} |")
    return "\n".join(rows)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "table":
        r = json.load(open(OUT / "downstream_in.json"))
        r["loro"] = json.load(open(OUT / "downstream_loro.json"))["loro"]
        for st in ("train-split", "grown", "loro"):
            for md in MODES:
                print(f"\n### {st} / {md}\n")
                print(table(r, st, md))
        print("oracle", r["oracle"])
    else:
        run(sys.argv[1], only=set(sys.argv[2].split(",")) if len(sys.argv) > 2 else None)
