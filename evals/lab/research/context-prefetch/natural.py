"""A natural experiment for uptake: does naming a target in the prompt shrink the hunt for it?

RepoBench prompts never contain a path (by construction), but 28% of targets have their file stem
(`Suspense`, `termui`, `_make`) as a word in the prompt: a weak, free "hint". Compare the calls
attributed to finding a target (targets.py) when its stem is named vs not, within repository
(stratified), on held-out control sessions. The ratio is a crude lower anchor for how much of the
hunt an explicit path hint removes.
"""
import collections
import json
import re
from pathlib import Path

import numpy as np

import data
import targets as TG


def main():
    S = data.load()
    info = TG.annotate(S)
    rows = []
    for s in S:
        if s.arm != "control" or s.phase != "eval" or s.sid not in info:
            continue
        d = info[s.sid]
        for f in d["T"]:
            stem = f.rsplit("/", 1)[-1].split(".")[0]
            named = len(stem) > 3 and re.search(r"\b" + re.escape(stem) + r"\b", s.prompt) is not None
            n_all = sum(1 for i, g in d["attr"].items() if g == f)
            n_search = sum(1 for i, g in d["attr"].items() if g == f and not d["access"][i])
            rows.append((s.repo, s.sid, named, n_all, n_search))
    by = collections.defaultdict(lambda: collections.defaultdict(list))
    for r, sid, named, a, b in rows:
        by[r][named].append((a, b))
    # stratified difference: mean over repos weighted by #targets
    def stat(sample):
        num = den = 0.0
        ratios = []
        for r, g in sample.items():
            if g[True] and g[False]:
                w = len(g[True]) + len(g[False])
                ratios.append((w, np.mean([x[0] for x in g[True]]), np.mean([x[0] for x in g[False]])))
        W = sum(w for w, _, _ in ratios)
        named = sum(w * a for w, a, _ in ratios) / W
        not_named = sum(w * b for w, _, b in ratios) / W
        return named, not_named
    named, not_named = stat(by)
    rng = np.random.default_rng(0)
    sids = sorted({sid for _, sid, *_ in rows})
    red = []
    for _ in range(1000):
        pick = collections.Counter(rng.choice(sids, len(sids)))
        b2 = collections.defaultdict(lambda: collections.defaultdict(list))
        for r, sid, nm, a, b in rows:
            for _ in range(pick.get(sid, 0)):
                b2[r][nm].append((a, b))
        x, y = stat(b2)
        red.append(1 - x / y)
    out = dict(targets=len(rows), share_named=round(float(np.mean([r[2] for r in rows])), 3),
               hunt_calls_named=round(named, 2), hunt_calls_not_named=round(not_named, 2),
               reduction=round(1 - named / not_named, 3),
               reduction_ci90=[round(float(np.percentile(red, 5)), 3), round(float(np.percentile(red, 95)), 3)])
    print(out)
    (Path(__file__).parent / "natural.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
