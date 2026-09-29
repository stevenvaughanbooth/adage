"""Learn "deterministic stretches": moments where the agent's next call is predictable, arguments
and all, from what is already in the context, so a hook could run it and remove the model turn.

At every decision point t (after call t, i.e. at the hook event that ends a model turn; t = −1 is
session start) the hook proposes candidates, generically:
  G1 repeat   every earlier non-authoring call of this session (exact key)
  G2 habit    keys seen in ≥2 pool sessions of this repository, or in ≥2 other repositories
  G3 pointer  a Read of a path printed in call t's output (first 5 mentions)
A gradient-boosted classifier scores each candidate with generic features (recency, counts, edits
since, outcome of its last run, output size, tool identity, template statistics learned from the
pool, position). Label: the candidate's key is taken within the next W calls before any edit.

No command names, error strings or runner spellings are used. Fixed parts (declared): the
authoring-tool set (Edit/Write/MultiEdit/NotebookEdit: the product's existing edit stimulus, and
what a hook may not author), the shell lexer (first word after VAR= assignments), W = 3.
"""
from __future__ import annotations

import json
import math
import re
from collections import Counter, defaultdict

import numpy as np
from sklearn.ensemble import HistGradientBoostingClassifier

from data import records

AUTHOR = ("Edit", "Write", "MultiEdit", "NotebookEdit")
TOOLS = ("Bash", "Read", "Grep", "Glob")
W = 3
PATH = re.compile(r"(?<![\w/.-])((?:[\w-]+/)*[\w-]+\.[A-Za-z]{1,5})(?![\w/])")
MAX_PTR = 5


def template(r) -> str:
    if r.tool != "Bash":
        return r.tool
    for w in r.text.split():
        if "=" in w and not w.startswith("-") and "/" not in w.split("=")[0]:
            continue
        return "Bash:" + w.rsplit("/", 1)[-1]
    return "Bash:"


def authoring(r) -> bool:
    return r.tool in AUTHOR


def inline_script(r) -> bool:
    # shell syntax only: a here-document or an interpreter's -c/-e argument carries new code
    return r.tool == "Bash" and ("<<" in r.text or re.search(r"\s-[ce]\s", r.text) is not None)


class Pool:
    """Statistics learned from the training pool (never held-out sessions)."""

    def __init__(self, sessions):
        self.repo_key = defaultdict(Counter)
        self.key_repos = defaultdict(set)
        self.key_pos = defaultdict(list)
        rep = Counter()
        tot = Counter()
        aft = Counter()
        lens = []
        for s in sessions:
            R = records(s)
            lens.append(len(R))
            for k in {r.key for r in R}:
                self.repo_key[s.repo][k] += 1
                self.key_repos[k].add(s.repo)
            seen = {}
            last_edit = -99
            for i, r in enumerate(R):
                tp = template(r)
                tot[tp] += 1
                if i - last_edit <= W:
                    aft[tp] += 1
                if authoring(r):
                    last_edit = i
                if r.key in seen:
                    rep[template(R[seen[r.key]])] += 1
                seen[r.key] = i
                self.key_pos[r.key].append(i / max(len(R), 1))
        self.tpl_repeat = {t: (rep[t] + 0.5) / (tot[t] + 2) for t in tot}
        self.tpl_after = {t: (aft[t] + 0.5) / (tot[t] + 2) for t in tot}
        self.mean_len = float(np.mean(lens)) if lens else 32.0
        self.habit = defaultdict(list)  # repo -> [(key, rec-like)]
        self.examples = {}
        for s in sessions:
            for r in records(s):
                self.examples.setdefault(r.key, r)

    def habits(self, repo):
        out = []
        for k, n in self.repo_key.get(repo, Counter()).items():
            if n >= 2:
                out.append(k)
        for k, reps in self.key_repos.items():
            if len(reps - {repo}) >= 2 and k not in out:
                out.append(k)
        return sorted(k for k in out if self.examples[k].tool not in AUTHOR and not inline_script(self.examples[k]))


NF = 30


def _feat(pool, repo, R, t, cand, src, seen_last, seen_count, edits_since_last, last_edit_i, n_edit_last_turn,
          ptr_rank, npaths):
    r = cand
    tp = template(r)
    f = np.zeros(NF, dtype=np.float32)
    f[0] = src == "repeat"
    f[1] = src == "habit"
    f[2] = src == "pointer"
    for j, tl in enumerate(TOOLS):
        f[3 + j] = r.tool == tl
    f[7] = math.log1p(t + 1)
    prev = R[t] if t >= 0 else None
    f[8] = prev is not None and authoring(prev)
    f[9] = prev is not None and prev.failed
    f[10] = prev is not None and prev.tool == "Bash"
    f[11] = prev is not None and prev.tool == "Read"
    li = seen_last.get(r.key)
    f[12] = li is not None
    f[13] = math.log1p(t - li) if li is not None else -1
    f[14] = math.log1p(seen_count.get(r.key, 0))
    f[15] = edits_since_last.get(r.key, -1) if li is not None else -1
    f[16] = R[li].failed if li is not None else -1
    f[17] = math.log1p(R[li].out_chars) if li is not None else -1
    f[18] = pool.tpl_repeat.get(tp, -1)
    f[19] = pool.tpl_after.get(tp, -1)
    f[20] = pool.repo_key.get(repo, Counter()).get(r.key, 0)
    f[21] = len(pool.key_repos.get(r.key, ()))
    pos = pool.key_pos.get(r.key)
    f[22] = abs(float(np.mean(pos)) - (t + 1) / pool.mean_len) if pos else -1
    f[23] = math.log1p(t - last_edit_i) if last_edit_i >= 0 else -1
    f[24] = n_edit_last_turn
    f[25] = ptr_rank
    f[26] = npaths
    # overlap of the candidate's text with the last edited file's stem (generic string overlap)
    if last_edit_i >= 0:
        p = R[last_edit_i].args.get("file_path", "") or ""
        stem = p.rsplit("/", 1)[-1].rsplit(".", 1)[0]
        f[27] = bool(stem) and len(stem) >= 3 and stem in r.text
    f[28] = inline_script(r)
    f[29] = seen_count.get(r.key, 0) > 0 and li is not None and R[li].key == R[t].key if t >= 0 else 0
    return f


def points(pool, s, R=None):
    """Yield (t, [(key, src, cand_rec, features)]) for each decision point of a session."""
    R = R if R is not None else records(s)
    habits = [pool.examples[k] for k in pool.habits(s.repo)]
    seen_last, seen_count = {}, Counter()
    edits_since_last = {}
    last_edit_i = -1
    n = len(R)
    for t in range(-1, n - 1):
        if t >= 0:
            r = R[t]
            if authoring(r):
                last_edit_i = t
                for k in edits_since_last:
                    edits_since_last[k] += 1
            else:
                seen_last[r.key] = t
                seen_count[r.key] += 1
                edits_since_last[r.key] = 0
        n_edit_last = 0
        if t >= 0:
            j = R[t].resp
            n_edit_last = sum(1 for x in R[max(0, t - 3):t + 1] if authoring(x) and x.resp >= j - 0)
        cands = {}
        for k, li in seen_last.items():
            if not inline_script(R[li]):
                cands[k] = ("repeat", R[li], 0, 0)
        for h in habits:
            if h.key not in cands:
                cands[h.key] = ("habit", h, 0, 0)
        if t >= 0 and R[t].out:
            paths = []
            for m in PATH.finditer(R[t].out):
                p = m.group(1).lstrip("./")
                if p not in paths:
                    paths.append(p)
                if len(paths) >= MAX_PTR:
                    break
            for rank, p in enumerate(paths):
                from data import Rec
                key = "Read|" + json.dumps({"file_path": p}, sort_keys=True)
                if key not in cands:
                    rr = Rec(-1, "Read", key, {"file_path": p}, f"file_path={p}", "", 0, False, -1, True)
                    cands[key] = ("pointer", rr, rank, len(paths))
        out = []
        for k, (src, rr, rank, npaths) in cands.items():
            out.append((k, src, rr, _feat(pool, s.repo, R, t, rr, src, seen_last, seen_count, edits_since_last,
                                          last_edit_i, n_edit_last, rank, npaths)))
        yield t, out


def label_window(R, t, key):
    """Index k of the first call taken with this key in (t, t+W] before any authoring call, else None."""
    for k in range(t + 1, min(len(R), t + 1 + W)):
        if R[k].key == key:
            return k
        if authoring(R[k]):
            return None
    return None


class Model:
    def __init__(self, sessions, neg_keep=0.25, seed=0):
        self.pool = Pool(sessions)
        rng = np.random.default_rng(seed)
        X, y = [], []
        for s in sessions:
            R = records(s)
            for t, cs in points(self.pool, s, R):
                for k, src, rr, f in cs:
                    lab = label_window(R, t, k) is not None
                    if lab or rng.random() < neg_keep:
                        X.append(f)
                        y.append(lab)
        X = np.array(X)
        y = np.array(y)
        w = np.where(y, 1.0, 1.0 / neg_keep)
        self.clf = HistGradientBoostingClassifier(max_iter=200, learning_rate=0.08, max_leaf_nodes=31,
                                                  min_samples_leaf=40, random_state=seed)
        self.clf.fit(X, y, sample_weight=w)
        self.n_train = len(y)
        self.pos_rate = float(y.mean()) if len(y) else 0.0

    def top(self, s, R=None):
        """[(t, key, src, cand_rec, p)] best candidate per decision point."""
        R = R if R is not None else records(s)
        res = []
        for t, cs in points(self.pool, s, R):
            if not cs:
                res.append((t, None, None, None, 0.0))
                continue
            F = np.stack([c[3] for c in cs])
            p = self.clf.predict_proba(F)[:, 1]
            i = int(np.argmax(p))
            res.append((t, cs[i][0], cs[i][1], cs[i][2], float(p[i])))
        return res
