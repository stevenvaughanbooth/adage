"""Targeted RANGES instead of files: shrink the agent's whole-file Reads of the files it needs to the
line windows that matter, and price what that saves.

The mechanism a hook could deliver: at UserPromptSubmit, "relevant: src/click/core.py lines
1041-1160 (Command.get_short_help_str)", or at PreToolUse on a whole-file Read of a retrieved file, an
`updatedInput` with offset/limit set to the window. Either way the counterfactual is the same: the
first whole-file Read of a target file returns only the window.

- The windows come from the same retriever (60-line chunks scored against the prompt: BM25 over
  the chunk's identifier tokens + potion-code-16M-v2 cosine, z-scored within the file), top-m
  chunks per file, merged.
- A window is *sufficient* for an edited file if every line the agent's edits touched (located from
  each Edit's old_string in the evolving file) lies inside it (± 0 lines). For a necessary read
  that was not edited there is no ground truth of which lines mattered, so it is priced at the
  edited files' measured sufficiency rate (flagged).
- Price (harness cost model): a sufficient window shrinks that call's share of the context growth
  by (1 − window/file), written once and read by every later response. An insufficient window
  costs the window's tokens plus one extra request (the agent re-reads the whole file).
- Oracle: the window = the edited lines ± 30, always sufficient.

Only the first whole-file (no offset/limit) Read of each target file is changed; reads the agent
already windowed are left alone.
"""

from __future__ import annotations

import collections
import json
import pickle
from pathlib import Path

import numpy as np

import data
import repo as R
import retrieve as RT
import targets as TG

EDIT = {"Edit", "MultiEdit"}


def edit_lines(s, d) -> dict:
    """file -> set of line numbers (1-based, in the base file's numbering, approximately) the
    session's edits touched."""
    cur = {}
    out = collections.defaultdict(set)
    for c in s.calls:
        if c.tool not in EDIT:
            continue
        f = c.paths[0] if c.paths else None
        if f not in d["tree"]:
            continue
        if f not in cur:
            cur[f] = R.text(s.repo, d["tree"][f])
        edits = c.inp.get("edits") if c.tool == "MultiEdit" else [c.inp]
        for e in edits or []:
            old, new = e.get("old_string", ""), e.get("new_string", "")
            if not old:
                continue
            i = cur[f].find(old)
            if i < 0:
                continue
            a = cur[f].count("\n", 0, i) + 1
            b = a + old.count("\n")
            out[f].update(range(a, b + 1))
            cur[f] = cur[f][:i] + new + cur[f][i + len(old):]
    return out


def chunk_scores(ix, f: str, q: str, qv) -> np.ndarray:
    text = R.text(ix.repo, ix.tree[f])
    chs = RT.chunks(f, text)
    E = RT.blob_chunks_emb(ix.repo, "code16", f, ix.tree[f]).astype(np.float32)
    emb = E @ qv
    qt = set(RT.tokens(q))
    bm = np.array([sum(1 for w in set(RT.tokens(c)) if w in qt) for c in chs], np.float32)
    return RT.norm(emb) + RT.norm(bm) if len(chs) > 1 else emb


def window(scores: np.ndarray, m: int, n_lines: int) -> set:
    top = np.argsort(-scores)[:m]
    lines = set()
    for c in top:
        lines.update(range(c * RT.CHUNK_LINES + 1, min((c + 1) * RT.CHUNK_LINES, n_lines) + 1))
    return lines


def shrink_usd(cm, k: int, frac_removed: float) -> float:
    """$ saved if call k's share of its response's context growth shrinks by frac_removed."""
    j = cm.resp_of.get(k)
    if j is None:
        return 0.0
    later = max(0, cm.n - j - 2)
    dtok = cm.growth[j] * cm.weight[k] * frac_removed
    return dtok * (cm.wrate[min(j + 1, cm.n - 1)] + cm.rate[j][3] * later)


def add_usd(cm, k: int, tokens: float, extra_request: bool) -> float:
    """$ cost of `tokens` more context after call k, plus one extra request at that point."""
    j = cm.resp_of.get(k, 0)
    later = max(0, cm.n - j - 2)
    cost = tokens * (cm.wrate[min(j + 1, cm.n - 1)] + cm.rate[j][3] * later)
    if extra_request:
        R_ = cm.s.responses[min(j + 1, cm.n - 1)]
        cost += R_.cache_read * cm.rate[j][3] + R_.output * cm.rate[j][1]
    return cost


def main():
    S = data.load()
    info = TG.annotate(S)
    split = {r: set(R.split(r)["test"]) for r in ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]}
    held = [s for s in S if s.phase == "eval" and s.arm == "control" and s.sid in info and s.task in split[s.repo]]
    res = collections.defaultdict(lambda: collections.defaultdict(list))
    suff = collections.defaultdict(list)
    anatomy = collections.defaultdict(list)
    for s in held:
        d = info[s.sid]
        cm = data.hcost(s.sid)
        base = cm.total
        el = edit_lines(s, d)
        t = R.tasks(s.repo)[s.task]
        ix = RT.index(s.repo, d["base"])
        qv = RT.model("code16").encode([s.prompt])[0]
        seen = set()
        # anatomy: all Read results vs whole-file reads of targets
        read_usd = sum(shrink_usd(cm, k, 1.0) for k, c in enumerate(s.calls) if c.tool == "Read")
        anatomy["read_share"].append(read_usd / base)
        tgt_full = 0.0
        acc = collections.defaultdict(float)
        for k, c in enumerate(s.calls):
            if c.tool != "Read" or not c.paths:
                continue
            f = c.paths[0]
            if f not in d["T"] or f in seen or c.inp.get("limit") or c.inp.get("offset"):
                continue
            seen.add(f)
            text = R.text(s.repo, d["tree"][f])
            n_lines = text.count("\n") + 1
            full = shrink_usd(cm, k, 1.0)
            tgt_full += full
            # oracle: edited lines +- 30
            if f in el:
                w = set()
                for x in el[f]:
                    w.update(range(max(1, x - 30), min(n_lines, x + 30) + 1))
                acc["oracle"] += shrink_usd(cm, k, max(0.0, 1 - len(w) / n_lines))
            sc = chunk_scores(ix, f, s.prompt, qv)
            for m in (1, 2, 3, 4):
                w = window(sc, m, n_lines)
                frac = len(w) / n_lines
                if frac >= 1:
                    continue
                if f in el:
                    ok = el[f] <= w
                    suff[m].append(ok)
                    if ok:
                        acc[f"m{m}:E"] += shrink_usd(cm, k, 1 - frac)
                    else:
                        acc[f"m{m}:E"] -= add_usd(cm, k, frac * full_tokens(cm, k), True)
                else:
                    acc[f"m{m}:N_pending"] += 0  # priced below at the measured rate
                    acc[f"m{m}:N_full"] += shrink_usd(cm, k, 1 - frac)
                    acc[f"m{m}:N_miss"] += add_usd(cm, k, frac * full_tokens(cm, k), True)
        anatomy["target_full_read_share"].append(tgt_full / base)
        for key in ["oracle"] + [f"m{m}:E" for m in (1, 2, 3, 4)]:
            res[key]["usd"].append(acc[key])
        for m in (1, 2, 3, 4):
            res[f"m{m}:N"]["full"].append(acc[f"m{m}:N_full"])
            res[f"m{m}:N"]["miss"].append(acc[f"m{m}:N_miss"])
        for key in res:
            pass
        res["_base"]["usd"].append(base)
        res["_model"]["m"].append(s.model)
    base = np.array(res["_base"]["usd"])
    out = {"sessions": len(held),
           "read_results_share_of_usd": round(float(np.mean(anatomy["read_share"])), 3),
           "target_whole_file_first_reads_share": round(float(np.sum(np.array(anatomy["target_full_read_share"]) * base) / base.sum()), 4)}

    def ratio(x):
        x = np.asarray(x)
        rng = np.random.default_rng(0)
        idx = rng.integers(0, len(x), (2000, len(x)))
        r = x[idx].sum(1) / base[idx].sum(1)
        return [round(float(x.sum() / base.sum()), 4), round(float(np.percentile(r, 5)), 4), round(float(np.percentile(r, 95)), 4)]
    out["oracle_edit_window_pm30"] = ratio(res["oracle"]["usd"])
    for m in (1, 2, 3, 4):
        p = float(np.mean(suff[m])) if suff[m] else 0.0
        e = np.asarray(res[f"m{m}:E"]["usd"])
        n = p * np.asarray(res[f"m{m}:N"]["full"]) - (1 - p) * np.asarray(res[f"m{m}:N"]["miss"])
        out[f"top{m}_chunks"] = dict(window_lines=m * RT.CHUNK_LINES, sufficiency_on_edited=round(p, 3),
                                     n_edited_reads=len(suff[m]), edited_only=ratio(e), edited_plus_reads_at_rate=ratio(e + n))
    print(json.dumps(out, indent=1))
    (Path(__file__).parent / "ranges.json").write_text(json.dumps(out, indent=1))


def full_tokens(cm, k):
    j = cm.resp_of.get(k)
    return cm.growth[j] * cm.weight[k] if j is not None else 0.0


if __name__ == "__main__":
    main()
