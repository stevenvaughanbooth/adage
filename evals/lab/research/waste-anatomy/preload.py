"""The biggest ceiling is orientation. Can the right files be predicted before the session, and what
is that worth in $ once pre-loaded content is paid for on every later request?

Predictors of a held-out session's *needed files* (files whose Read is in the provenance slice of
the final diff, plus the files of surviving edits), no hand-coding:

- `memory`: past sessions of the same repository (the TRAIN split). TF-IDF cosine between the
  held-out prompt and each training prompt; a file's score is the similarity-weighted count of
  training sessions that needed it. This is what Adage's traces can offer.
- `bm25`: BM25 of the prompt against every source file (path + content) of the task's base commit.
  Needs no traces at all, so it is leave-one-repo-out by construction.
- `hybrid`: rank-sum of the two.

Translation to $ (`reprice` from ceilings.py): pre-load the top-k files' full contents at request 0
(as a SessionStart hook would); every Read of a pre-loaded file is removed except the first Read of
a file the agent edits (Claude Code's Edit tool refuses a file that was not Read in the session).
`alpha` = the share of the *remaining unneeded* orientation calls that also disappear; 0 is
conservative (the agent explores as before), `alpha = coverage` assumes the unneeded exploration
shrinks in proportion to the share of needed files that were supplied.
"""

from __future__ import annotations

import gzip
import json
import math
import pickle
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ceilings import reprice, boot_mean, boot_ratio  # noqa: E402

CACHE = Path("/tmp/adage-research/waste-anatomy")
HERE = Path(__file__).resolve().parent
TASKS = HERE.parents[2] / "repobench" / "tasks"
SRC_EXT = re.compile(r"\.(py|pyi|go|ts|tsx|js|mjs|vue|rs)$")
WORD = re.compile(r"[A-Za-z_][A-Za-z0-9_]+")


def split_ident(t: str) -> list[str]:
    parts = re.sub(r"([a-z])([A-Z])", r"\1 \2", t).replace("_", " ").lower().split()
    return [t.lower()] + parts if len(parts) > 1 else [t.lower()]


def words(s: str) -> list[str]:
    out = []
    for t in WORD.findall(s):
        out += split_ident(t)
    return out


_SNAP: dict = {}


def snapshot(sess_meta) -> dict[str, str]:
    if sess_meta["task"] in _SNAP:
        return _SNAP[sess_meta["task"]]
    _SNAP[sess_meta["task"]] = _snapshot(sess_meta)
    return _SNAP[sess_meta["task"]]


def _snapshot(sess_meta) -> dict[str, str]:
    base = {"adage-rb2": "/tmp/adage-rb2/runs", "adage-repobench": "/tmp/adage-repobench/runs"}[sess_meta["base"]]
    repo = Path(base) / sess_meta["repo_dir"] / "runs" / sess_meta["rid"] / "repo"
    root = subprocess.run(["git", "-C", str(repo), "rev-list", "--max-parents=0", "HEAD"], capture_output=True,
                          text=True).stdout.split()[0]
    names = subprocess.run(["git", "-C", str(repo), "ls-tree", "-r", "--name-only", root], capture_output=True,
                           text=True).stdout.split("\n")
    out = {}
    for f in names:
        if SRC_EXT.search(f) and "node_modules" not in f:
            r = subprocess.run(["git", "-C", str(repo), "show", f"{root}:{f}"], capture_output=True, text=True)
            if r.returncode == 0 and len(r.stdout) < 400_000:
                out[f] = r.stdout
    return out  # in memory only (shared disk is nearly full)


class BM25:
    def __init__(self, docs: dict[str, str], k1=1.2, b=0.75):
        self.names = list(docs)
        self.tf = [Counter(words(n) * 3 + words(docs[n])) for n in self.names]
        self.len = np.array([sum(t.values()) for t in self.tf], float)
        self.avg = self.len.mean() if len(self.len) else 1
        df = Counter()
        for t in self.tf:
            df.update(t.keys())
        N = len(self.tf)
        self.idf = {w: math.log(1 + (N - n + 0.5) / (n + 0.5)) for w, n in df.items()}
        self.k1, self.b = k1, b

    def rank(self, q: str) -> list[str]:
        qw = set(words(q))
        sc = np.zeros(len(self.names))
        for i, t in enumerate(self.tf):
            s = 0.0
            for w in qw:
                f = t.get(w)
                if f:
                    s += self.idf[w] * f * (self.k1 + 1) / (f + self.k1 * (1 - self.b + self.b * self.len[i] / self.avg))
            sc[i] = s
        return [self.names[i] for i in np.argsort(-sc)]


def needed_files(s) -> set[str]:
    f = {c["file"] for c in s["calls"] if c["needed"] and c["name"] == "Read"}
    f |= {c["file"] for c in s["calls"] if c["survive"]}
    return {x for x in f if x and not x.startswith("/")}


def memory_rank(s, train, vec):
    if not train:
        return []
    q = vec.transform([s["prompt"]])
    T = vec.transform([t["prompt"] for t in train])
    sim = (T @ q.T).toarray().ravel()
    sc = Counter()
    for t, w in zip(train, sim):
        for f in needed_files(t):
            sc[f] += w
    return [f for f, _ in sc.most_common()]


def hybrid(a: list[str], b: list[str], n=50) -> list[str]:
    sc = Counter()
    for r, f in enumerate(a[:n]):
        sc[f] += 1 / (r + 1)
    for r, f in enumerate(b[:n]):
        sc[f] += 1 / (r + 1)
    return [f for f, _ in sc.most_common()]


def file_tokens(content: str) -> float:
    return (len(content) + 7 * content.count("\n")) / 3.08


def simulate(s, files: list[str], snap: dict, alpha: float):
    """Pre-load `files`; return (calls, tokens, $)."""
    pre = set(files)
    add = sum(file_tokens(snap.get(f, "")) for f in pre)
    edited = {c["file"] for c in s["calls"] if c["role"] == "edit"}
    first_read_of_edited = {}
    for c in s["calls"]:
        if c["name"] == "Read" and c["file"] in edited and c["file"] not in first_read_of_edited:
            first_read_of_edited[c["file"]] = c["idx"]
    remove = set()
    for c in s["calls"]:
        if c["name"] == "Read" and c["file"] in pre and first_read_of_edited.get(c["file"]) != c["idx"]:
            remove.add(c["idx"])
    unneeded = [c for c in s["calls"] if c["role"] == "explore" and not c["needed"] and c["idx"] not in remove]
    m = int(round(alpha * len(unneeded)))
    # remove the most expensive unneeded orientation first? no: remove a deterministic evenly spaced subset
    if m:
        step = len(unneeded) / m
        remove |= {unneeded[int(i * step)]["idx"] for i in range(m)}
    return reprice(s, remove=frozenset(remove), prefix_add=add)


def main():
    from sklearn.feature_extraction.text import TfidfVectorizer
    S = pickle.load((CACHE / "calls.pkl").open("rb"))
    meta = {}
    for l in gzip.open(CACHE / "sessions.jsonl.gz", "rt"):
        d = json.loads(l)
        meta[d["sid"]] = {k: d[k] for k in ("base", "repo_dir", "rid", "repo", "task")}
    ctrl = [s for s in S if s["arm"] == "control" and (s["base"] == "adage-rb2" or s["repo"] == "urfave-cli")]
    train = defaultdict(list)
    for s in S:
        if s["arm"] == "train":
            train[s["repo"]].append(s)
    gold = {}
    for f in TASKS.glob("*.json"):
        for t in json.loads(f.read_text())["tasks"]:
            gold[t["id"]] = set(eval(t["src_files"])) if isinstance(t["src_files"], str) else set(t["src_files"])

    # instrument check: are reads of the task's gold source files in the needed slice?
    g_need, g_n, o_need, o_n = 0, 0, 0, 0
    for s in ctrl:
        gs = gold.get(s["task"], set())
        for c in s["calls"]:
            if c["name"] == "Read":
                if c["file"] in gs:
                    g_n += 1
                    g_need += c["needed"]
                else:
                    o_n += 1
                    o_need += c["needed"]
    per_file = []  # does each session's needed slice include at least one read of each gold src file it read?
    for s in ctrl:
        gs = gold.get(s["task"], set())
        read = {c["file"] for c in s["calls"] if c["name"] == "Read"}
        for f in gs & read:
            per_file.append(any(c["needed"] for c in s["calls"] if c["name"] == "Read" and c["file"] == f))
    check = {"reads_of_gold_src_needed": g_need / max(g_n, 1), "n_gold_reads": g_n,
             "reads_of_other_files_needed": o_need / max(o_n, 1), "n_other_reads": o_n,
             "gold_src_file_read_has_a_needed_read": float(np.mean(per_file)), "n_gold_files": len(per_file)}

    vecs = {r: TfidfVectorizer(sublinear_tf=True).fit([t["prompt"] for t in train[r]] or ["x"]) for r in train}
    ranks, bm25s = {}, {}
    for s in ctrl:
        snap = snapshot(meta[s["sid"]])
        if s["task"] not in bm25s:
            bm25s[s["task"]] = BM25(snap)
        bm = bm25s[s["task"]].rank(s["prompt"])
        mem = memory_rank(s, train[s["repo"]], vecs[s["repo"]])
        ranks[s["sid"]] = {"memory": mem, "bm25": bm, "hybrid": hybrid(mem, bm),
                           "gold_src (task metadata, not available live)": sorted(gold.get(s["task"], []))}
    rec = {}
    for m in ("memory", "bm25", "hybrid", "gold_src (task metadata, not available live)"):
        for k in (1, 3, 5):
            r = []
            for s in ctrl:
                nf = needed_files(s)
                if nf:
                    r.append(len(set(ranks[s["sid"]][m][:k]) & nf) / len(nf))
            rec[f"{m}@{k}"] = boot_mean(r)
    # translate to calls and $
    trans = {}
    base = np.array([reprice(s) for s in ctrl])
    for m in ("memory", "bm25", "hybrid", "gold_src (task metadata, not available live)"):
        for k in (1, 2, 3):
            for mode in ("alpha0", "alpha_cov"):
                v = []
                for s in ctrl:
                    snap = snapshot(meta[s["sid"]])
                    files = ranks[s["sid"]][m][:k]
                    nf = needed_files(s)
                    cov = len(set(files) & nf) / len(nf) if nf else 0
                    v.append(simulate(s, files, snap, 0.0 if mode == "alpha0" else cov))
                v = np.array(v)
                d = base - v
                trans[f"{m}@{k}/{mode}"] = {"calls_saved": boot_mean(d[:, 0]), "usd_saved_share": boot_ratio(d[:, 2], base[:, 2]),
                                            "usd_saved": boot_mean(d[:, 2])}
    # the pure cost of carrying a pre-loaded file: $ per session per 1k tokens pre-loaded
    carry = [reprice(s, prefix_add=1000)[2] - reprice(s)[2] for s in ctrl]
    out = {"n": len(ctrl), "instrument_check": check, "recall_of_needed_files": rec, "translation": trans,
           "usd_per_1k_preloaded_tokens": boot_mean(carry),
           "needed_files_per_session": boot_mean([len(needed_files(s)) for s in ctrl]),
           "mean_file_tokens_needed": float(np.mean([file_tokens(snapshot(meta[s["sid"]]).get(f, "")) for s in ctrl
                                                     for f in needed_files(s) if f in snapshot(meta[s["sid"]])]))}
    (HERE / "preload.json").write_text(json.dumps(out, indent=1))
    print(json.dumps(out, indent=1)[:6000])


if __name__ == "__main__":
    main()
