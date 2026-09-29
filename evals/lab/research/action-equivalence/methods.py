"""Action representations. Each is fitted on a set of sessions (never the held-out control arm)
and maps any Bash command to a class id (`labeller(...)`), so that "roughly the same" = same id.

Learned (see feats.py for the only fixed parts, which are shell syntax):
  tfidf     corpus-vocabulary + BPE pieces, TF-IDF; clusters by average-linkage cosine   (method 2)
  potion    potion-base-8M static embedding of the normalised command line              (method 2)
  a2v       action2vec: PPMI-SVD of feature x trace-context (neighbour actions, own outcome) (method 1)
  rrr       outcome-supervised metric: reduced-rank ridge regression X -> outcome features Y  (method 3)
  triplet   outcome-supervised linear projection trained with a cosine triplet loss (numpy)  (method 3)
  hybrid    [tfidf-SVD, rrr] concatenated                                                   (2 + 3)
  lgg-L/H   anti-unification with learned literals and learned slot types                  (method 4)
Baselines:
  literal, head, headsub                    string / first word / first two words
  lgg-loose                                 evals/lab/abstraction (hand-typed holes, repo facts)
  taxonomy                                  action.rs's follow equivalence: role|runner|env class,
                                            via its Python ancestor grammar.py (HAND-WRITTEN)
  intent                                    grammar.py intent (role|scope class|relation, env-free)
"""
from __future__ import annotations

import re
from collections import Counter, defaultdict

import numpy as np
import scipy.sparse as sp
from scipy.cluster.hierarchy import fcluster, linkage
from scipy.sparse.linalg import svds

import feats as FE

RNG = np.random.default_rng(7)


def bash_calls(S):
    """[(rec, prev_recs, next_recs, session)] for every Bash call of the sessions."""
    out = []
    for s in S:
        rs = s.recs
        for i, r in enumerate(rs):
            if r.tool == "Bash" and r.cmd.strip():
                out.append((r, rs[max(0, i - 2):i], rs[i + 1:i + 3], s))
    return out


def key_of(r, fe=None) -> str:
    if r.tool != "Bash":
        return "T:" + r.tool
    segs = FE.lex(r.cmd)
    return "H:" + (segs[0][0][0].rsplit("/", 1)[-1] if segs else "")


def _norm(E):
    return E / np.maximum(np.linalg.norm(E, axis=1, keepdims=True), 1e-9)


# ------------------------------------------------------------------------------------------------
# Embedding methods
# ------------------------------------------------------------------------------------------------

class Lexical:
    name = "tfidf"

    def fit(self, calls):
        cmds = [c[0].cmd for c in calls]
        self.fe = FE.Featuriser().fit(cmds, [c[3].key for c in calls])
        X = self.fe.transform(cmds)
        k = min(256, X.shape[1] - 1)
        _, _, vt = svds(X.astype(np.float64), k=k)
        self.V = vt.T.astype(np.float32)
        return self

    def sparse(self, cmds):
        return self.fe.transform(cmds)

    def embed(self, cmds):
        return _norm((self.sparse(cmds) @ self.V))


class Potion:
    name = "potion"

    def fit(self, calls):
        import sys
        from pathlib import Path
        sys.path.insert(0, str(Path(__file__).resolve().parents[1].parent))
        from embed import MODELS, SPECS, Static
        self.m = Static(MODELS / SPECS["potion-8M"]["dir"])
        return self

    def embed(self, cmds):
        return _norm(self.m.encode([FE.text(c)[:600] for c in cmds]))


def outcome_matrix(calls, fit=None):
    """Y: what followed the action. Output words (first 400 chars), Drain-style first-line
    template, the harness error flag, and the next call's key. Vocabularies = seen in >= 3 sessions."""
    if fit is None:
        dfw, dft, dfn = defaultdict(set), defaultdict(set), defaultdict(set)
        for r, _, nx, s in calls:
            for w in set(FE.out_tokens(r.out)):
                dfw[w].add(s.key)
            dft[FE.out_template(r.out)].add(s.key)
            dfn[key_of(nx[0]) if nx else "END"].add(s.key)
        fit = [
            {w: i for i, w in enumerate(sorted(k for k, v in d.items() if len(v) >= 3))}
            for d in (dfw, dft, dfn)
        ]
    vw, vt, vn = fit
    blocks = []
    for vocab, get in ((vw, lambda c: set(FE.out_tokens(c[0].out))), (vt, lambda c: {FE.out_template(c[0].out)}),
                       (vn, lambda c: {key_of(c[2][0]) if c[2] else "END"})):
        rows, cols = [], []
        for i, c in enumerate(calls):
            for w in get(c):
                if w in vocab:
                    rows.append(i)
                    cols.append(vocab[w])
        M = sp.csr_matrix((np.ones(len(rows), np.float32), (rows, cols)), shape=(len(calls), max(1, len(vocab))))
        nrm = np.sqrt(np.asarray(M.sum(1)).ravel())
        blocks.append(sp.diags(1 / np.maximum(nrm, 1e-9)) @ M)
    err = sp.csr_matrix(np.array([[1.0 if c[0].is_error else 0.0] for c in calls], np.float32))
    Y = sp.hstack([blocks[0], blocks[1], err, 0.5 * blocks[2]]).tocsr()
    return Y, fit


class OutcomeRRR(Lexical):
    """Reduced-rank ridge regression from command features to outcome features: two commands are
    close when they predict the same outcome distribution."""
    name = "rrr"

    def __init__(self, rank: int = 32, lam: float = 1.0):
        self.rank, self.lam = rank, lam

    def fit(self, calls):
        super().fit(calls)
        Z = self.sparse([c[0].cmd for c in calls]) @ self.V  # n x 256
        Y, self.yfit = outcome_matrix(calls)
        Y = Y.toarray()
        A = Z.T @ Z + self.lam * np.eye(Z.shape[1], dtype=np.float32)
        B = np.linalg.solve(A, Z.T @ Y)
        _, _, vt = np.linalg.svd(Z @ B, full_matrices=False)
        self.W = (self.V @ B @ vt[: self.rank].T).astype(np.float32)  # p x r
        return self

    def embed(self, cmds):
        return _norm(self.sparse(cmds) @ self.W)


class OutcomeTriplet(Lexical):
    """Linear projection trained with a cosine triplet loss: positives are calls whose outcomes
    (Y) are similar, negatives calls whose outcomes differ."""
    name = "triplet"

    def __init__(self, dim: int = 32, epochs: int = 15, margin: float = 0.3, lr: float = 0.05):
        self.dim, self.epochs, self.margin, self.lr = dim, epochs, margin, lr

    def fit(self, calls):
        super().fit(calls)
        Z = np.asarray(self.sparse([c[0].cmd for c in calls]) @ self.V, np.float32)
        Y, _ = outcome_matrix(calls)
        Y = _norm(Y.toarray())
        n = len(Z)
        idx = RNG.choice(n, size=min(n, 3000), replace=False)
        Ys = Y[idx] @ Y[idx].T
        W = RNG.normal(0, 0.1, (Z.shape[1], self.dim)).astype(np.float32)
        Zs = Z[idx]
        pos = [np.where(Ys[i] >= 0.7)[0] for i in range(len(idx))]
        neg = [np.where(Ys[i] <= 0.3)[0] for i in range(len(idx))]
        for ep in range(self.epochs):
            G = np.zeros_like(W)
            for i in RNG.permutation(len(idx)):
                if len(pos[i]) < 2 or not len(neg[i]):
                    continue
                p, q = RNG.choice(pos[i]), RNG.choice(neg[i])
                if p == i:
                    continue
                za, zp, zn = Zs[i], Zs[p], Zs[q]
                ua, up, un = za @ W, zp @ W, zn @ W
                na, np_, nn = (np.linalg.norm(x) + 1e-9 for x in (ua, up, un))
                a, pp, nq = ua / na, up / np_, un / nn
                cp, cn = a @ pp, a @ nq
                if self.margin - cp + cn <= 0:
                    continue
                # d(cn - cp)/dW via the cosine gradients
                ga = ((nq - cn * a) - (pp - cp * a)) / na
                gp = -(a - cp * pp) / np_
                gn = (a - cn * nq) / nn
                G += np.outer(za, ga) + np.outer(zp, gp) + np.outer(zn, gn)
            W -= self.lr * G / len(idx)
        self.W = (self.V @ W).astype(np.float32)
        return self

    def embed(self, cmds):
        return _norm(self.sparse(cmds) @ self.W)


class Hybrid:
    name = "hybrid"

    def __init__(self, alpha: float = 0.5):
        self.alpha = alpha

    def fit(self, calls):
        self.lex = Lexical().fit(calls)
        self.out = OutcomeRRR()
        self.out.fe, self.out.V = self.lex.fe, self.lex.V
        Z = self.lex.sparse([c[0].cmd for c in calls]) @ self.lex.V
        Y, _ = outcome_matrix(calls)
        Y = Y.toarray()
        B = np.linalg.solve(Z.T @ Z + np.eye(Z.shape[1], dtype=np.float32), Z.T @ Y)
        _, _, vt = np.linalg.svd(Z @ B, full_matrices=False)
        self.out.W = (self.lex.V @ B @ vt[:32].T).astype(np.float32)
        return self

    def embed(self, cmds):
        a = np.sqrt(self.alpha)
        return _norm(np.hstack([a * self.lex.embed(cmds), np.sqrt(1 - self.alpha) * self.out.embed(cmds)]))


def first_in_session(calls):
    """The calls that are the first of their head word in their session: their outcome is the
    action's own, before the agent repaired anything (installed a package, created a venv)."""
    seen, keep = set(), []
    for c in calls:
        k = (c[3].key, key_of(c[0]))
        if k not in seen:
            seen.add(k)
            keep.append(c)
    return keep


class OutcomeRRRFirst(OutcomeRRR):
    """rrr supervised only by first-in-session outcomes (state-free)."""
    name = "rrr-first"

    def fit(self, calls):
        Lexical.fit(self, calls)
        fc = first_in_session(calls)
        Z = self.sparse([c[0].cmd for c in fc]) @ self.V
        Y, self.yfit = outcome_matrix(fc)
        Y = Y.toarray()
        B = np.linalg.solve(Z.T @ Z + self.lam * np.eye(Z.shape[1], dtype=np.float32), Z.T @ Y)
        _, _, vt = np.linalg.svd(Z @ B, full_matrices=False)
        self.W = (self.V @ B @ vt[: self.rank].T).astype(np.float32)
        return self


class HybridFirst(Hybrid):
    name = "hybrid-first"

    def fit(self, calls):
        self.lex = Lexical().fit(calls)
        self.out = OutcomeRRRFirst()
        self.out.fe, self.out.V = self.lex.fe, self.lex.V
        fc = first_in_session(calls)
        Z = self.lex.sparse([c[0].cmd for c in fc]) @ self.lex.V
        Y, _ = outcome_matrix(fc)
        Y = Y.toarray()
        B = np.linalg.solve(Z.T @ Z + np.eye(Z.shape[1], dtype=np.float32), Z.T @ Y)
        _, _, vt = np.linalg.svd(Z @ B, full_matrices=False)
        self.out.W = (self.lex.V @ B @ vt[:32].T).astype(np.float32)
        return self


class Action2Vec(Lexical):
    """PPMI-SVD over (command feature, trace context): the neighbouring actions (+-2) and the
    action's own outcome words, template and error flag."""
    name = "a2v"

    def __init__(self, dim: int = 64):
        self.dim = dim

    def fit(self, calls):
        cmds = [c[0].cmd for c in calls]
        self.fe = FE.Featuriser().fit(cmds, [c[3].key for c in calls])
        X = (self.fe.transform(cmds) > 0).astype(np.float32)
        ctx_df = defaultdict(set)
        ctxs = []
        for r, pv, nx, s in calls:
            c = [f"p{len(pv) - j}:{key_of(x)}" for j, x in enumerate(pv)] + [f"n{j + 1}:{key_of(x)}" for j, x in enumerate(nx)]
            c += ["o:" + w for w in set(FE.out_tokens(r.out))] + ["tpl:" + FE.out_template(r.out), f"err:{r.is_error}"]
            ctxs.append(c)
            for x in set(c):
                ctx_df[x].add(s.key)
        cv = {x: i for i, x in enumerate(sorted(k for k, v in ctx_df.items() if len(v) >= 3))}
        rows, cols = [], []
        for i, c in enumerate(ctxs):
            for x in c:
                if x in cv:
                    rows.append(i)
                    cols.append(cv[x])
        C = sp.csr_matrix((np.ones(len(rows), np.float32), (rows, cols)), shape=(len(calls), len(cv)))
        M = (X.T @ C).toarray()  # feature x context counts
        tot = M.sum()
        pw = M.sum(1, keepdims=True) / tot
        pc = M.sum(0, keepdims=True) ** 0.75
        pc = pc / pc.sum()
        with np.errstate(divide="ignore", invalid="ignore"):
            pmi = np.log((M / tot) / (pw * pc))
        P = np.where(np.isfinite(pmi) & (pmi > 0), pmi, 0).astype(np.float32)
        u, s_, _ = np.linalg.svd(P, full_matrices=False)
        self.W = (u[:, : self.dim] * np.sqrt(s_[: self.dim])).astype(np.float32)  # feature vectors
        return self

    def embed(self, cmds):
        return _norm(self.fe.transform(cmds) @ self.W)


# ------------------------------------------------------------------------------------------------
# Clustering: embedding -> equivalence classes (fit), nearest-prototype assignment (hook time)
# ------------------------------------------------------------------------------------------------

class Clustered:
    def __init__(self, emb, max_fit: int = 3500):
        self.emb, self.max_fit = emb, max_fit
        self.name = emb.name

    def fit(self, calls):
        self.emb.fit(calls)
        cmds = sorted({c[0].cmd for c in calls})
        if len(cmds) > self.max_fit:
            cmds = list(RNG.choice(cmds, self.max_fit, replace=False))
        self.cmds = cmds
        self.E = self.emb.embed(cmds).astype(np.float64)
        zero = np.linalg.norm(self.E, axis=1) < 1e-6  # no known feature: its own class
        self.E[zero] = _norm(RNG.normal(size=(zero.sum(), self.E.shape[1])))
        self.Z = linkage(self.E, method="average", metric="cosine")
        self.E = self.E.astype(np.float32)
        return self

    def cut(self, tau: float):
        """Prototypes (centroids) of the clusters at similarity threshold tau."""
        lab = fcluster(self.Z, t=1 - tau, criterion="distance")
        k = lab.max()
        C = np.zeros((k, self.E.shape[1]))
        np.add.at(C, lab - 1, self.E)
        return _norm(C).astype(np.float32)

    def labeller(self, tau: float):
        C = self.cut(tau)
        cache = {}

        def lab(cmds):
            new = [c for c in set(cmds) if c not in cache]
            if new:
                S = self.emb.embed(new) @ C.T
                j = S.argmax(1)
                for c, jj, ss in zip(new, j, S[np.arange(len(new)), j]):
                    cache[c] = f"c{jj}" if ss >= tau else "u:" + c
            return [cache[c] for c in cmds]
        return lab


# ------------------------------------------------------------------------------------------------
# Label methods
# ------------------------------------------------------------------------------------------------

class Rule:
    """A fixed labelling function (baselines)."""

    def __init__(self, name, fn):
        self.name, self.fn = name, fn

    def fit(self, calls):
        return self

    def labeller(self, tau=None):
        return lambda cmds: [self.fn(c) for c in cmds]


def _head(c):
    s = [a for a, p in FE.lex(c) if not p]
    return s[0][0].rsplit("/", 1)[-1] if s else ""


def _headsub(c):
    s = [a for a, p in FE.lex(c) if not p]
    return " ".join(s[0][:2]) if s else ""


def shape(t: str) -> str:
    s = re.sub(r"[A-Za-z]+", "a", t)
    s = re.sub(r"\d+", "9", s)
    return re.sub(r"(a[_-])+a", "a", s)


class LearnedLGG:
    """Anti-unification over argv with learned literals and learned slot types.

    A token stays literal when it recurs in >= k sessions of >= 2 tasks (so the names of one
    task's files and tests become holes). A hole's type is learned: its character shape, plus its
    extension when that extension is itself a recurring literal. Flags are a set; segments after a
    pipe are dropped (they filter output). `level="H"` keeps only each segment's head and first
    literal operand (the anti-unifier of everything the class's members agree on at that depth).
    """

    def __init__(self, level: str = "L", k: int = 3):
        self.level, self.k = level, k
        self.name = f"lgg-{level}"

    def fit(self, calls):
        sess, task = defaultdict(set), defaultdict(set)
        ext = defaultdict(set)
        for r, _, _, s in calls:
            for a, p in FE.lex(r.cmd):
                for t in a:
                    sess[t].add(s.key)
                    task[t].add(s.task)
                    if "." in t:
                        ext[t.rsplit(".", 1)[-1]].add(s.key)
        self.lit = {t for t in sess if len(sess[t]) >= self.k and len(task[t]) >= 2}
        self.ext = {e for e, v in ext.items() if len(v) >= self.k and len(e) <= 5}
        return self

    def hole(self, t):
        if t == "<STR>":
            return "<STR>"
        e = t.rsplit(".", 1)[-1] if "." in t else ""
        base = t[: -len(e) - 1] if e in self.ext else t
        return f"<{shape(base)}{'.' + e if e in self.ext else ''}>"

    def label(self, c):
        parts = []
        for a, p in FE.lex(c):
            if p:
                continue
            head = a[0] if a[0] in self.lit else self.hole(a[0])
            flags = sorted({t.split("=")[0] for t in a[1:] if t.startswith("-") and t.split("=")[0] in self.lit})
            ops = [t if t in self.lit else self.hole(t) for t in a[1:] if not t.startswith("-")]
            if self.level == "H":
                lit_ops = [t for t in ops if not t.startswith("<")]
                parts.append(" ".join([head] + lit_ops[:1]))
            else:
                folded = []
                for t in ops:
                    if folded and folded[-1].rstrip("+") == t and t.startswith("<"):
                        folded[-1] = t + "+"
                    else:
                        folded.append(t)
                parts.append(" ".join([head] + folded + flags))
        return " && ".join(parts)

    def labeller(self, tau=None):
        return lambda cmds: [self.label(c) for c in cmds]


EMBEDDERS = {
    "tfidf": Lexical,
    "potion": Potion,
    "a2v": Action2Vec,
    "rrr": OutcomeRRR,
    "triplet": OutcomeTriplet,
    "hybrid": Hybrid,
    "rrr-first": OutcomeRRRFirst,
    "hybrid-first": HybridFirst,
}
RULES = {
    "literal": Rule("literal", FE.text),
    "head": Rule("head", _head),
    "headsub": Rule("headsub", _headsub),
}
TAUS = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9]
