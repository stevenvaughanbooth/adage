"""Skip-ahead prediction: predict an action the agent will *eventually* take, deliver it early, and
count what the jump skips.

Everything is over learned action clusters (space.py). Per regime (in-repo split, leave-one-repo-out,
leave-task-out learning curve):

1. **Discovery weights** W[c, c'] (learned from training sequences only): how specific cluster c' is
   to the stretch *before* the first occurrence of c. W = 1 − rate_after / rate_before (clipped,
   shrunk toward the global rate of c'). W ≈ 1: c' happens only while the agent has not yet done c
   (probing, failed variants, installs); W ≈ 0: c' happens as much after c (reading code, editing).
2. **Target** at decision point i (before each batch of calls) for cluster c: t = next index ≥ i
   whose cluster is c; skippable mass L(i,c) = Σ_{j∈[i,t)} W[c, c_j]. y = 1 iff t exists and L ≥ 1
   ("delivering c now would skip at least one call of discovery"). Also next-k: t − i < k.
3. **Models** score every (i, c): prior, PPM/variable-order Markov over eventual targets (Witten–Bell
   blending, orders 0..D), next-k PPM, logistic and a 1-hidden-layer MLP over bag-of-recent-actions
   embeddings, and the prefix tree of successful sessions.
4. **Calibration**: isotonic regression fitted on cross-fitted (out-of-fold, by task) training scores.
5. **Delivery**: at each decision point the best not-yet-delivered cluster, if p ≥ θ; delivered text
   = its most frequent training representative (this repo's when it has one).
6. **Outcome**: per session, the union of skipped calls (each weighted by W, max over deliveries),
   × p_follow, in calls and $ (cumulative-context attribution, load.py); minus carry cost of every
   delivery (80 tokens re-read on every later API turn) and harm of wrong deliveries (target never
   taken: p_follow × m calls). Also skip distance (upper bound) and failed calls in window.
"""

from __future__ import annotations

import collections
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from load import load, rates  # noqa: E402
from space import Space, potion, step_text  # noqa: E402
from normalize import EXPLORE_TOOLS, normalize_bash  # noqa: E402  (instrument only)
from metrics import DISCOVERY  # noqa: E402  (instrument only)

P_FOLLOW = 0.8
DIRECTIVE_TOKENS = 80
HARM_M = 1.0


# ----------------------------------------------------------------------------------------------
# data


class Sess:
    """A session projected into a Space."""

    def __init__(self, s, space: Space):
        self.s = s
        self.sid, self.repo, self.task, self.arm = s["sid"], s["repo"], s["task"], s["arm"]
        st = s["steps"]
        self.n = len(st)
        self.texts = [step_text(x) for x in st]
        self.c = space.assign(self.texts)
        self.err = np.array([x["err"] for x in st], bool)
        u = np.array([x["usd"] for x in st])
        self.u = u * (s["usd"] / max(u.sum(), 1e-12))  # sums to the true session $
        self.usd = s["usd"]
        self.mean_call = s["usd"] / max(self.n, 1)
        b = [x["batch"] for x in st]
        self.dp = [0] + [j for j in range(1, self.n) if b[j] != b[j - 1]]  # decision points
        turn = np.array([x["turn"] for x in st])
        _, _, rw, rr = rates(s["model"])
        left = s["api_turns"] - turn
        self.carry = DIRECTIVE_TOKENS * (rw + rr * left) / 1e6  # $ to carry a directive given before j
        self.prompt = s["prompt"]
        self.passed = s["passed"]
        self.V = space.vec(self.texts)
        self.inst = np.array([instrument(x) for x in st], bool)
        self.explore = np.array([explored(x) for x in st], bool)


def explored(st) -> bool:
    """MEASUREMENT INSTRUMENT ONLY: failed, or any exploratory step (the lab's hand taxonomy:
    Read/Grep/Glob, grep/find/cat/ls/git log...). Loose: includes reading the code the fix needs."""
    if st["err"]:
        return True
    if st["tool"] == "Bash":
        return normalize_bash(st["arg"].replace(" <<more", ""))[3]
    return st["tool"] in EXPLORE_TOOLS


def instrument(st) -> bool:
    """MEASUREMENT INSTRUMENT ONLY (hand-written, never a model input or training label): the lab's
    `metrics.discovery` definition of a wasted step: a failed call, or an exploratory step about how
    to work in the repository (manifests, tool config, environment probes). Same regex and
    exploratory taxonomy as evals/lab/metrics.py + normalize.py."""
    if st["err"]:
        return True
    if st["tool"] == "Bash":
        a = normalize_bash(st["arg"].replace(" <<more", ""))
        explore = a[3]
    else:
        explore = st["tool"] in EXPLORE_TOOLS
    return bool(explore and DISCOVERY.search(st["arg"]))


def next_occ(c: np.ndarray, K: int) -> np.ndarray:
    """nxt[i, k] = first j >= i with c[j] == k, else -1; shape (n+1, K)."""
    n = len(c)
    nxt = np.full((n + 1, K), -1, int)
    for j in range(n - 1, -1, -1):
        nxt[j] = nxt[j + 1]
        nxt[j, c[j]] = j
    return nxt


def discovery_weights(sessions: list[Sess], K: int, alpha: float = 5.0) -> np.ndarray:
    nb = np.zeros((K, K))
    na = np.zeros((K, K))
    Lb = np.zeros(K)
    La = np.zeros(K)
    tot = np.zeros(K)
    steps = 0
    for s in sessions:
        oh = np.zeros((s.n + 1, K))
        oh[np.arange(1, s.n + 1), s.c] = 1
        cum = oh.cumsum(0)
        tot += cum[-1]
        steps += s.n
        first = {}
        for j, c in enumerate(s.c):
            first.setdefault(int(c), j)
        for c, t in first.items():
            nb[c] += cum[t]
            na[c] += cum[s.n] - cum[t + 1]
            Lb[c] += t
            La[c] += s.n - t - 1
    r0 = tot / max(steps, 1)
    rb = (nb + alpha * r0) / (Lb[:, None] + alpha)
    ra = (na + alpha * r0) / (La[:, None] + alpha)
    W = np.clip(1 - ra / np.maximum(rb, 1e-12), 0, 1)
    W[Lb < 1] = 0
    return W


REL = 0.35  # "related to the target": cosine in the learned action space (see README)
SIM = 0.8  # "a failed variant of the target"


def labels(s: Sess, W, K: int, k_next: int = 5):
    """Per decision point i and cluster c: t = next occurrence of c at >= i. The learned target
    y = 1 iff t exists and the window [i, t) holds >= 1 failed call (the harness's is_error: data,
    not a hand list) that is *related* to the target step (cos >= REL in the learned space):
    "c is what the agent eventually does, after a dead end on the way to it". Also next-k."""
    nxt = next_occ(s.c, K)
    G = (s.V @ s.V.T >= REL) & s.err[None, :]  # G[t, j]: step j failed and relates to step t
    cumG = np.hstack([np.zeros((s.n, 1)), np.cumsum(G, 1)])
    dp = np.array(s.dp)
    t = nxt[dp]
    taken = t >= 0
    tt = np.where(taken, t, 0)
    F = np.where(taken, cumG[tt, tt] - cumG[tt, np.broadcast_to(dp[:, None], tt.shape)], 0)
    dist = np.where(taken, tt - dp[:, None], 0)
    y = taken & (F >= 1)
    nk = taken & (dist < k_next)
    return {"y": y, "F": F, "t": t, "dist": dist, "taken": taken, "nextk": nk}


# ----------------------------------------------------------------------------------------------
# features for the neural models


def seen_matrix(s: Sess, K: int) -> np.ndarray:
    if getattr(s, "_seen", None) is not None:
        return s._seen
    out = np.zeros((len(s.dp), K), np.float32)
    for r, i in enumerate(s.dp):
        if i:
            out[r, np.unique(s.c[:i])] = 1
    s._seen = out
    return out


def features(s: Sess, K: int, decay: float = 0.7, last: int = 5):
    if getattr(s, "_feat", None) is not None:
        return s._feat
    E = potion(s.texts) if s.n else np.zeros((0, 256), np.float32)
    P = potion([s.prompt[:600]])[0]
    rows = []
    for i in s.dp:
        bag = np.zeros(K, np.float32)
        for j in range(max(0, i - 20), i):
            bag[s.c[j]] += decay ** (i - 1 - j)
        rec = E[max(0, i - last):i].mean(0) if i else np.zeros(E.shape[1] if s.n else 256, np.float32)
        errs = s.err[max(0, i - 5):i].mean() if i else 0.0
        rows.append(np.concatenate([bag, rec, P, [np.log1p(i), errs, float(i == 0)]]))
    s._feat = np.array(rows, np.float32)
    return s._feat


# ----------------------------------------------------------------------------------------------
# models: fit(train: list[(Sess, lab)]) ; score(Sess) -> (n_dp, K)


class Prior:
    name = "prior"

    def __init__(self, K, target="y"):
        self.K, self.target = K, target

    def fit(self, data):
        pos = np.zeros((2, self.K))
        tot = np.zeros((2, self.K))
        for s, lab in data:
            seen = seen_matrix(s, self.K).astype(int)
            y = lab[self.target]
            for f in (0, 1):
                m = seen == f
                pos[f] += (y & m).sum(0)
                tot[f] += m.sum(0)
        self.p = (pos + 0.01) / (tot + 1)
        return self

    def score(self, s):
        seen = seen_matrix(s, self.K).astype(int)
        return np.where(seen == 1, self.p[1][None], self.p[0][None])


class PPM:
    """Variable-order Markov over cluster contexts (last m clusters, m = 0..D), predicting a
    multi-label target per cluster, with Witten-Bell interpolation between orders. The target
    cluster's seen/unseen state is part of the context. With repo=True every context after the
    global one is prefixed by the repository (a new repository falls back to the global order)."""

    def __init__(self, K, D=3, target="y", repo=False):
        self.K, self.D, self.target, self.repo = K, D, target, repo
        self.name = f"ppm{D}-{target}" + ("-repo" if repo else "")

    def _ctxs(self, s, i):
        out = [()]
        if self.repo:
            out.append((s.repo,))
        for m in range(1, self.D + 1):
            if m > i:
                break
            out.append(((s.repo,) if self.repo else ()) + tuple(s.c[i - m:i].tolist()))
        return out

    def fit(self, data):
        self.tab = collections.defaultdict(lambda: np.zeros((2, 2, self.K)))
        for s, lab in data:
            seen = seen_matrix(s, self.K).astype(int)
            y = lab[self.target]
            for r, i in enumerate(s.dp):
                for ctx in self._ctxs(s, i):
                    a = self.tab[ctx]
                    for f in (0, 1):
                        msk = seen[r] == f
                        a[f, 0] += msk
                        a[f, 1] += msk & y[r]
        return self

    def score(self, s):
        seen = seen_matrix(s, self.K).astype(int)
        out = np.zeros((len(s.dp), self.K))
        base = np.full((2, self.K), 1.0 / self.K)
        for r, i in enumerate(s.dp):
            p = base.copy()
            for ctx in self._ctxs(s, i):
                a = self.tab.get(ctx)
                if a is None:
                    break
                tot, pos = a[:, 0], a[:, 1]
                T = (pos > 0).sum(1, keepdims=True) + 1.0
                p = np.where(tot > 0, (pos + T * p) / (tot + T), p)
            out[r] = np.where(seen[r] == 1, p[1], p[0])
        return out


class NextSym:
    """Classic PPM next-symbol model (instrument: top-1 next-cluster accuracy)."""

    def __init__(self, K, D=3):
        self.K, self.D = K, D

    def fit(self, seqs):
        self.tab = [collections.defaultdict(lambda: np.zeros(self.K)) for _ in range(self.D + 1)]
        for c in seqs:
            for i in range(len(c)):
                for m in range(min(self.D, i) + 1):
                    self.tab[m][tuple(c[i - m:i].tolist())][c[i]] += 1
        return self

    def predict(self, c, i):
        p = np.full(self.K, 1.0 / self.K)
        for m in range(min(self.D, i) + 1):
            a = self.tab[m].get(tuple(c[i - m:i].tolist()))
            if a is None:
                break
            T = (a > 0).sum() + 1.0
            p = (a + T * p) / (a.sum() + T)
        return p


class Net:
    """Logistic (hidden=0) or 1-hidden-layer MLP, numpy, Adam, BCE, one sigmoid per cluster, plus
    a learned per-cluster 'already seen in this session' logit offset."""

    def __init__(self, K, hidden=0, target="y", epochs=40, lr=3e-3, l2=1e-4, seed=0):
        self.K, self.h, self.target, self.epochs, self.lr, self.l2, self.seed = K, hidden, target, epochs, lr, l2, seed
        self.name = ("mlp" if hidden else "logistic") + f"-{target}"

    def _feat(self, s):
        F = features(s, self.K)
        R = np.zeros((len(F), len(self.repos)), np.float32)
        if s.repo in self.repos:
            R[:, self.repos.index(s.repo)] = 1
        return np.hstack([F, R])

    def _xy(self, data):
        X, Y, S = [], [], []
        for s, lab in data:
            X.append(self._feat(s))
            S.append(seen_matrix(s, self.K))
            if lab is not None:
                Y.append(lab[self.target].astype(np.float32))
        return np.vstack(X), (np.vstack(Y) if Y else None), np.vstack(S)

    def fit(self, data):
        rng = np.random.default_rng(self.seed)
        self.repos = sorted({s.repo for s, _ in data})
        X, Y, S = self._xy(data)
        self.mu, self.sd = X.mean(0), X.std(0) + 1e-6
        X = (X - self.mu) / self.sd
        d = X.shape[1]
        P = {}
        if self.h:
            P["W1"] = rng.normal(0, np.sqrt(2 / d), (d, self.h)).astype(np.float32)
            P["b1"] = np.zeros(self.h, np.float32)
            P["W2"] = rng.normal(0, np.sqrt(1 / self.h), (self.h, self.K)).astype(np.float32)
        else:
            P["W2"] = np.zeros((d, self.K), np.float32)
        prior = np.clip(Y.mean(0), 1e-3, 1 - 1e-3)
        P["b2"] = np.log(prior / (1 - prior)).astype(np.float32)
        P["s2"] = np.zeros(self.K, np.float32)
        m = {k: np.zeros_like(v) for k, v in P.items()}
        v = {k: np.zeros_like(v) for k, v in P.items()}
        n = len(X)
        bs = 256
        step = 0
        for ep in range(self.epochs):
            idx = rng.permutation(n)
            for b in range(0, n, bs):
                j = idx[b:b + bs]
                x, y, sn = X[j], Y[j], S[j]
                if self.h:
                    a1 = x @ P["W1"] + P["b1"]
                    h = np.maximum(a1, 0)
                else:
                    h = x
                z = h @ P["W2"] + P["b2"] + sn * P["s2"]
                p = 1 / (1 + np.exp(-np.clip(z, -30, 30)))
                g = (p - y) / len(j)
                G = {"W2": h.T @ g + self.l2 * P["W2"], "b2": g.sum(0), "s2": (g * sn).sum(0)}
                if self.h:
                    gh = (g @ P["W2"].T) * (a1 > 0)
                    G["W1"] = x.T @ gh + self.l2 * P["W1"]
                    G["b1"] = gh.sum(0)
                step += 1
                for k in P:
                    m[k] = 0.9 * m[k] + 0.1 * G[k]
                    v[k] = 0.999 * v[k] + 0.001 * G[k] ** 2
                    P[k] -= self.lr * (m[k] / (1 - 0.9 ** step)) / (np.sqrt(v[k] / (1 - 0.999 ** step)) + 1e-8)
        self.P = P
        return self

    def score(self, s):
        X = (self._feat(s) - self.mu) / self.sd
        S = seen_matrix(s, self.K)
        P = self.P
        h = np.maximum(X @ P["W1"] + P["b1"], 0) if self.h else X
        z = h @ P["W2"] + P["b2"] + S * P["s2"]
        return 1 / (1 + np.exp(-np.clip(z, -30, 30)))


class PrefixTree:
    """Prefix tree of successful training sessions (collapsed cluster sequences). At a decision
    point, follow the current prefix as deep as it matches; score c by the share of the matched
    successful sessions that took c later (after the matched depth)."""

    name = "prefix-tree"

    def __init__(self, K):
        self.K = K

    @staticmethod
    def _collapse(c):
        out = []
        for x in c:
            if not out or out[-1] != x:
                out.append(int(x))
        return out

    def fit(self, data):
        self.seqs = [self._collapse(s.c) for s, _ in data if s.passed] or [self._collapse(s.c) for s, _ in data]
        return self

    def score(self, s):
        out = np.zeros((len(s.dp), self.K))
        for r, i in enumerate(s.dp):
            p = self._collapse(s.c[:i])
            best, M = -1, []
            for q in self.seqs:
                l = 0
                while l < len(p) and l < len(q) and p[l] == q[l]:
                    l += 1
                if l > best:
                    best, M = l, [q]
                elif l == best:
                    M.append(q)
            for q in M:
                out[r, list(set(q[best:]))] += 1
            out[r] /= max(len(M), 1)
        return out


def make(name, K):
    return {
        "prior": lambda: Prior(K),
        "repo-prior": lambda: PPM(K, 0, repo=True),
        "ppm1": lambda: PPM(K, 1),
        "ppm3": lambda: PPM(K, 3),
        "ppm1-repo": lambda: PPM(K, 1, repo=True),
        "ppm3-repo": lambda: PPM(K, 3, repo=True),
        "ppm3-next5": lambda: PPM(K, 3, "nextk"),
        "ppm3-next5-repo": lambda: PPM(K, 3, "nextk", repo=True),
        "logistic": lambda: Net(K, 0),
        "mlp": lambda: Net(K, 64),
        "prefix-tree": lambda: PrefixTree(K),
    }[name]()


# ----------------------------------------------------------------------------------------------
# calibration and delivery


def iso_fit(scores, ys):
    from sklearn.isotonic import IsotonicRegression
    return IsotonicRegression(out_of_bounds="clip", y_min=0, y_max=1).fit(scores, ys)


def top_pairs(sc, n=2):
    idx = np.argsort(-sc, 1)[:, :n]
    return idx


def crossfit_calibrator(name, K, train, folds=5, seed=0):
    tasks = sorted({s.task for s, _ in train})
    rng = np.random.default_rng(seed)
    rng.shuffle(tasks)
    fold = {t: k % folds for k, t in enumerate(tasks)}
    xs, ys = [], []
    for f in range(folds):
        tr = [d for d in train if fold[d[0].task] != f]
        te = [d for d in train if fold[d[0].task] == f]
        if not tr or not te:
            continue
        m = make(name, K).fit(tr)
        for s, lab in te:
            sc = m.score(s)
            idx = top_pairs(sc)
            r = np.arange(len(sc))[:, None]
            xs.append(sc[r, idx].ravel())
            ys.append(lab["y"][r, idx].ravel())
    return iso_fit(np.concatenate(xs), np.concatenate(ys).astype(float))


def deliver(s: Sess, prob: np.ndarray, theta: float, max_per_session: int = 99):
    """Greedy delivery: at each decision point the best not-yet-delivered cluster if p >= theta."""
    out, done = [], set()
    for r, i in enumerate(s.dp):
        if len(out) >= max_per_session:
            break
        order = np.argsort(-prob[r])
        for c in order[:5]:
            if prob[r, c] < theta:
                break
            if int(c) in done:
                continue
            out.append((r, i, int(c), float(prob[r, c])))
            done.add(int(c))
            break
    return out


MEASURES = ("fsim", "relfail", "inst", "explore", "dist", "inst_any")


def window_masks(s: Sess, i: int, t: int) -> dict:
    """Steps in [i, t) a followed skip to step t removes, per measure. All but `dist`/`inst_any`
    are gated on relevance to the target (cos >= REL), so a far-off target (e.g. the final commit)
    is not credited with unrelated dead ends it would not have prevented."""
    win = np.arange(i, t)
    cos = s.V[win] @ s.V[t]
    rel = cos >= REL
    return {"fsim": win[s.err[win] & (cos >= SIM)], "relfail": win[s.err[win] & rel],
            "inst": win[s.inst[win] & rel], "explore": win[s.explore[win] & rel], "dist": win,
            "inst_any": win[s.inst[win]]}


def outcome(s: Sess, lab, W, dels, p_follow=P_FOLLOW, harm_m=HARM_M, rep=None):
    """Counterfactual value of a delivery set. Per step, three measures of what a followed skip
    removes (union over the session's deliveries, each step counted once):
      fsim  - failed calls in the window that are variants of the target (cos >= SIM) - no hand parts
      inst  - the lab's hand-written waste instrument (failed, or env/manifest exploration)
      dist  - every call in the window (skip distance; loose upper bound)
    Net = p_follow * saved - carry(all deliveries) - p_follow * harm_m * wrong."""
    sv = {k: np.zeros(s.n, bool) for k in MEASURES}
    useful = useful_inst = useful_text = wrong = taken = 0
    carry = 0.0
    for r, i, c, p in dels:
        carry += s.carry[i] if i < s.n else 0
        t = lab["t"][r, c]
        if t < 0:
            wrong += 1
            continue
        taken += 1
        useful += bool(lab["y"][r, c])
        if rep is not None and bool(lab["y"][r, c]):
            useful_text += bool(float(s.V[t] @ rep[c]) >= 0.9)
        win = np.arange(i, t)
        if len(win) == 0:
            continue
        wm = window_masks(s, i, t)
        for k in MEASURES:
            sv[k][wm[k]] = True
        useful_inst += bool(len(wm["inst"]))
    harm_calls = p_follow * harm_m * wrong
    carry_calls = carry / s.mean_call
    out = {"n": len(dels), "useful": useful, "useful_inst": useful_inst, "useful_text": useful_text, "taken": taken, "wrong": wrong,
           "usd": s.usd, "calls": s.n, "passed": s.passed}
    for k, m in sv.items():
        out[f"gross_{k}"] = p_follow * m.sum()
        out[f"net_{k}"] = p_follow * m.sum() - harm_calls - carry_calls
        out[f"netusd_{k}"] = p_follow * s.u[m].sum() - harm_calls * s.mean_call - carry
    return out


def boot(rows, fn, B=2000, seed=0):
    rng = np.random.default_rng(seed)
    n = len(rows)
    vals = []
    for _ in range(B):
        idx = rng.integers(0, n, n)
        v = fn([rows[j] for j in idx])
        if v is not None and np.isfinite(v):
            vals.append(v)
    return [float(np.percentile(vals, 5)), float(np.percentile(vals, 95))] if vals else [None, None]


def wilson(k, n, z=1.645):
    if n == 0:
        return [None, None]
    p = k / n
    d = 1 + z * z / n
    c = p + z * z / (2 * n)
    h = z * np.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return [float((c - h) / d), float((c + h) / d)]


def summarise(rows, ci=True):
    n = sum(r["n"] for r in rows)
    u = sum(r["useful"] for r in rows)
    ui = sum(r["useful_inst"] for r in rows)
    tk = sum(r["taken"] for r in rows)
    tot_usd = sum(r["usd"] for r in rows)
    tot_calls = sum(r["calls"] for r in rows)
    out = {
        "sessions": len(rows),
        "deliveries_per_session": n / len(rows),
        "precision": u / n if n else None,
        "precision_ci": wilson(u, n),
        "precision_inst": ui / n if n else None,
        "precision_text": sum(r["useful_text"] for r in rows) / n if n else None,
        "taken_rate": tk / n if n else None,
        "calls_per_session": float(np.mean([r["calls"] for r in rows])),
        "usd_per_session": float(np.mean([r["usd"] for r in rows])),
    }
    for k in MEASURES:
        out[f"gross_{k}"] = float(np.mean([r[f"gross_{k}"] for r in rows]))
        out[f"net_{k}"] = float(np.mean([r[f"net_{k}"] for r in rows]))
        out[f"net_{k}_usd_pct"] = 100 * sum(r[f"netusd_{k}"] for r in rows) / tot_usd
    if ci:
        for k in ("fsim", "relfail", "inst", "explore"):
            out[f"net_{k}_ci"] = boot(rows, lambda rs, k=k: np.mean([r[f"net_{k}"] for r in rs]))
            out[f"net_{k}_usd_pct_ci"] = boot(
                rows, lambda rs, k=k: 100 * sum(r[f"netusd_{k}"] for r in rs) / sum(r["usd"] for r in rs))
    return {k: (round(v, 4) if isinstance(v, float) else v) for k, v in out.items()}


def oracle(s: Sess, lab, budget=None):
    """Hindsight upper bound (p_follow 1, no harm, no carry): the deliveries that skip the most,
    under each measure. budget=None: any number of deliveries."""
    res = {}
    cand = [(i, int(lab["t"][r, c])) for r, i in enumerate(s.dp) for c in np.where(lab["taken"][r])[0]]
    cand = [(i, t) for i, t in cand if t > i]
    for k in MEASURES:
        def mask(i, t):
            return window_masks(s, i, t)[k]
        saved = np.zeros(s.n, bool)
        if budget is None:
            for i, t in cand:
                saved[mask(i, t)] = True
        else:
            masks = [mask(i, t) for i, t in cand]
            for _ in range(budget):
                g = [(~saved[m]).sum() for m in masks]
                if not g or max(g) == 0:
                    break
                saved[masks[int(np.argmax(g))]] = True
        res[k] = (saved.sum(), s.u[saved].sum())
    return res


def rep_text(space, c, repo_texts):
    """The text delivered for cluster c: its most frequent training member in this repo if any."""
    for t, _ in space.members[c].most_common():
        if t in repo_texts:
            return t
    return space.describe(c, 1)[0] if space.members[c] else ""
