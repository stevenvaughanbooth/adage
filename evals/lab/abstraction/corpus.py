"""Load every session once, with its repo facts, base commit and grammar labels (cached)."""
from __future__ import annotations

import os
import pickle
from dataclasses import dataclass
from pathlib import Path

import facts as F
import grammar as G
import traces as T

CACHE = Path(os.environ.get("ABSTRACTION_CACHE", "/tmp/adage-abstraction-cache.pkl"))


@dataclass
class Labelled:
    sess: T.Session
    base: str
    acts: list  # grammar.Act per call
    facts: F.RepoFacts


def load(refresh: bool = False) -> list[Labelled]:
    if CACHE.exists() and not refresh:
        with open(CACHE, "rb") as f:
            return pickle.load(f)
    out = []
    tasks = {r: T.tasks(r) for r in T.REPOS}
    for s in T.load():
        base = tasks[s.repo].get(s.task, {}).get("base", "")
        fx = F.scan(s.repo, s.cwd, base) if Path(s.cwd).is_dir() else None
        if fx is None:
            continue
        out.append(Labelled(s, base, G.label_session(s, fx, base), fx))
    with open(CACHE, "wb") as f:
        pickle.dump(out, f)
    return out


if __name__ == "__main__":
    import collections
    import corpus as C
    L = C.load(refresh=True)
    print(len(L), "sessions")
    bash = [(l, c, a) for l in L for c, a in zip(l.sess.calls, l.acts) if c.tool == "Bash"]
    print(collections.Counter(a.role for _, _, a in bash).most_common())
    print(collections.Counter(a.level(1) for _, _, a in bash if a.role == "test-run").most_common())
    print(collections.Counter(a.level(3) for _, _, a in bash if a.role == "test-run").most_common(30))
