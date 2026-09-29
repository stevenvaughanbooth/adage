"""Learned action clusters, two classic steps, both fitted on training-split sessions only and
without outcomes:

1. Drain (He et al. 2017; the drain3 0.9.11 implementation, vendored from its sdist because it
   will not build here) turns each call's one-line rendering (`<tool> <command | path | pattern>`)
   into a template: `Bash .venv/bin/python -m pytest <*> -q`, `Read <*>`. Held-out calls are
   matched against the training tree (`match`), falling back to the raw line.
2. potion-base-8M embeds the template; k-means over training templates gives the clusters, and
   every call goes to its nearest centroid (cosine), so roughly-the-same behaviour (pytest vs
   `.venv/bin/python -m pytest -x`) shares a cluster.

Two granularities (coarse k=12, fine k=40). `fit(exclude_repo=...)` refits without one repository's
training sessions, for leave-one-repo-out. Cluster names are for the report only (the top
templates of each cluster); nothing downstream reads them.
"""

from __future__ import annotations

import collections
import sys
from pathlib import Path

import numpy as np
from sklearn.cluster import KMeans

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import embed  # noqa: E402

from load import CACHE, load  # noqa: E402

_EMB = None


sys.path.insert(0, "/tmp/adage-research/success-uplift/pkg/drain3-0.9.11")


def line(st: dict) -> str:
    """The agent's own call, one line: tool + first line of the command (or the path / pattern)."""
    if st["tool"] == "Bash":
        body = st["raw"].split("\n", 1)[0]
    else:
        body = st["path"] or st["text"].split(" ", 1)[-1]
    return (st["tool"] + " " + " ".join(body.split()[:24])).strip()


_TPL = {}


def templates(S, exclude_repo=None) -> dict:
    """{line: drain template}, Drain fitted on training sessions (minus exclude_repo)."""
    if exclude_repo in _TPL:
        return _TPL[exclude_repo]
    from drain3.drain import Drain
    d = Drain(sim_th=0.4, depth=4, max_children=100)
    for s in S:
        if s["phase"] == "train" and s["repo"] != exclude_repo:
            for st in s["steps"]:
                d.add_log_message(line(st))
    out = {}
    for s in S:
        for st in s["steps"]:
            l = line(st)
            if l not in out:
                c = d.match(l, full_search_strategy="fallback")
                out[l] = c.get_template() if c else l
    _TPL[exclude_repo] = out
    return out


def call_text(st: dict, tpl: dict) -> str:
    return tpl[line(st)]


def embeddings(S) -> tuple:
    """(text -> row, matrix) over every template of every Drain fit, cached on disk."""
    global _EMB
    if _EMB is not None:
        return _EMB
    p = CACHE / "emb8m.npz"
    texts = set()
    for r in [None] + sorted({s["repo"] for s in S}):
        texts |= set(templates(S, r).values())
    texts = sorted(texts)
    if p.exists():
        z = np.load(p, allow_pickle=True)
        idx = {t: i for i, t in enumerate(z["texts"])}
        V = z["V"]
        if all(t in idx for t in texts):
            _EMB = (idx, V)
            return _EMB
    m = embed.load("potion-8M")
    V = np.concatenate([m.encode(texts[i:i + 512]) for i in range(0, len(texts), 512)]).astype(np.float32)
    np.savez(p, texts=np.array(texts, dtype=object), V=V)
    _EMB = ({t: i for i, t in enumerate(texts)}, V)
    return _EMB


def fit(S, k: int, exclude_repo: str | None = None, seed: int = 0):
    idx, V = embeddings(S)
    tpl = templates(S, exclude_repo)
    train = [call_text(st, tpl) for s in S if s["phase"] == "train" and s["repo"] != exclude_repo
             for st in s["steps"]]
    X = V[[idx[t] for t in train]]
    km = KMeans(n_clusters=k, n_init=4, random_state=seed).fit(X)
    C = km.cluster_centers_ / np.linalg.norm(km.cluster_centers_, axis=1, keepdims=True)
    return C


def assign(S, C, exclude_repo=None) -> dict:
    idx, V = embeddings(S)
    tpl = templates(S, exclude_repo)
    out = {}
    for s in S:
        X = V[[idx[call_text(st, tpl)] for st in s["steps"]]]
        out[s["sid"]] = np.argmax(X @ C.T, axis=1)
    return out


def names(S, lab: dict, k: int, top: int = 3) -> list[str]:
    c = [collections.Counter() for _ in range(k)]
    for s in S:
        for st, l in zip(s["steps"], lab[s["sid"]]):
            c[l][st["template"]] += 1  # the lab normaliser's template, for naming only
    return [" | ".join(f"{t} ({n})" for t, n in cc.most_common(top)) for cc in c]


if __name__ == "__main__":
    S = load()
    for k in (12, 40):
        C = fit(S, k)
        lab = assign(S, C)
        for i, n in enumerate(names(S, lab, k)):
            print(k, i, n)
