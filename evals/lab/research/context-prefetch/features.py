"""Compute every content/graph scorer for every task (all 188 RepoBench tasks), cached.

features.pkl: {task_id: {"repo", "files": [...], "f": {name: float32[len(files)]}}}
"""

from __future__ import annotations

import pickle
import sys
import time

import numpy as np

import repo as R
import retrieve as RT

OUT = RT.CACHE / "features.pkl"
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
EMB = ["p8", "p32", "code16"]


def task_features(repo: str, t: dict) -> dict:
    ix = RT.index(repo, t["base"])
    q = t["prompt"]
    f = {"bm25": ix.bm25(q), "bm25c": ix.bm25c(q)}
    for m in EMB:
        f[m] = ix.emb(m, q)
    C, hot = ix.cochange()
    A = ix.ref()
    content = RT.norm(f["bm25"]) + RT.norm(f["code16"])
    f["hot"] = hot
    f["co_bm25"] = RT.propagate(C, f["bm25"])
    f["co_content"] = RT.propagate(C, content)
    f["ref_content"] = RT.propagate(A, content)
    return {"repo": repo, "files": ix.files, "f": f}


def main():
    feats = pickle.loads(OUT.read_bytes()) if OUT.exists() and "--refresh" not in sys.argv else {}
    for repo in REPOS:
        t0 = time.time()
        for tid, t in R.tasks(repo).items():
            if tid not in feats:
                feats[tid] = task_features(repo, t)
        RT.save_emb()
        RT._emb.clear()
        RT.index.cache_clear()
        OUT.write_bytes(pickle.dumps(feats))
        print(repo, f"{time.time() - t0:.0f}s", flush=True)
    OUT.write_bytes(pickle.dumps(feats))


if __name__ == "__main__":
    main()
