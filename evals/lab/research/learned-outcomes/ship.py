"""What would ship: the template dictionary + a tiny classifier, its size and its latency.

Two shippable forms, both fitted on the non-control pool:
  tiny   Drain template dictionary + a logistic regression over hashed templates (no model file)
  full   the same + potion-8M embeddings of the output and the action (the model Adage already pins)
Latency is per tool output, single thread, end to end from raw output text to a failure
probability (the PostToolUse hook's work).

  python3 ship.py -> results/ship.json
"""

from __future__ import annotations

import json
import pickle
import sys
import time
from pathlib import Path

import numpy as np
from scipy.sparse import csr_matrix, hstack

import features as F
import models as M

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import embed  # noqa: E402


def main():
    a = F.table()
    T = F.outputs()
    acts = [F.act_text(c["tool"], c["arg"]) for s in F.load.sessions() for c in s["calls"]]
    pool = a["arm"] != "control"
    t0 = time.time()
    tm = F.drain_miner()
    for t, f in zip(T, pool):
        if f:
            for l in F.lines_of(t):
                tm.add_log_message(l)
    fit_s = time.time() - t0
    tp = pickle.loads((F.load.CACHE / "tpl_main.pkl").read_bytes())
    H = F.hash_templates(tp)
    y = a["is_error"]
    tiny = M.lr(H[pool], y[pool])
    EO, EA = a["EO"].astype(np.float32), a["EA"].astype(np.float32)
    vocab = {t: i for i, t in enumerate(sorted(set(a["tool"][pool])))}
    Tm = M.tool_onehot(a["tool"], vocab)
    ln = np.c_[np.log1p(a["out_len"]) / 10, (a["out_len"] == 0)].astype(np.float32)
    X = hstack([H, csr_matrix(np.c_[EO, EA, Tm, ln])]).tocsr()
    full = M.lr(X[pool], y[pool], C=0.5)
    dict_bytes = len(pickle.dumps(tm.drain))
    nclusters = len(tm.drain.clusters)
    m = embed.load("potion-8M")
    test = np.nonzero(~pool)[0][:300]
    lat_tiny, lat_full = [], []
    for i in test:
        t1 = time.perf_counter()
        tpl = []
        for l in F.lines_of(T[i]):
            c = tm.match(l, full_search_strategy="never")
            tpl.append(c.get_template() if c else "<unseen>")
        h = F.hash_templates([tpl])
        tiny.predict_proba(h)
        t2 = time.perf_counter()
        eo = m.encode([F.out_text(T[i])])
        ea = m.encode([acts[i]])
        tl = np.zeros((1, len(vocab) + 1), np.float32)
        tl[0, vocab.get(a["tool"][i], len(vocab))] = 1
        x = hstack([h, csr_matrix(np.c_[eo, ea, tl, ln[i:i + 1]])]).tocsr()
        full.predict_proba(x)
        t3 = time.perf_counter()
        lat_tiny.append((t2 - t1) * 1000)
        lat_full.append((t3 - t1) * 1000)
    q = lambda v, p: round(float(np.percentile(v, p)), 3)
    res = {"drain_fit_seconds_on_pool": round(fit_s, 1), "pool_calls": int(pool.sum()),
           "template_clusters": nclusters, "template_dictionary_bytes_pickled": dict_bytes,
           "tiny_classifier_weights": int(np.count_nonzero(tiny.coef_)),
           "full_classifier_weights": int(np.count_nonzero(full.coef_)),
           "latency_ms_tiny": {"median": q(lat_tiny, 50), "p95": q(lat_tiny, 95)},
           "latency_ms_full": {"median": q(lat_full, 50), "p95": q(lat_full, 95)},
           "note": "single thread, shared 4-vCPU container under load ~30; Python; Rust would be faster"}
    # most failure-indicative templates (the learned dictionary, by weight)
    from collections import Counter
    cnt = Counter(t for i in np.nonzero(pool)[0] for t in tp[i])
    w = full.coef_[0][:3 * F.NH + 1]
    import hashlib
    top = []
    for t, c in cnt.most_common(4000):
        h = int(hashlib.md5(t.encode()).hexdigest()[:8], 16) % F.NH
        top.append((float(w[h]), c, t[:120]))
    top.sort(reverse=True)
    res["top_failure_templates"] = [{"weight": round(x, 2), "count": c, "template": t} for x, c, t in top[:25]]
    res["top_success_templates"] = [{"weight": round(x, 2), "count": c, "template": t} for x, c, t in top[-15:]]
    (M.HERE / "results" / "ship.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
