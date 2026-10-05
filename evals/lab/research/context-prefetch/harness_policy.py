"""The file-prefetch lever as a harness policy, scored by the common harness (`../harness`).

Learner `fit(train_sessions) -> policy`: a logistic-regression fusion of the content and git
scorers (BM25, potion-code-16M-v2 / potion-base-8M / 32M chunk embeddings, co-change and reference
propagation; plus kNN over the training sessions' prompts when the repository has training
sessions), trained on what the training sessions touched that mattered (edited, or opened before
their last edit). Nothing about tools, file kinds or languages is written by hand.

Policy: at the first UserPromptSubmit, `Directive("read_files", files=top-k)`: "Files most likely
relevant to this task: …". The harness scores it with its `read_files` class (Sat: a Read/Edit of a
listed file; W: searches and listings before it). Variant `rehint`: after each model turn in which
the agent opened a listed file, the files not yet opened are re-listed (a new directive, so each
listed file gets its own window; each re-hint pays its own carry).

Oracle: the same directive listing the session's own targets T (hindsight; a ceiling, not a policy).
"""

from __future__ import annotations

import collections
import json
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))

import data  # noqa: E402
import evaluate as EV  # noqa: E402
import targets as TG  # noqa: E402
from harness import Directive, load, score  # noqa: E402
from harness.drivers import curve, loro, split  # noqa: E402
from harness.leakage import scan_memory  # noqa: E402

S = {s.sid: s for s in data.load()}
INFO = TG.annotate(list(S.values()))


def labels(sid):
    d = INFO[sid]
    return d["E"] | d["pre"]


class Prefetch:
    def __init__(self, train_sessions, k=5, rehint=False, use_knn=True):
        self.k, self.rehint = k, rehint
        tr = [s for s in train_sessions if s.sid in INFO and s.task in EV.FEATS]
        self.memory = collections.defaultdict(list)
        for s in tr:
            self.memory[s.repo].append((s.prompt, labels(s.sid), s.task))
        self.use_knn = use_knn and any(self.memory.values())
        self.names = EV.BASE_FEATS + (["knn", "prior"] if self.use_knn else [])
        rows = []
        for s in tr:
            rows.append((self.scores(s.task, s.repo, s.prompt), EV.FEATS[s.task]["files"], labels(s.sid)))
        self.model = EV.fit(rows, self.names)
        self.name = f"prefetch-k{k}" + ("-rehint" if rehint else "")
        self.events = {"UserPromptSubmit", "PostToolBatch"} if rehint else {"UserPromptSubmit"}

    def scores(self, task, repo, prompt):
        F = EV.FEATS[task]
        sc = dict(F["f"])
        if self.use_knn:
            mem = [(p, fs) for p, fs, t in self.memory.get(repo, []) if t != task]
            sc["knn"] = EV.knn_scores(prompt, F["files"], mem)
            sc["prior"] = EV.prior_scores(F["files"], mem)
        return sc

    def top(self, sess):
        F = EV.FEATS[sess.task]
        x = EV.fused(self.model, self.scores(sess.task, sess.repo, sess.prompt), self.names)
        return [F["files"][i] for i in np.argsort(-x, kind="stable")[: self.k]]

    def __call__(self, sit):
        st = sit.state
        if sit.event == "UserPromptSubmit" and "files" not in st:
            st["files"] = self.top(sit.session)
            st["open"] = set()
            return self.directive(st["files"])
        if sit.event == "PostToolBatch" and "files" in st:
            opened = {f for c in sit.history for f in st["files"]
                      if c.tool in ("Read", "Edit", "Write", "MultiEdit")
                      and isinstance(c.input, dict) and str(c.input.get("file_path", "")).endswith(f)}
            new = opened - st["open"]
            st["open"] |= opened
            rest = [f for f in st["files"] if f not in st["open"]]
            if new and rest:
                return self.directive(rest)
        return None

    @staticmethod
    def directive(files):
        return Directive("read_files", files=tuple(files), route="prefetch",
                         text="Files most likely relevant to this task: " + ", ".join(files))


class Oracle(Prefetch):
    """Lists the session's own targets (hindsight)."""

    def __init__(self, rehint=False):
        self.rehint = rehint
        self.name = "oracle-targets" + ("-rehint" if rehint else "")
        self.events = {"UserPromptSubmit", "PostToolBatch"} if rehint else {"UserPromptSubmit"}

    def top(self, sess):
        return sorted(INFO[sess.sid]["T"]) if sess.sid in INFO else []


def main():
    corpus = load()
    out = {}
    for model in ("haiku", None):
        test = corpus.held_out(model=model)
        tag = model or "all"
        print(f"== held-out control sessions, model={tag}: {len(test)}")
        for pol in (Oracle(), Oracle(rehint=True)):
            for pf in (1.0, 0.5):
                rep = score(pol, test, p_follow=pf)
                print(rep.headline())
                out[f"{tag}:{pol.name}:p{pf}"] = rep.to_json()
        for k in (3, 5, 10):
            for rh in (False, True):
                fit = lambda tr, k=k, rh=rh: Prefetch(tr, k=k, rehint=rh)
                for pf in (1.0, 0.5):
                    rep = split(fit, corpus, model=model, name=f"split:prefetch-k{k}{'-rehint' if rh else ''}", p_follow=pf)
                    print(rep.headline())
                    out[f"{tag}:split:k{k}:{'rehint' if rh else 'once'}:p{pf}"] = rep.to_json()
        for rh in (False, True):
            fit = lambda tr, rh=rh: Prefetch(tr, k=5, rehint=rh, use_knn=False)
            for pf in (1.0, 0.5):
                rep = loro(fit, corpus, model=model, pool="all", name=f"loro:prefetch-k5{'-rehint' if rh else ''}", p_follow=pf)
                print(rep.headline())
                out[f"{tag}:loro:k5:{'rehint' if rh else 'once'}:p{pf}"] = rep.to_json()
    # learning curve (Haiku): the kNN memory grows with the training sessions
    cv = curve(lambda tr: Prefetch(tr, k=5, rehint=True), corpus, sizes=(1, 2, 4, 10), model="haiku")
    out["curve_rehint_k5"] = {str(n): r.to_json() for n, r in cv.items()}
    for n, r in cv.items():
        print("curve", n, r.headline().splitlines()[1])
    # leakage: what the learner stores are training prompts and file paths
    mem = [s.prompt for s in corpus.train()]
    out["scan_memory"] = str(scan_memory(mem, corpus.held_out(), corpus.train()))
    print("scan_memory:", out["scan_memory"][:300])
    (HERE / "harness_results.json").write_text(json.dumps(out, indent=1, default=str))


if __name__ == "__main__":
    main()
