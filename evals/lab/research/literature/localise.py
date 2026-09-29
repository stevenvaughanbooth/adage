"""Localisation lever: how much of a session is spent before the agent first touches a file it has
to change, and could a zero-LLM retriever (BM25, potion-code-16M-v2, a prior learned from the
repository's training traces) have pointed there at the prompt?

Literature: Agentless (hierarchical localisation), SWE-bench's BM25 retrieval baseline, aider's
repo map, Cursor's semantic search (+12.5% answer accuracy), Semble (potion-code + BM25). All
claim the localisation phase is where retrieval pays. This script measures, on held-out control
sessions:

  1. `touch`: the index of the first request whose tool call names a gold source file (Read/Edit
     path, or a Bash/Grep argument containing it) and the cost share of the requests before it
     (cumulative-context model, the same counterfactual as cost_anatomy.py);
  2. file-level hit@k of each retriever, with the task prompt as the query, over the repository's
     code files at the task's base commit (read from git, never the agent's edited working tree);
  3. the translation: expected $ share saved if the top-k list were delivered at the prompt =
     mean over sessions of hit@k(task) x cost share of the requests between request 1 and `touch`.
     This is an *upper bound* (it assumes the agent jumps straight to the file when told, and that
     every pre-touch request was spent looking for it).

Gold files are the task's `src_files` (from the upstream fix commit): an evaluation label only.
The trace prior is learned (edit counts in the repository's training sessions), nothing is
hand-written. Hand-written parts: the code-file extension list below (instrument).

    nice -n 10 python3 localise.py  ->  localise.json
"""

from __future__ import annotations

import ast
import json
import math
import random
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[1]))
sys.path.insert(0, str(HERE))
from traces import RUN  # noqa: E402
from cost_anatomy import parse, price_of, req_cost, counterfactual, boot  # noqa: E402

BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
TASKS = HERE.parents[2] / "repobench" / "tasks"
SPLITS = HERE.parents[2] / "repobench" / "splits"
CODE = re.compile(r"\.(py|go|ts|tsx|js|mjs|vue)$")
MODELS = Path("/tmp/adage-lab-models")
KS = (1, 3, 5, 10)


def tokens(s: str):
    s = re.sub(r"([a-z])([A-Z])", r"\1 \2", s)
    return [t for t in re.split(r"[^A-Za-z0-9]+", s.lower()) if len(t) > 1]


class BM25:
    def __init__(self, docs, k1=1.2, b=0.75):
        self.tf = [Counter(d) for d in docs]
        self.len = np.array([len(d) for d in docs], float)
        self.avg = self.len.mean() if len(docs) else 1
        df = Counter(t for d in self.tf for t in d)
        n = len(docs)
        self.idf = {t: math.log(1 + (n - c + 0.5) / (c + 0.5)) for t, c in df.items()}
        self.k1, self.b = k1, b

    def score(self, q):
        out = np.zeros(len(self.tf))
        for t in set(q):
            if t not in self.idf:
                continue
            for i, tf in enumerate(self.tf):
                f = tf.get(t)
                if f:
                    out[i] += self.idf[t] * f * (self.k1 + 1) / (f + self.k1 * (1 - self.b + self.b * self.len[i] / self.avg))
        return out


class Static:
    def __init__(self, emb: Path, tok: Path):
        from safetensors.numpy import load_file
        from tokenizers import Tokenizer
        t = load_file(str(emb))
        self.E = t.get("embeddings", next(iter(t.values()))).astype(np.float32)
        self.tok = Tokenizer.from_file(str(tok))

    def encode(self, texts):
        self.cache = getattr(self, "cache", {})
        miss = [t for t in dict.fromkeys(texts) if t not in self.cache]
        if miss:
            for t, v in zip(miss, self._encode(miss)):
                self.cache[t] = v
        return np.stack([self.cache[t] for t in texts])

    def _encode(self, texts):
        out = np.zeros((len(texts), self.E.shape[1]), np.float32)
        for i, enc in enumerate(self.tok.encode_batch(list(texts), add_special_tokens=False)):
            ids = [j for j in enc.ids if j < len(self.E)]
            if ids:
                out[i] = self.E[ids].mean(0)
        return out / np.maximum(np.linalg.norm(out, axis=1, keepdims=True), 1e-9)


def models():
    out = {}
    pc = MODELS / "potion-code-16m-v2/package/assets"
    if pc.is_dir():
        out["potion-code-16M-v2"] = Static(pc / "model.safetensors", pc / "tokenizer/tokenizer.json")
    pb = MODELS / "yarflam-potion-base-8m-1.0.4/package/models"
    if pb.is_dir():
        out["potion-base-8M"] = Static(pb / "model.safetensors", pb / "tokenizer.json")
    return out


def chunks(text: str, size=60):
    lines = text.splitlines()
    return ["\n".join(lines[i:i + size]) for i in range(0, max(len(lines), 1), size)]


def git(repo: Path, *args):
    return subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True).stdout


def lst(s):
    try:
        return list(ast.literal_eval(s)) if isinstance(s, str) else list(s)
    except Exception:
        return []


def task_runs(repo):
    """task id -> (checkout, [held-out control transcripts]) and training transcripts."""
    ck, ctl, train = {}, {}, []
    seen = set()
    for base in BASES:
        rdir = base / repo
        for d in sorted((rdir / "runs").glob("*")) if (rdir / "runs").is_dir() else []:
            m = RUN.match(d.name)
            if m and (d / "repo").is_dir():
                ck.setdefault(f"{repo.split('-sonnet')[0]}-{m.group(3)}", d / "repo")
        for f in sorted((rdir / "transcripts").glob("*/*.jsonl")) if (rdir / "transcripts").is_dir() else []:
            if f.stem in seen:
                continue
            rid = re.sub(r"-repo$", "", re.sub(r"^.*-runs-(?=(train|eval)-)", "", f.parent.name))
            m = RUN.match(rid)
            if not m:
                continue
            seen.add(f.stem)
            tid = f"{repo.split('-sonnet')[0]}-{m.group(3)}"
            if m.group(1) == "train":
                train.append(f)
            elif m.group(5) == "control":
                ctl.setdefault(tid, []).append(f)
    return ck, ctl, train


def touched_files(path: Path, root_hint: str):
    """Repository-relative files named by each request's tool calls, in request order."""
    rows = [json.loads(l) for l in path.open() if l.strip()]
    reqs, order = {}, []
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        mid = m.get("id")
        if mid not in reqs:
            reqs[mid] = []
            order.append(mid)
        for x in m.get("content") or []:
            if isinstance(x, dict) and x.get("type") == "tool_use":
                reqs[mid].append((x.get("name"), json.dumps(x.get("input"))))
    return [reqs[i] for i in order]


def first_touch(req_calls, gold):
    for j, calls in enumerate(req_calls):
        for name, inp in calls:
            if any(g in inp for g in gold):
                return j
    return len(req_calls)


def edited_in(req_calls):
    out = set()
    for calls in req_calls:
        for name, inp in calls:
            if name in ("Edit", "Write", "MultiEdit"):
                try:
                    p = json.loads(inp).get("file_path", "")
                except Exception:
                    p = ""
                m = re.search(r"/repo/(.+)$", p)
                if m:
                    out.add(m.group(1))
    return out


def main():
    mods = models()
    rows, hits = [], []
    for repo in ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]:
        base_repo = repo.split("-sonnet")[0]
        tj = json.loads((TASKS / f"{base_repo}.json").read_text())["tasks"]
        tasks = {t["id"]: t for t in tj}
        ck, ctl, train = task_runs(repo)
        if not ctl:
            continue
        # learned prior: files edited in this repository's training sessions (v1 training traces)
        prior = Counter()
        _, _, train_all = task_runs(base_repo)
        for f in train_all:
            for p in edited_in(touched_files(f, "")):
                prior[p] += 1
        for tid, files in sorted(ctl.items()):
            t = tasks.get(tid)
            if not t or tid not in ck:
                continue
            gold = [g for g in lst(t["src_files"]) if CODE.search(g)]
            if not gold:
                continue
            repo_dir = ck[tid]
            names = [p for p in git(repo_dir, "ls-files").split("\n") if CODE.search(p)]
            head = git(repo_dir, "rev-parse", "HEAD").strip()
            # the checkout's HEAD is the task's starting commit; the agent's edits are uncommitted
            docs = {}
            for p in names:
                if "/node_modules/" in p or p.startswith("node_modules"):
                    continue
                txt = git(repo_dir, "show", f"{head}:{p}")
                docs[p] = txt
            paths = list(docs)
            q = t["prompt"]
            scores = {}
            bm = BM25([tokens(p) * 3 + tokens(docs[p]) for p in paths])
            scores["bm25"] = bm.score(tokens(q))
            for name, m in mods.items():
                qv = m.encode([q])[0]
                s = np.zeros(len(paths))
                for i, p in enumerate(paths):
                    cs = chunks(docs[p])
                    v = m.encode([p + "\n" + c for c in cs])
                    s[i] = float((v @ qv).max())
                scores[name] = s
            scores["trace-prior"] = np.array([prior.get(p, 0) + 1e-6 * (1 / (1 + len(p))) for p in paths], float)

            def rrf(*keys):
                out = np.zeros(len(paths))
                for k in keys:
                    rank = np.argsort(-scores[k]).argsort()
                    out += 1 / (60 + rank)
                return out
            emb = "potion-code-16M-v2" if "potion-code-16M-v2" in scores else None
            scores["bm25+trace-prior"] = rrf("bm25", "trace-prior")
            if emb:
                scores["bm25+potion-code"] = rrf("bm25", emb)
                scores["bm25+potion-code+trace-prior"] = rrf("bm25", emb, "trace-prior")
            h = {"repo": repo, "task": tid, "n_files": len(paths)}
            for k, s in scores.items():
                top = [paths[i] for i in np.argsort(-s)[:max(KS)]]
                for kk in KS:
                    h[f"{k}@{kk}"] = float(any(g in top[:kk] for g in gold))
            hits.append(h)
            for f in files:
                reqs = parse(f)
                if not reqs:
                    continue
                p = price_of(reqs[0]["model"])
                total = sum(sum(req_cost(r["usage"], p).values()) for r in reqs)
                rc = touched_files(f, "")
                j = first_touch(rc, gold)
                # upper bound: the requests strictly between the first (reads the prompt) and the
                # touch would not have been needed
                share = counterfactual(reqs, p, None, drop_req=lambda i, r: 1 <= i < j) / total
                rows.append({"repo": repo, "task": tid, "model": reqs[0]["model"], "requests": len(reqs),
                             "touch": j, "pre_touch_share": share, **{k: v for k, v in h.items() if "@" in k}})
        print(repo, len(hits), len(rows), flush=True)

    methods = sorted({k.rsplit("@", 1)[0] for k in hits[0] if "@" in k})
    out = {"n_tasks": len(hits), "n_sessions": len(rows), "hit_at_k": {}, "translation": {}}
    for m in methods:
        out["hit_at_k"][m] = {kk: sum(h[f"{m}@{kk}"] for h in hits) / len(hits) for kk in KS}
    for grp, sel in {"haiku": [r for r in rows if "haiku" in r["model"]],
                     "sonnet": [r for r in rows if "sonnet" in r["model"]]}.items():
        if not sel:
            continue
        o = {"n": len(sel), "touch_request": {"mean": float(np.mean([r["touch"] for r in sel])),
                                               "median": float(np.median([r["touch"] for r in sel]))},
             "requests": float(np.mean([r["requests"] for r in sel])),
             "ceiling_pre_touch_share": {"mean": float(np.mean([r["pre_touch_share"] for r in sel])),
                                         "ci90": boot([r["pre_touch_share"] for r in sel])}}
        for m in methods:
            for kk in (1, 3, 5):
                v = [r[f"{m}@{kk}"] * r["pre_touch_share"] for r in sel]
                o[f"{m}@{kk}"] = {"mean": float(np.mean(v)), "ci90": boot(v)}
        out["translation"][grp] = o
    out["per_repo_hit@3"] = {r: {m: float(np.mean([h[f"{m}@3"] for h in hits if h["repo"] == r])) for m in methods}
                             for r in sorted({h["repo"] for h in hits})}
    (HERE / "localise.json").write_text(json.dumps(out, indent=1))
    print(json.dumps({"hit_at_k": out["hit_at_k"], "translation": {g: {k: v for k, v in o.items() if not k.startswith(("bm25+potion-code+", "potion-base"))} for g, o in out["translation"].items()}}, indent=1))


if __name__ == "__main__":
    main()
