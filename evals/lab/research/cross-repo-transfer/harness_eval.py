"""Score the cross-repository transfer learner with the sprint's common harness.

    OMP_NUM_THREADS=1 nice -n 10 python3 harness_eval.py            # LORO + curve + split + U11

The learner is `fit(train_sessions) -> TransferPolicy`. It sees only the sessions the driver hands
it; it keeps the `train` and `control` arms (the `adage` and `agents-md` arms were steered by the
U11 instinct or a hand-written AGENTS.md, so learning from them would smuggle the hand rule in).
The target repository's file tree (the zero-config bind checkout) is the one thing it reads about
the held-out repository, as a day-one install would.

Delivery:
- `post` : at PostToolUse of a Bash call the learned detector scores as a dead end, "run <c + the
  agent's own arguments>", c the best replacement head of the same learned intent;
- `pre`  : also at PreToolUse of a Bash call whose head's posterior P(first attempt productive)
  < 0.5 in this repository, the same rewrite before it runs (predictive, no hindsight).
Each (session, h0) fires at most once, like U11 routes.
"""

from __future__ import annotations

import collections
import json
import shlex
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import data  # noqa: E402
import learn  # noqa: E402
import run as R  # noqa: E402
from abstract import segments  # noqa: E402
from harness import Directive, load, score  # noqa: E402
from harness.drivers import _chronological, _pooled, loro, split  # noqa: E402
from harness.leakage import assert_clean  # noqa: E402
from harness.policies.u11 import U11  # noqa: E402

TREES = learn.trees_for([])
FPS = learn.tree_vec(TREES)


def adapt(hs) -> data.Session:
    calls = []
    for i, c in enumerate(hs.calls):
        inp = c.input if isinstance(c.input, dict) else {}
        inp = {k: data.strip_root(v) for k, v in inp.items()
               if k in ("command", "file_path", "pattern", "path", "notebook_path") and isinstance(v, str)}
        out = data.strip_root(c.output or "")
        if len(out) > 3000:
            out = out[:1500] + "\n...\n" + out[-1500:]
        calls.append(data.Call(c.tool, inp, out, bool(c.is_error), i))
    return data.Session(hs.sid, hs.repo, "sonnet" if "sonnet" in (hs.model or "") else "haiku", hs.run, hs.phase,
                        hs.arm, hs.task, hs.passed, hs.cost_usd, calls)


class TransferPolicy:
    events = {"PreToolUse", "PostToolUse"}

    def __init__(self, train, delivery="pre", mode="hier"):
        pool = [adapt(s) for s in train if s.arm in ("train", "control")]
        self.delivery, self.mode = delivery, mode
        self.name = f"cross-repo {mode}/{delivery}"
        self.L = learn.Learner(pool, TREES)
        self.stats = self.L.first_attempts([self.L.recs[s.sid] for s in pool])
        self.pool_repos = sorted({s.repo for s in pool})
        self.heads = {h for (_, h) in self.stats}
        self.ctx = {}
        self.log = collections.Counter()

    def variant(self, delivery=None, mode=None):
        import copy
        v = copy.copy(self)
        v.delivery, v.mode = delivery or self.delivery, mode or self.mode
        v.name = f"cross-repo {v.mode}/{v.delivery}"
        v.ctx, v.log = {}, collections.Counter()
        return v

    def _context(self, X):
        if X in self.ctx:
            return self.ctx[X]
        L = self.L
        repos = [r for r in self.pool_repos if r != X]
        w = {r: float(max(0.0, FPS[X] @ FPS[r]) ** 2 + 0.05) if X in FPS and r in FPS else 1.0 for r in repos}
        prior, k0 = learn.eb_prior(self.stats, self.heads, repos, w)
        own = {k: v for k, v in self.stats.items() if k[0] == X}
        ir = R.intent_rates(self.stats, L.cluster_of, set(repos))
        post = R.Posterior(prior, own, X, ir, L.cluster_of, k0)
        byK = collections.defaultdict(set)
        for h in self.heads:
            byK[L.cluster_of(h)].add(h)
        self.ctx[X] = (post, byK, TREES.get(X, set()))
        return self.ctx[X]

    def _pdead(self, out, err):
        from scipy.sparse import csr_matrix, hstack
        Xt = self.L.tf.transform([learn.out_norm(out)[:1500]])
        e = csr_matrix([[float(err), float(not out.strip())]])
        return float(self.L.clf.predict_proba(hstack([Xt, e]).tocsr())[0, 1])

    def __call__(self, sit):
        if sit.call is None or sit.call.tool != "Bash":
            return None
        if sit.event == "PreToolUse" and self.delivery != "pre":
            return None
        X = sit.session.repo
        cmd = sit.call.input.get("command", "") if isinstance(sit.call.input, dict) else ""
        cmd = data.strip_root(cmd.replace(sit.session.cwd.rstrip("/") + "/", "").replace(sit.session.cwd.rstrip("/"), "."))
        argv, h0 = self.L.V.action(cmd, X)
        if not h0:
            return None
        post, byK, tree = self._context(X)
        if sit.event == "PreToolUse":
            p0 = post.p(h0, self.mode)[0]
            if p0 is None or p0 >= 0.5:
                return None
        else:
            if self._pdead(data.strip_root(sit.call.output or ""), sit.failed) < 0.5:
                return None
        fired = sit.state.setdefault("xfer", set())
        if h0 in fired:
            return None
        ch = R.choose(self.L, post, self.mode, self.L.cluster_of(h0), h0, tree, byK)
        if ch is None:
            return None
        c = ch[0]
        segs = segments(cmd)
        if not segs:
            return None
        m = self.L.V.main(segs)
        k = len(h0.split(" && ")[-1].split())
        rest = [t for t in list(m)[k:] if t != "{code}"]
        command = (c + " " + shlex.join(rest)).strip()
        fired.add(h0)
        self.log[(X, h0, c)] += 1
        text = f"That is likely to fail here. Run this instead:\n  {command}"
        return Directive("run", text=text, command=command, route=f"xfer:{h0} -> {c}")


def hier_curve(corpus, ns=(0, 1, 3, 10), delivery="pre", model="haiku"):
    """Hierarchical learning curve: other repositories + the target's first n training sessions."""
    out, logs = {}, {}
    repos = sorted({s.repo for s in corpus.held_out(model=model)})
    for n in ns:
        reps = []
        for r in repos:
            test = corpus.held_out(repo=r, model=model)
            keep = set(_chronological(r)[:n])
            own = [s for s in corpus.train(repo=r) if s.task in keep]
            train = [s for s in corpus.sessions if s.repo != r] + own
            assert_clean(train, test)
            pol = TransferPolicy(train, delivery)
            reps.append(score(pol, test, name=f"hier n={n}:{r}"))
            logs[(n, r)] = {f"{h0} -> {c}": k for (_, h0, c), k in pol.log.items()}
            print(f"  n={n} {r}: {reps[-1].summary()['calls_saved_per_task']:+.2f} calls", flush=True)
        out[n] = _pooled(f"cross-repo hier/{delivery} n={n}", reps, reps[0].assumptions, 0.90)
    return out, logs


def curves(corpus, res, rec):
    from harness.drivers import curve as own_curve
    for n, rep in own_curve(lambda tr: TransferPolicy(tr, "pre", "own"), corpus, sizes=(1, 3, 10), model="haiku").items():
        rec(f"curve_own_pre_n{n}", rep)
    curve, logs = hier_curve(corpus, ns=(1, 3, 10))
    for n, rep in curve.items():
        rec(f"curve_hier_pre_n{n}", rep)
    res["curve_logs"] = {f"n={n} {r}": v for (n, r), v in logs.items()}
    (HERE / "results" / "harness.json").write_text(json.dumps(res, indent=1, default=str))
    print("done", flush=True)


def main():
    t0 = time.time()
    corpus = load()
    res = {}
    curves_only = "--curves" in sys.argv
    if curves_only:
        res = json.loads((HERE / "results" / "harness.json").read_text())

    def rec(key, rep):
        print(rep.headline(), flush=True)
        res[key] = rep.to_json()
        (HERE / "results" / "harness.json").write_text(json.dumps(res, indent=1, default=str))

    test_h = corpus.held_out(model="haiku")
    if curves_only:
        return curves(corpus, res, rec)
    train_all = [s for s in corpus.sessions if s.phase == "train"]
    u11 = {"u11_post": score(U11("post-hoc"), test_h, name="U11 post-hoc"),
           "u11_predictive": score(U11("predictive", train_all), test_h, name="U11 predictive")}
    for k, v in u11.items():
        rec(k, v)
    # Leave one repository out (the harness's loro protocol, one fit per held-out repository,
    # every delivery variant scored from the same fit).
    variants = {"loro_pre": ("pre", 1.0, "haiku"), "loro_post": ("post", 1.0, "haiku"),
                "loro_pre_pf05": ("pre", 0.5, "haiku"), "loro_post_pf05": ("post", 0.5, "haiku"),
                "loro_pre_all_models": ("pre", 1.0, None)}
    reps = collections.defaultdict(list)
    logs0 = {}
    for r in sorted({s.repo for s in corpus.held_out()}):
        train = [s for s in corpus.sessions if s.repo != r]
        base = TransferPolicy(train, "pre")
        for key, (d, pf, model) in variants.items():
            test = corpus.held_out(repo=r, model=model)
            if not test:
                continue
            assert_clean(train, test)
            pol = base.variant(d)
            reps[key].append(score(pol, test, name=f"{key}:{r}", p_follow=pf))
            if key == "loro_pre":
                logs0[r] = {f"{h0} -> {c}": k for (_, h0, c), k in pol.log.items()}
        print(f"[{time.time() - t0:.0f}s] loro {r}", flush=True)
    for key, rr in reps.items():
        rep = _pooled(f"cross-repo LORO day-0 {key}", rr, rr[0].assumptions, 0.90)
        rec(key, rep)
        res[key]["by_repo"] = {k: {kk: v[kk] for kk in ("sessions", "calls_saved_per_task", "calls_saved_ci",
                                                        "usd_saved_per_task", "usd_saved_ci", "usd_saved_share",
                                                        "directives", "useful_rate")}
                               for k, v in rep.by(lambda x: x.slice).items()}
    res["loro_directives"] = logs0
    # paired: learned day-0 transfer minus the hand-written U11, same sessions, task-clustered
    from harness.stats import clustered
    for mine, theirs in (("loro_pre", "u11_predictive"), ("loro_post", "u11_post"), ("loro_pre", "u11_post")):
        a = {r.sid: r for rep in reps[mine] for r in rep.rows}
        b = {r.sid: r for r in u11[theirs].rows}
        sids = sorted(set(a) & set(b))
        for metric in ("net_calls", "net_usd"):
            m, ci = clustered([getattr(a[x], metric) - getattr(b[x], metric) for x in sids], [a[x].cluster for x in sids])
            res[f"paired_{mine}_minus_{theirs}_{metric}"] = {"mean": m, "ci90": ci, "sessions": len(sids)}
            print(f"paired {mine} - {theirs} {metric}: {m:+.4f} [{ci[0]:+.4f}, {ci[1]:+.4f}] n={len(sids)}", flush=True)
    # own-repository only (the split driver): what a repo-local learner gets from its 7-10 sessions
    curves(corpus, res, rec)
    print(f"done {time.time() - t0:.0f}s")


if __name__ == "__main__":
    main()
