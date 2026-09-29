"""Precompute and hook-time cost of the recommended retriever (BM25 + potion-code-16M-v2 chunks,
fused, plus co-change), on the largest repository here (vue-core, ~700 files) and a small one.

Precompute (learn time / SessionStart, keyed by the HEAD tree; incremental per changed blob):
BM25 postings, chunk embeddings, co-change matrix. Hook time (UserPromptSubmit): tokenise and embed
the prompt, one sparse and one dense product, max-pool, one sparse propagation, top-k.
"""
import time

import numpy as np

import repo as R
import retrieve as RT


def main():
    for repo in ("click", "vue-core"):
        t = list(R.tasks(repo).values())[-1]
        RT._emb.clear()
        t0 = time.perf_counter()
        ix = RT.Index(repo, t["base"])
        ix.bm25("x")
        t1 = time.perf_counter()
        E, owner = [], []
        m = RT.model("code16")
        for i, f in enumerate(ix.files):
            e = m.encode(RT.chunks(f, R.text(repo, ix.tree[f])))
            E.append(e); owner += [i] * len(e)
        E = np.concatenate(E).astype(np.float32); owner = np.asarray(owner)
        t2 = time.perf_counter()
        C, hot = ix.cochange()
        t3 = time.perf_counter()
        ts = []
        for tt in list(R.tasks(repo).values())[:30]:
            q = tt["prompt"]
            a = time.perf_counter()
            b = ix.bm25(q)
            qv = m.encode([q])[0]
            fe = np.full(len(ix.files), -1.0, np.float32)
            np.maximum.at(fe, owner, E @ qv)
            z = RT.norm(b) + RT.norm(fe)
            z = z + 0.3 * RT.norm(RT.propagate(C, z))
            top = np.argsort(-z)[:5]
            ts.append((time.perf_counter() - a) * 1000)
        print(f"{repo}: {len(ix.files)} files, {len(E)} chunks | precompute: BM25 {t1 - t0:.1f}s, "
              f"embeddings {t2 - t1:.1f}s, co-change {t3 - t2:.1f}s | index {E.nbytes / 1e6:.1f} MB f32 "
              f"| hook query median {np.median(ts):.1f} ms, p95 {np.percentile(ts, 95):.1f} ms (Python, shared 4-vCPU)")


if __name__ == "__main__":
    main()
