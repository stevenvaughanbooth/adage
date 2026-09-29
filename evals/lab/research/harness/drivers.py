"""Evaluation protocols for *learners*: a learner is `fit(train_sessions) -> policy`.

    from harness.drivers import split, loro, curve, leave_task_out
    rep = split(fit, corpus)            # train on the repo's training split, test on held-out control
    rep = loro(fit, corpus)             # leave-one-repository-out: never saw this repository
    tab = curve(fit, corpus)            # learning curve over 1, 2, 4, 7, 10 training sessions
    rep = leave_task_out(fit, corpus)   # grown memory: every other task's sessions, same repo

Every protocol checks leakage before fitting (`harness.leakage.assert_clean`): no training session
is a held-out session, on a held-out task, or a copy of one. `fit` receives Session objects (full
transcripts, outcomes); it must not look at anything else on disk.
"""

from __future__ import annotations

import json
from collections import defaultdict

from .leakage import assert_clean
from .replay import Report, score
from .traces import SPLITS, Corpus


def _pooled(name, reports, assumptions, level):
    rows = [r for rep in reports for r in rep.rows]
    return Report(name, rows, assumptions, level)


def split(fit, corpus: Corpus, slices=None, name="split", model=None, **kw) -> Report:
    """Per runner slice: fit on the base repository's training sessions, score on the slice's
    held-out control sessions. Returns the pooled report (use `.by(lambda r: r.slice)`)."""
    reps = []
    for sl in slices or sorted({s.slice for s in corpus.held_out(model=model)}):
        test = corpus.held_out(slice=sl, model=model)
        if not test:
            continue
        train = corpus.train(repo=test[0].repo)
        assert_clean(train, test)
        reps.append(score(fit(train), test, name=f"{name}:{sl}", **kw))
    a = reps[0].assumptions if reps else {}
    return _pooled(name, reps, a, kw.get("level", 0.90))


def loro(fit, corpus: Corpus, pool: str = "all", name="loro", model=None, **kw) -> Report:
    """Leave one repository out. For each repository R: fit on the other repositories' sessions
    (`pool="all"`: every arm and phase; `"train"`: their training sessions only), score on R's
    held-out control sessions (all of R's slices). The honest test of "any repository"."""
    reps = []
    repos = sorted({s.repo for s in corpus.held_out(model=model)})
    for r in repos:
        test = corpus.held_out(repo=r, model=model)
        others = [s for s in corpus.sessions if s.repo != r]
        train = others if pool == "all" else [s for s in others if s.phase == "train"]
        assert_clean(train, test)
        reps.append(score(fit(train), test, name=f"{name}:{r}", **kw))
    a = reps[0].assumptions if reps else {}
    return _pooled(name, reps, a, kw.get("level", 0.90))


def _chronological(repo: str) -> list[str]:
    p = SPLITS / f"{repo}.json"
    if not p.exists():
        return []
    sp = json.loads(p.read_text())["repos"][repo]
    dates = sp.get("dates", {})
    return sorted(sp.get("train", []), key=lambda t: dates.get(t, ""))


def curve(fit, corpus: Corpus, sizes=(1, 2, 4, 7, 10), model=None, **kw) -> dict:
    """Learning curve: for each n, fit on each repository's first n training tasks (chronological),
    score on its held-out control sessions. {n: Report}. Flat curve = the learner is not learning."""
    out = {}
    for n in sizes:
        reps = []
        for sl in sorted({s.slice for s in corpus.held_out(model=model)}):
            test = corpus.held_out(slice=sl, model=model)
            repo = test[0].repo
            keep = set(_chronological(repo)[:n])
            train = [s for s in corpus.train(repo=repo) if s.task in keep]
            assert_clean(train, test)
            reps.append(score(fit(train), test, name=f"curve{n}:{sl}", **kw))
        out[n] = _pooled(f"curve n={n}", reps, reps[0].assumptions if reps else {}, kw.get("level", 0.90))
    return out


def leave_task_out(fit, corpus: Corpus, model=None, name="leave-task-out", **kw) -> Report:
    """Grown memory, same repository: for each held-out task T, fit on the repository's training
    sessions plus every session (any arm) of its *other* held-out tasks, score on T's control
    sessions. What more history in one repository buys. One fit per task: keep `fit` cheap."""
    reps = []
    by_task = defaultdict(list)
    for s in corpus.held_out(model=model):
        by_task[(s.repo, s.task)].append(s)
    for (repo, task), test in sorted(by_task.items()):
        train = corpus.train(repo=repo) + [s for s in corpus.select(repo=repo, phase="eval") if s.task != task]
        assert_clean(train, test)
        reps.append(score(fit(train), test, name=f"{name}:{task}", **kw))
    return _pooled(name, reps, reps[0].assumptions if reps else {}, kw.get("level", 0.90))
