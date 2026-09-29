"""The learner: action clusters, a learned dead-end detector, and cross-repository priors.

Fitted on a *pool* of sessions (other repositories, plus the held-out repository's own first n
sessions when n > 0). Everything the evaluation uses about the held-out repository on day one is
its file tree.

1. Actions (`abstract.py`): every Bash call -> abstracted argv + head, with repository-unique
   tokens slotted against the repository's own tree.
2. Action clusters (learned equivalence): a head's vector is the mean of the potion-base-8M
   embedding of its text and of the outputs it produced; average-linkage clustering (cosine).
   Two heads are one intent when they read alike *and* produce the same kind of output
   (`.venv/bin/python -m pytest` ~ `python -m pytest`; `pip install pytest` is not, its output is
   an installer's).
3. Dead ends (learned). Behavioural label on the pool: a call is a dead end when the agent's next
   call of the same intent comes within 8 calls, with no file edited in between, and either
   changes the head or is preceded by a Bash call of another intent (the agent changed something
   to get a different result). A logistic regression on the call's output text (word 1-2 grams
   that occur in >= 2 repositories) + the harness's is_error then scores any output, including the
   exit-0 piped ones the harness cannot flag.
4. Episodes: a call scored as a dead end opens an episode of its intent; the next call of the same
   intent that is not a dead end closes it (recovery). The calls in between are what a directive
   at the dead end could save.
5. Priors: for each head h and repository r, the first attempt of h in each session is productive
   or a dead end. Rates are pooled across repositories with a beta-binomial hierarchy
   (method-of-moments kappa, Efron-Morris / Gelman ch. 5), optionally weighted by file-tree
   similarity, and updated with the target repository's own sessions (posterior
   Beta(kappa m + s, kappa (1 - m) + f)).
"""

from __future__ import annotations

import collections
import re
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1]))  # evals/lab for embed.py

import data  # noqa: E402
from abstract import Vocab  # noqa: E402

# Structural signal (declared hand-written part): tools whose calls change a file.
WRITE_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
HORIZON = 8

_M = None


def emb(texts):
    global _M
    if _M is None:
        import embed as E
        _M = E.load("potion-8M")
    return _M.encode(list(texts)) if texts else np.zeros((0, 256), np.float32)


def out_norm(text: str) -> str:
    t = re.sub(r"^Exit code \d+\n", "", text)  # the harness's framing; is_error carries it
    t = re.sub(r"\d+", "0", t)
    return t


def trees_for(sessions) -> dict[str, set[str]]:
    t = {r: data.tree(r) for r in data.REAL}
    for s in sessions:
        if s.repo in data.TREES:
            continue
        tt = t.setdefault(s.repo, set())
        for c in s.calls:
            p = c.inp.get("file_path")
            if p and not p.startswith("/"):
                parts = p.split("/")
                for k in range(1, len(parts)):
                    tt.add("/".join(parts[:k]) + "/")
                tt.add(p)
    return t


class Rec:
    __slots__ = ("repo", "sid", "task", "i", "tool", "head", "argv", "out", "err", "write", "K", "pdead", "dead_beh")

    def __init__(self, **kw):
        for k, v in kw.items():
            setattr(self, k, v)


def records(V: Vocab, s) -> list[Rec]:
    out = []
    for i, c in enumerate(s.calls):
        if c.tool == "Bash":
            argv, head = V.action(c.inp.get("command", ""), s.repo)
        else:
            argv, head = (), ""
        out.append(Rec(repo=s.repo, sid=s.sid, task=s.task, i=i, tool=c.tool, head=head, argv=argv,
                       out=c.out, err=c.is_error, write=c.tool in WRITE_TOOLS, K=None, pdead=0.0, dead_beh=None))
    return out


def _args(r):
    h = set(r.head.replace("&&", " ").split())
    return {t for t in r.argv if t not in h}


def _jacc(a, b):
    return 1.0 if not a and not b else len(a & b) / len(a | b)


class Learner:
    def __init__(self, pool, trees, tau: float = 0.25, min_head: int = 2):
        self.trees = trees
        self.V = Vocab(pool, trees)
        self.recs = {s.sid: records(self.V, s) for s in pool}
        self.pool = pool
        self._clusters(tau, min_head)
        for s in pool:
            self.assign(self.recs[s.sid])
        self._label()
        self._classifier()
        for s in pool:
            self.score(self.recs[s.sid])

    # --- 2. action clusters -------------------------------------------------------------
    def _clusters(self, tau, min_head):
        by = collections.defaultdict(list)
        for rs in self.recs.values():
            for r in rs:
                if r.head:
                    by[r.head].append(r)
        heads = [h for h, rs in by.items() if len(rs) >= min_head]
        self.heads = heads
        tv = emb(heads)
        ov = np.stack([emb([out_norm(r.out)[:300] for r in by[h][:40]]).mean(0) for h in heads])
        ov /= np.maximum(np.linalg.norm(ov, axis=1, keepdims=True), 1e-9)
        X = tv + ov
        X /= np.maximum(np.linalg.norm(X, axis=1, keepdims=True), 1e-9)
        from sklearn.cluster import AgglomerativeClustering
        lab = AgglomerativeClustering(n_clusters=None, metric="cosine", linkage="average",
                                      distance_threshold=tau).fit_predict(X)
        self.hvec = {h: tv[i] for i, h in enumerate(heads)}
        self.hK = {h: int(lab[i]) for i, h in enumerate(heads)}
        self.cent = {}
        for k in set(lab):
            v = tv[lab == k].mean(0)
            self.cent[int(k)] = v / max(np.linalg.norm(v), 1e-9)
        self.tau = tau
        self._next = max(self.cent) + 1

    def cluster_of(self, head: str) -> int:
        if head in self.hK:
            return self.hK[head]
        v = emb([head])[0]
        ks = list(self.cent)
        sims = np.array([self.cent[k] @ v for k in ks])
        j = int(sims.argmax())
        if 1 - sims[j] <= self.tau:
            k = ks[j]
        else:
            k = self._next
            self._next += 1
            self.cent[k] = v
        self.hK[head] = k
        return k

    def assign(self, rs):
        for r in rs:
            r.K = self.cluster_of(r.head) if r.head else None

    # --- 3. dead ends -------------------------------------------------------------------
    def _label(self):
        for rs in self.recs.values():
            for a, r in enumerate(rs):
                if r.K is None:
                    continue
                dead = False
                for b in range(a + 1, min(len(rs), a + 1 + HORIZON)):
                    x = rs[b]
                    if x.write:
                        break
                    if x.K == r.K:
                        other = any(y.K is not None and y.K != r.K for y in rs[a + 1:b])
                        dead = (x.head != r.head or other) and _jacc(_args(r), _args(x)) >= 0.5
                        break
                r.dead_beh = dead

    def _classifier(self):
        from sklearn.feature_extraction.text import CountVectorizer, TfidfVectorizer
        from sklearn.linear_model import LogisticRegression
        rs = [r for v in self.recs.values() for r in v if r.K is not None]
        texts = [out_norm(r.out)[:1500] for r in rs]
        cv = CountVectorizer(ngram_range=(1, 2), min_df=2, binary=True, token_pattern=r"[A-Za-z_][A-Za-z_]+")
        B = cv.fit_transform(texts)
        units = {r.repo for r in rs}
        repos = np.array([r.repo if len(units) >= 2 else r.task for r in rs])
        keep = np.zeros(B.shape[1], bool)
        seen = None
        for rep in set(repos):
            m = (B[repos == rep].sum(0) > 0).A1
            seen = m.astype(int) if seen is None else seen + m
        keep = seen >= 2
        if not keep.any():  # a single session: nothing can be shared yet, keep every term
            keep = np.ones_like(keep)
        vocab = [w for w, k in zip(cv.get_feature_names_out(), keep) if k]
        self.tf = TfidfVectorizer(ngram_range=(1, 2), vocabulary=vocab, token_pattern=r"[A-Za-z_][A-Za-z_]+",
                                  sublinear_tf=True)
        Xt = self.tf.fit_transform(texts)
        from scipy.sparse import hstack, csr_matrix
        e = csr_matrix(np.array([[float(r.err), float(not r.out.strip())] for r in rs]))
        X = hstack([Xt, e]).tocsr()
        y = np.array([r.dead_beh for r in rs], int)
        if len(set(y)) < 2:  # no re-attempt observed yet: nothing can be called a dead end
            from sklearn.dummy import DummyClassifier
            self.clf = DummyClassifier(strategy="constant", constant=0).fit(X, np.r_[y[:-1], 1 - y[-1]] if len(y) > 1 else y)
            return
        self.clf = LogisticRegression(C=1.0, max_iter=2000, class_weight="balanced").fit(X, y)

    def score(self, rs):
        from scipy.sparse import hstack, csr_matrix
        idx = [r for r in rs if r.K is not None]
        if not idx:
            return
        Xt = self.tf.transform([out_norm(r.out)[:1500] for r in idx])
        e = csr_matrix(np.array([[float(r.err), float(not r.out.strip())] for r in idx]))
        p = self.clf.predict_proba(hstack([Xt, e]).tocsr())[:, 1]
        for r, q in zip(idx, p):
            r.pdead = float(q)

    def prepare(self, s):
        """Records for a session outside the pool (held-out): assign intents and score outputs."""
        rs = records(self.V, s)
        self.assign(rs)
        self.score(rs)
        return rs

    # --- 4/5. first-attempt statistics per (repo, head) ---------------------------------
    def first_attempts(self, rs_by_session, theta=0.5):
        """{(repo, head): [successes, trials]} over sessions: the first call of each head."""
        st = collections.defaultdict(lambda: [0, 0])
        for rs in rs_by_session:
            seen = set()
            for r in rs:
                if r.K is None or r.head in seen:
                    continue
                seen.add(r.head)
                st[(r.repo, r.head)][1] += 1
                st[(r.repo, r.head)][0] += int(r.pdead < theta)
        return st


def episodes(rs, theta=0.5):
    """Dead-end episodes of one session: (start index, recovery index or None, intent, head)."""
    out, open_ = [], {}
    for a, r in enumerate(rs):
        if r.K is None:
            continue
        if r.pdead >= theta:
            if r.K not in open_:
                open_[r.K] = a
        elif r.K in open_:
            st = open_.pop(r.K)
            out.append((st, a, r.K, rs[st].head, r.head))
    for K, st in open_.items():
        out.append((st, None, K, rs[st].head, None))
    return sorted(out)


def binds(head: str, tree: set[str]) -> bool:
    """Every path-like token of the head must exist in the target repository's tree. A token is
    path-like when it has a '/' (tree-grounded abstraction keeps only shared components)."""
    if not tree:
        return True
    tops = {p.strip("/").split("/")[0] for p in tree}
    for seg in head.split(" && "):
        for tok in seg.split():
            if "/" not in tok or tok.startswith("{"):
                continue
            t = tok.strip("/")
            if "*" in t:
                pat = re.compile("^" + re.escape(t).replace(r"\*", "[^/]+") + "/?$")
                if not any(pat.match(p) for p in tree) and t.split("/")[0] not in tops:
                    return False
            elif t not in tree and (t + "/") not in tree:
                # deeper than the stored tree: accept when its first two components exist
                parts = t.split("/")
                if len(parts) <= 3 or ("/".join(parts[:3]) + "/") not in tree:
                    return False
    return True


def tree_vec(trees: dict[str, set[str]]):
    """File-tree fingerprints: TF-IDF over path components shared by >= 2 repositories."""
    from sklearn.feature_extraction.text import TfidfVectorizer
    names = sorted(trees)
    docs = [" ".join(c for p in trees[n] for c in [p.strip("/").split("/")[-1]]) for n in names]
    tv = TfidfVectorizer(token_pattern=r"\S+", min_df=2, lowercase=False)
    M = tv.fit_transform(docs).toarray()
    M /= np.maximum(np.linalg.norm(M, axis=1, keepdims=True), 1e-9)
    return {n: M[i] for i, n in enumerate(names)}


def eb_prior(stats: dict, heads, repos, weights=None):
    """Hierarchical beta-binomial across repositories for each head's first-attempt success.
    Returns {head: (m, kappa, n_repos, n_trials)}. kappa by method of moments over repositories,
    falling back to the median kappa across heads; clipped to [1, 50]."""
    out, kap = {}, []
    for h in heads:
        rows = [(stats[(r, h)][0], stats[(r, h)][1], (weights or {}).get(r, 1.0)) for r in repos
                if (r, h) in stats and stats[(r, h)][1] > 0]
        if not rows:
            continue
        s = np.array([x[0] for x in rows], float)
        n = np.array([x[1] for x in rows], float)
        w = np.array([x[2] for x in rows], float) * n
        p = s / n
        m = float((w * p).sum() / w.sum()) if w.sum() > 0 else float(s.sum() / n.sum())
        k = None
        if len(rows) >= 2:
            var = float(np.average((p - m) ** 2, weights=n))
            nbar = float(n.mean())
            bv = m * (1 - m)
            # var(p_r) = m(1-m)/(k+1) + m(1-m)/n (k+1 is the between-repository scale)
            excess = var - bv / nbar
            if excess > 1e-6 and bv > 0:
                k = min(50.0, max(1.0, bv / excess - 1))
            elif bv > 0:
                k = 50.0
            if k:
                kap.append(k)
        out[h] = [m, k, len(rows), int(n.sum())]
    k0 = float(np.median(kap)) if kap else 5.0
    for h in out:
        if out[h][1] is None:
            out[h][1] = k0
    return {h: tuple(v) for h, v in out.items()}, k0
