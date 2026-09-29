"""Leave-one-repository-out evaluation of cross-repository priors, with a learning curve.

For each held-out repository X and n in {0, 1, 3, 10} own sessions:

- pool = every other repository's `train` + `control` sessions (Haiku and Sonnet; the `adage` and
  `agents-md` arms are excluded because the U11 instinct or a hand-written AGENTS.md steered them)
  + X's first n training sessions (chronological).
- The learner (learn.py) is fitted on the pool; X contributes only its file tree on day one.
- Test set: X's held-out `control` sessions (hooks on, directives withheld), all n.

Policies (all choose, for a dead end of intent K reached with head h0, a head c of the same intent):

- `hier`   : hierarchical EB, prior pooled over the other repositories weighted by file-tree
             similarity, posterior updated with X's own n sessions;
- `hier_u` : the same with uniform repository weights;
- `pooled` : the prior alone (own data ignored);
- `own`    : X's own sessions alone (Beta(1,1) prior, >= 2 sessions of support);
- `u11`    : the hand-written instinct, as a baseline (hand trigger: `python(3) -m pytest` with
             "No module named pytest" in the output -> `.venv/bin/python -m pytest`).
- `oracle` : every recovered dead end removed at its first call (the ceiling).

Delivery modes: `post` (after the dead end, as hooks do today) and `pre` (a PreToolUse rewrite of
h0 -> c whenever the posterior says h0's first attempt is more likely a dead end than not).

Measurement: an episode is a dead end (learned classifier) up to the next same-intent call that
is not one. Saving of a useful directive = Bash calls + failed calls strictly inside the window
(+1 for the intercepted call itself in `pre`). *Useful* also requires that c is **verified** to
work in X: among every X session (all arms, measurement only) the calls with head c are
productive >= 50% of the time. A directive whose c ran and failed in X is a misfire (-1 call). A
directive whose c never ran in X is scored 0 (unverifiable), and counted.
"""

from __future__ import annotations

import collections
import json
import re
import sys
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import data  # noqa: E402
import learn  # noqa: E402

OUT = HERE / "results"
NS = [0, 1, 3, 10]
STRICT = False  # --strict: count only failed / dead-end calls inside a window
P_MIN, GAIN_MIN = 0.7, 0.2
U11_HEAD = re.compile(r"^python3? -m pytest$")
U11_SIG = "No module named pytest"  # hand instrument, baseline only
PRICE_CACHE_READ = {"haiku": 0.10e-6, "sonnet": 0.30e-6}


def boot(vals, groups, B=2000, seed=7, q=(5, 95)):
    vals = np.asarray(vals, float)
    if len(vals) == 0:
        return (0.0, 0.0, 0.0)
    g = collections.defaultdict(list)
    for v, k in zip(vals, groups):
        g[k].append(v)
    keys = list(g)
    sums = np.array([sum(g[k]) for k in keys])
    cnts = np.array([len(g[k]) for k in keys])
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, len(keys), (B, len(keys)))
    est = sums[idx].sum(1) / cnts[idx].sum(1)
    return (float(vals.mean()), float(np.percentile(est, q[0])), float(np.percentile(est, q[1])))


def wilson(k, n, z=1.645):
    if n == 0:
        return (0.0, 0.0, 1.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * np.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (p, c - h, c + h)


class Posterior:
    """P(first attempt of head h is productive in repository X).

    hier  : Beta(k m + s, k (1 - m) + f), m and k from the repositories in the pool (EB); a head
            the pool never ran gets its intent's pooled rate as m and the median k;
    pooled: the prior alone; own: Beta(1 + s, 1 + f) from X's own sessions only."""

    def __init__(self, prior, own, X, intent_prior=None, cluster_of=None, k0=5.0):
        self.prior, self.own, self.X = prior, own, X
        self.intent_prior, self.cluster_of, self.k0 = intent_prior or {}, cluster_of, k0

    def p(self, h, mode):
        """(mean, trials, 10% lower quantile) or (None, trials, None)."""
        from scipy.stats import beta
        s, n = self.own.get((self.X, h), (0, 0))
        if mode == "own":
            if n < 2:
                return None, n, None
            a, b = 1 + s, 1 + n - s
        elif h in self.prior:
            m, k, nr, nt = self.prior[h]
            if mode == "pooled":
                s, n = 0, 0
            a, b = k * m + s, k * (1 - m) + (n - s)
            n = nt + n
        elif mode in ("hier", "hier_u") and n >= 2 and self.cluster_of is not None:
            m = self.intent_prior.get(self.cluster_of(h), 0.5)
            a, b = self.k0 * m + s, self.k0 * (1 - m) + (n - s)
        else:
            return None, 0, None
        a, b = max(a, 1e-3), max(b, 1e-3)
        return a / (a + b), n, float(beta.ppf(0.10, a, b))


def intent_rates(stats, cluster_of, repos):
    """Pooled first-attempt productive rate per intent over the pool repositories."""
    acc = collections.defaultdict(lambda: [0, 0])
    for (r, h), (s, n) in stats.items():
        if r in repos:
            k = cluster_of(h)
            acc[k][0] += s
            acc[k][1] += n
    return {k: s / n for k, (s, n) in acc.items() if n}


def subject(head: str) -> str:
    """What a head runs: the basename of its last token, version digits dropped
    (`python -m pytest` -> pytest, `.venv/bin/python3` -> python)."""
    t = head.split()[-1] if head.split() else ""
    return re.sub(r"[\d.]+$", "", t.rsplit("/", 1)[-1])


def choose(L, post, mode, K, h0, tree, heads_by_K):
    """The best head of intent K for the target repository, or None. Gates: it binds in X's tree,
    it is not h0 at another granularity, posterior mean >= P_MIN and 10% quantile >= 0.5 over >= 3
    trials, and it beats h0's posterior mean by GAIN_MIN."""
    p0 = post.p(h0, mode)[0]
    best = None
    for c in heads_by_K.get(K, ()):
        if c == h0 or not learn.binds(c, tree) or "{" in c or "*" in c:
            continue
        a, b = c.split(), h0.split()
        if a[:len(b)] == b or b[:len(a)] == a:
            continue  # one is the other at a coarser granularity: re-attaching args gives the same call
        if subject(c) != subject(h0):
            continue  # a rewrite changes how the same thing is run, not what is run
        pc, nt, lb = post.p(c, mode)
        if pc is None or nt < 3 or pc < P_MIN or lb < 0.5:
            continue
        if p0 is not None and pc - p0 < GAIN_MIN:
            continue
        if best is None or (pc, nt) > (best[1], best[2]):
            best = (c, pc, nt)
    return best


def session_cost(s):
    n = len(s.calls)
    return (s.cost or 0.0) / max(1, n)


def main():
    t0 = time.time()
    allss = data.load()
    usable = [s for s in allss if s.arm in ("train", "control")]
    trees = learn.trees_for(allss)
    fps = learn.tree_vec(trees)
    global STRICT
    STRICT = "--strict" in sys.argv
    targets = [a for a in sys.argv[1:] if not a.startswith("--")] or data.REPOS
    tag = "-strict" if STRICT else ""
    opt = {a.split("=")[0]: a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--") and "=" in a}
    only_pool = set(opt["--pool"].split(",")) if "--pool" in opt else None
    ns = [int(x) for x in opt["--ns"].split(",")] if "--ns" in opt else NS
    tag += opt.get("--tag", "")
    OUT.mkdir(exist_ok=True)
    results = {}
    discoveries = {}
    for X in targets:
        own_all = sorted([s for s in usable if s.repo == X and s.arm == "train" and s.model == "haiku"],
                         key=lambda s: (s.order, s.rid))
        test = [s for s in allss if s.repo == X and s.arm == "control"]
        every_X = [s for s in allss if s.repo == X]
        others = [s for s in usable if s.repo != X and (only_pool is None or s.repo in only_pool)]
        results[X] = {}
        # Fixed measurement per held-out repository: the day-0 learner (other repositories only)
        # defines intents, dead ends and episodes on X's sessions, identically for every n.
        M = learn.Learner(others, trees)
        xrecs = {s.sid: M.prepare(s) for s in every_X}
        truth = collections.defaultdict(lambda: [0, 0])
        for rs in xrecs.values():
            for r in rs:
                if r.K is not None:
                    truth[r.head][1] += 1
                    truth[r.head][0] += int(r.pdead < 0.5)
        discoveries[X] = {}
        for n in ns:
            if n > len(own_all) and n != NS[-1]:
                continue
            own = own_all[:n]
            L = M if n == 0 else learn.Learner(others + own, trees)
            # --- statistics ---
            pool_recs = [L.recs[s.sid] for s in others + own]
            stats = L.first_attempts(pool_recs)
            repos = sorted({s.repo for s in others})
            w = {r: float(max(0.0, fps[X] @ fps[r]) ** 2 + 0.05) if X in fps and r in fps else 1.0 for r in repos}
            heads = {h for (r, h) in stats}
            prior_w, k0 = learn.eb_prior(stats, heads, repos, w)
            prior_u, _ = learn.eb_prior(stats, heads, repos, None)
            own_stats = {k: v for k, v in stats.items() if k[0] == X}
            heads_by_K = collections.defaultdict(set)
            for h in heads:
                heads_by_K[L.cluster_of(h)].add(h)
            tree = trees.get(X, set())
            ir = intent_rates(stats, L.cluster_of, set(repos))
            posts = {m: Posterior(prior_u if m == "hier_u" else prior_w, own_stats, X, ir, L.cluster_of, k0)
                     for m in ("hier", "hier_u", "pooled", "own")}
            rows = collections.defaultdict(list)  # policy/mode -> per-session dicts
            disc = {}
            for s in test:
                rs = xrecs[s.sid]
                eps = learn.episodes(rs)
                ncalls = len(rs)
                dpc = session_cost(s)
                model = s.model

                def window(st, rec):
                    if STRICT:
                        return sum(1 for r in rs[st + 1:rec] if r.err or (r.K is not None and r.pdead >= 0.5))
                    return sum(1 for r in rs[st + 1:rec] if r.tool == "Bash" or r.err)

                def cost_calls(text_len, st):
                    toks = text_len / 4 + 25
                    left = ncalls - st
                    return toks * PRICE_CACHE_READ[model] * left / dpc if dpc > 0 else 0.0

                for pol in ("hier", "hier_u", "pooled", "own", "u11", "oracle"):
                    for mode in ("post", "pre"):
                        saved = misfire = deliv = useful = unver = 0
                        carry = 0.0
                        done_heads = set()
                        for (st, rec, K, h0, hrec) in eps:
                            if pol == "oracle":
                                if rec is not None:
                                    saved += window(st, rec) + (1 if mode == "pre" else 0)
                                    deliv += 1
                                    useful += 1
                                continue
                            if pol == "u11":
                                if not (U11_HEAD.match(h0) and U11_SIG in rs[st].out):
                                    continue
                                c = ".venv/bin/python -m pytest" if learn.binds(".venv/bin/python -m pytest", tree) else None
                                if c is None:
                                    continue
                            else:
                                ch = choose(L, posts[pol], pol, L.cluster_of(h0), h0, tree, heads_by_K)
                                if ch is None:
                                    continue
                                c = ch[0]
                                disc.setdefault((pol, h0), (c, round(ch[1], 3), ch[2]))
                                if mode == "pre":
                                    p0 = posts[pol].p(h0, pol)[0]
                                    if p0 is None or p0 >= 0.5:
                                        continue
                            deliv += 1
                            carry += cost_calls(len(c) + 40, st)
                            ok, tot = truth.get(c, (0, 0))
                            if tot == 0:
                                unver += 1
                                continue
                            if ok / tot < 0.5 or M.cluster_of(c) != K:
                                misfire += 1  # fails in X, or does not reach the dead end's intent
                                continue
                            if rec is not None:
                                useful += 1
                                saved += window(st, rec) + (1 if mode == "pre" else 0)
                            done_heads.add(h0)
                        net = saved - misfire - carry
                        rows[f"{pol}/{mode}"].append(dict(sid=s.sid, task=s.task, model=model, calls=ncalls,
                                                          net=net, saved=saved, misfire=misfire, deliv=deliv,
                                                          useful=useful, unver=unver, dpc=dpc,
                                                          usd=net * dpc, cost=s.cost or 0.0, passed=s.passed))
            summ = {}
            for key, rr in rows.items():
                groups = [r["task"] for r in rr]
                net = boot([r["net"] for r in rr], groups)
                pct = boot([r["net"] / max(1, r["calls"]) for r in rr], groups)
                usd = boot([r["usd"] for r in rr], groups)
                d = sum(r["deliv"] for r in rr)
                u = sum(r["useful"] for r in rr)
                summ[key] = dict(sessions=len(rr), net_calls=net, pct_calls=pct, usd=usd, directives=d,
                                 useful=wilson(u, d), misfires=sum(r["misfire"] for r in rr),
                                 unverified=sum(r["unver"] for r in rr),
                                 affected=sum(1 for r in rr if r["deliv"] > 0),
                                 mean_calls=float(np.mean([r["calls"] for r in rr])),
                                 mean_cost=float(np.mean([r["cost"] for r in rr])))
            results[X][n] = dict(own_sessions=len(own), pool_sessions=len(others) + len(own), kappa0=k0,
                                 weights=w, summary=summ,
                                 per_session={k: [{kk: v[kk] for kk in ("sid", "task", "net", "calls", "usd")}
                                                  for v in rr] for k, rr in rows.items()})
            discoveries[X][n] = [dict(policy=pol, from_head=h0, to_head=c, p=p, trials=nt,
                                      verified_in_X=list(truth.get(c, (0, 0))),
                                      from_in_X=list(truth.get(h0, (0, 0))))
                                 for (pol, h0), (c, p, nt) in sorted(disc.items())]
            h = summ.get("hier/post", {})
            if "--quiet" in sys.argv:
                continue
            print(f"{X} n={n} ({len(own)} own) {time.time() - t0:.0f}s  hier/post {h.get('net_calls')}  "
                  f"hier/pre {summ['hier/pre']['net_calls']}  u11/pre {summ['u11/pre']['net_calls']}  "
                  f"oracle/pre {summ['oracle/pre']['net_calls']}", flush=True)
        (OUT / f"loro-{X}{tag}.json").write_text(json.dumps(dict(results=results[X], discoveries=discoveries.get(X)),
                                                        indent=1, default=str))


if __name__ == "__main__":
    main()
