"""Per-call table: metadata, potion-8M embeddings of each call's output and action, Drain line
templates, and the self-supervised targets read from what the agent did next.

Nothing here matches a hand-written string list. The only structure used is: the harness's
`is_error` flag (the exit status the agent itself saw), the tool name as an opaque categorical,
action similarity (cosine of the action's embedding), the session order, and the hidden-test
outcome in `state.json`.

Self-supervised targets (from the agent's next moves; k = 3 calls, τ = 0.9 cosine):
  - `retry`:    a similar action (cos ≥ τ, same tool) follows within k calls: the agent tried again.
  - `recover`:  a similar action came within the previous k calls and one of those had `retry`
                set, and no similar action follows within k: the last attempt of a retry chain,
                after which the agent moved on (a success).
  - `moved_on`: no similar action within k calls either side.
"""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path

import numpy as np

import load

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # evals/lab
sys.path.insert(0, str(load.CACHE))  # vendored drain3
import embed  # noqa: E402

TABLE = load.CACHE / "table.npz"
K, TAU = 3, 0.9
NH = 2048  # hashed Drain-template buckets


def out_text(o: str) -> str:
    o = o.strip()
    return o if len(o) <= 1000 else o[:600] + "\n" + o[-400:]


def act_text(tool: str, arg: str) -> str:
    return f"{tool} {arg[:300]}"


def lines_of(o: str, n: int = 6) -> list[str]:
    ls = [l.strip()[:200] for l in o.splitlines() if l.strip()]
    if len(ls) <= 2 * n:
        return ls
    return ls[:n] + ls[-n:]


def build():
    rows, outs, acts = [], [], []
    for s in load.sessions():
        for i, c in enumerate(s["calls"]):
            rows.append((s["sid"], s["repo"], s["base_repo"], s["arm"], s["phase"], s["bench"], s["task"],
                         -1 if s["passed"] is None else int(s["passed"]), i, len(s["calls"]), c["tool"],
                         int(c["is_error"]), c["out_len"], c["batch"], s["cost_usd"] or 0.0))
            outs.append(out_text(c["output"]))
            acts.append(act_text(c["tool"], c["arg"]))
    m = embed.load("potion-8M")
    EO = m.encode(outs).astype(np.float16)
    EA = m.encode(acts).astype(np.float16)
    cols = list(zip(*rows))
    names = ["sid", "repo", "base_repo", "arm", "phase", "bench", "task", "passed", "idx", "n", "tool", "is_error",
             "out_len", "batch", "cost"]
    arr = {k: np.array(v) for k, v in zip(names, cols)}
    arr.update(EO=EO, EA=EA)
    arr.update(weak_targets(arr))
    np.savez_compressed(TABLE, **arr)
    print("table", len(rows), "->", TABLE, file=sys.stderr)


def weak_targets(a):
    n = len(a["sid"])
    EA = a["EA"].astype(np.float32)
    retry = np.zeros(n, np.int8)
    prev_sim = np.zeros(n, np.int8)
    starts = np.r_[0, np.nonzero(a["sid"][1:] != a["sid"][:-1])[0] + 1, n]
    for s, e in zip(starts[:-1], starts[1:]):
        for i in range(s, e):
            for j in range(i + 1, min(e, i + 1 + K)):
                if a["tool"][j] == a["tool"][i] and float(EA[i] @ EA[j]) >= TAU:
                    retry[i] = 1
                    prev_sim[j] = 1
    recover = np.zeros(n, np.int8)
    for s, e in zip(starts[:-1], starts[1:]):
        for i in range(s, e):
            if retry[i]:
                continue
            for j in range(max(s, i - K), i):
                if retry[j] and a["tool"][j] == a["tool"][i] and float(EA[i] @ EA[j]) >= TAU:
                    recover[i] = 1
                    break
    moved_on = ((retry == 0) & (prev_sim == 0)).astype(np.int8)
    return {"retry": retry, "recover": recover, "moved_on": moved_on}


def table():
    z = np.load(TABLE, allow_pickle=False)
    return {k: z[k] for k in z.files}


# ------------------------------------------------------------------------------------------------
# Drain templates (unsupervised log-template mining), fitted per fold on the training pool only.
# ------------------------------------------------------------------------------------------------

def outputs():
    out = []
    for s in load.sessions():
        for c in s["calls"]:
            out.append(c["output"])
    return out


def drain_miner(sim_th: float = 0.4):
    from drain3 import TemplateMiner
    from drain3.template_miner_config import TemplateMinerConfig
    cfg = TemplateMinerConfig()
    cfg.drain_sim_th = sim_th
    cfg.drain_depth = 4
    cfg.profiling_enabled = False
    return TemplateMiner(config=cfg)


def drain_fit(texts: list[str], fit_mask: np.ndarray):
    """Fit Drain on the lines of the fit outputs; return per-output line templates (strings) for
    every output (unseen lines match the nearest cluster, or are `<unseen>`)."""
    tm = drain_miner()
    for t, f in zip(texts, fit_mask):
        if f:
            for l in lines_of(t):
                tm.add_log_message(l)
    res = []
    for t in texts:
        ls = lines_of(t)
        tpl = []
        for l in ls:
            c = tm.match(l, full_search_strategy="fallback")
            tpl.append(c.get_template() if c is not None else "<unseen>")
        res.append(tpl)
    return tm, res


def hash_templates(tpls: list[list[str]]):
    """Binary hashed bag of line templates, with the first and last line's template in their own
    buckets, plus two shape features (empty output; number of lines)."""
    from scipy.sparse import csr_matrix
    ind, ptr, data = [], [0], []
    for tp in tpls:
        cols = set()
        for k, t in enumerate(tp):
            h = int(hashlib.md5(t.encode()).hexdigest()[:8], 16)
            cols.add(h % NH)
            if k == 0:
                cols.add(NH + h % NH)
            if k == len(tp) - 1:
                cols.add(2 * NH + h % NH)
        if not tp:
            cols.add(3 * NH)
        ind.extend(sorted(cols))
        data.extend([1.0] * len(cols))
        ptr.append(len(ind))
    return csr_matrix((data, ind, ptr), shape=(len(tpls), 3 * NH + 1), dtype=np.float32)


if __name__ == "__main__":
    build()
