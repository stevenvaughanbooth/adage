"""Hook-time cost of a learned action representation: model size and per-command latency.

The hook's work for one command: shell-lex it, look up its features in the vocabulary (a hash
map), sum the rows of W (p x d) for the ~10-40 features present, L2-normalise, and take the
nearest of K prototypes (K x d dot products). Timed here in Python (single thread) as an upper
bound; the operation count is reported so the Rust cost can be estimated.
"""
import json
import sys
import time

import numpy as np

import common as C
import data
import feats as FE
import methods as M


def main(name):
    S = C.sessions()
    taus = json.load(open(data.HERE / "results" / "taus.json"))
    m = C.clustered(name, S)
    tau = taus[name]
    P = m.cut(tau)
    emb = m.emb
    fe = emb.lex.fe if hasattr(emb, "lex") else emb.fe
    W = emb.W if hasattr(emb, "W") else None
    held = [r.cmd for s in S if s.arm == "control" for r in s.recs if r.tool == "Bash" and r.cmd.strip()][:2000]
    nfeat = []
    t0 = time.perf_counter()
    for c in held:
        f = FE.raw_features(c, fe.bpe)
        nfeat.append(sum(1 for x in f if x in fe.vocab))
    t_feat = (time.perf_counter() - t0) / len(held)
    X = fe.transform(held)
    t0 = time.perf_counter()
    for i in range(X.shape[0]):
        e = emb.embed([held[i]]) if W is None else X[i] @ W
    t_embed_total = (time.perf_counter() - t0) / len(held)
    E = emb.embed(held)
    t0 = time.perf_counter()
    for i in range(len(held)):
        (P @ E[i]).argmax()
    t_nn = (time.perf_counter() - t0) / len(held)
    d = E.shape[1]
    wbytes = (W.size * 4) if W is not None else None
    out = {"method": name, "tau": tau, "vocab": len(fe.vocab), "dim": d, "prototypes": int(P.shape[0]),
           "W_bytes_f32": wbytes, "prototypes_bytes_f32": int(P.size * 4),
           "bpe_vocab": fe.bpe.tok.get_vocab_size() if fe.bpe else 0,
           "features_per_cmd_mean": float(np.mean(nfeat)),
           "python_us": {"lex+features": t_feat * 1e6, "project": t_embed_total * 1e6, "nearest_prototype": t_nn * 1e6},
           "ops_per_cmd": {"feature_lookups": float(np.mean(nfeat)), "mults_project": float(np.mean(nfeat)) * d, "mults_nearest": int(P.shape[0]) * d}}
    print(json.dumps(out, indent=1))
    json.dump(out, open(data.HERE / "results" / f"bench_{name}.json", "w"), indent=1)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "a2v")
