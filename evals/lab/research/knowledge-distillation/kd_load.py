"""Load RepoBench sessions as raw calls: input text, output text, harness error flag, API cost.

Session discovery and de-duplication follow `evals/lab/traces.py` (v1 + v2 run directories, by
session id). Unlike `traces.py`, nothing is classified here: a call keeps its tool, its input as
text, its output as text and the harness's `is_error`. The run-specific sandbox root (the
transcript's own `cwd`) is stripped so paths are repository-relative across sessions.

Cost: every assistant API request's `usage` is priced with the session model's rates (Haiku 4.5:
input $1, output $5, 1h cache write $2, cache read $0.10 per MTok; Sonnet 5 at $3/$15/$6/$0.30);
a request that issued k tool calls charges each 1/k. This is the cumulative-context cost model:
the request that issued a call paid for the whole context read at that point.

Cached as a pickle in /tmp/adage-research/knowledge-distillation.
"""

from __future__ import annotations

import json
import os
import pickle
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

LAB = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(LAB))
import traces  # noqa: E402

CACHE = Path(os.environ.get("KD_CACHE", "/tmp/adage-research/knowledge-distillation"))
RATES = {"haiku": (1.0, 5.0, 2.0, 0.10), "sonnet": (3.0, 15.0, 6.0, 0.30)}
MAX_OUT = 12000  # characters of a tool result kept (atoms are extracted from these)


@dataclass
class Call:
    tool: str
    inp: str  # the call's arguments rendered as text, root stripped
    out: str  # the tool result as text, root stripped, truncated
    error: bool  # the harness's is_error
    req: int  # index of the API request that issued it
    cost: float  # $ share of that request
    out_chars: int


@dataclass
class Sess:
    sid: str
    repo: str
    run: str
    phase: str
    arm: str
    task: str
    passed: bool | None
    prompt: str
    model: str
    calls: list[Call] = field(default_factory=list)
    n_requests: int = 0
    cost: float = 0.0  # $ of all requests
    req_cost: list = field(default_factory=list)  # $ per API request
    req_new: list = field(default_factory=list)  # tokens newly written to context by each request


def price(u: dict, model: str) -> float:
    i, o, cw, cr = RATES["sonnet" if "sonnet" in model else "haiku"]
    return (u.get("input_tokens", 0) * i + u.get("output_tokens", 0) * o
            + u.get("cache_creation_input_tokens", 0) * cw + u.get("cache_read_input_tokens", 0) * cr) / 1e6


def _text(c) -> str:
    if isinstance(c, str):
        return c
    return "\n".join(y.get("text", "") for y in (c or []) if isinstance(y, dict))


def render_input(name: str, inp: dict) -> str:
    # The arguments as the model wrote them; no field is interpreted.
    parts = []
    for k, v in inp.items():
        if isinstance(v, (dict, list)):
            v = json.dumps(v)
        parts.append(str(v) if k in ("command", "file_path", "pattern") else f"{k}={v}")
    return f"{name}: " + " ".join(parts)


def strip(s: str, roots: list[str]) -> str:
    for r in roots:
        if r:
            s = s.replace(r + "/", "").replace(r, ".")
    return s


def parse(path: Path) -> tuple[str, str, list[Call], int, float]:
    rows = [json.loads(l) for l in path.open() if l.strip()]
    roots = sorted({r.get("cwd") for r in rows if r.get("cwd")}, key=len, reverse=True)
    model = ""
    for r in rows:
        if r.get("type") == "assistant":
            model = r.get("message", {}).get("model", "") or model
    results, prompt = {}, ""
    for r in rows:
        if r.get("type") != "user":
            continue
        c = r.get("message", {}).get("content")
        if isinstance(c, str) and not prompt:
            prompt = c
        elif isinstance(c, list):
            for x in c:
                if isinstance(x, dict) and x.get("type") == "text" and not prompt:
                    prompt = x.get("text", "")
                if isinstance(x, dict) and x.get("type") == "tool_result":
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), _text(x.get("content")))
    reqs: dict[str, dict] = {}
    order: list[str] = []
    uses: dict[str, list] = {}
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        mid = m.get("id") or r.get("uuid")
        if mid not in reqs:
            reqs[mid] = m.get("usage") or {}
            order.append(mid)
            uses[mid] = []
        for x in m.get("content") or []:
            if isinstance(x, dict) and x.get("type") == "tool_use":
                uses[mid].append(x)
    calls, total, rc, rn = [], 0.0, [], []
    for ri, mid in enumerate(order):
        c = price(reqs[mid], model)
        u = reqs[mid]
        rc.append(c)
        rn.append(u.get("cache_creation_input_tokens", 0) + u.get("input_tokens", 0))
        total += c
        us = uses[mid]
        for u in us:
            err, text = results.get(u.get("id"), (False, ""))
            text = strip(text, roots)
            calls.append(Call(u.get("name", ""), strip(render_input(u.get("name", ""), u.get("input") or {}), roots)[:4000],
                              text[:MAX_OUT], err, ri, c / len(us), len(text)))
    return strip(prompt, roots), model, calls, len(order), total, rc, rn


def excluded() -> set[str]:
    """Session ids the common harness excludes: transcripts of discarded or retried runs, which
    the old lab loader keeps (and mislabels with the rerun's outcome)."""
    sys.path.insert(0, str(LAB / "research"))
    from harness import load as hload
    return {Path(x["path"]).stem for x in hload().excluded}


def load(refresh: bool = False) -> list[Sess]:
    p = CACHE / "sessions.pkl"
    if p.exists() and not refresh:
        ex = excluded()
        return [s for s in pickle.loads(p.read_bytes()) if s.sid not in ex]
    CACHE.mkdir(parents=True, exist_ok=True)
    out, seen = [], set()
    for base in traces.BASES:
        for repo in traces.REPOS:
            state = traces._state(base, repo)
            tdir = base / repo / "transcripts"
            if not tdir.is_dir():
                continue
            for f in sorted(tdir.glob("*/*.jsonl")):
                sid = f.stem
                if sid in seen:
                    continue
                d = f.parent.name
                rid = re.sub(r"^.*-runs-(?=(train|eval)-)", "", d)
                rid = re.sub(r"-repo$", "", rid)
                m = traces.RUN.match(rid)
                if not m:
                    continue
                phase, _, h, rnd, arm = m.groups()
                st = state.get(rid, {})
                met = st.get("metrics") or {}
                passed = met.get("hidden_passed", met.get("passed"))
                prompt, model, calls, nreq, cost, rc, rn = parse(f)
                if not calls:
                    continue
                seen.add(sid)
                s = Sess(sid, repo, rid, phase, arm or "train", f"{repo}-{h}", passed, prompt, model, calls, nreq, cost, rc, rn)
                s.version = "v1" if "adage-repobench" in str(base) else "v2"
                out.append(s)
    p.write_bytes(pickle.dumps(out))
    ex = excluded()
    return [s for s in out if s.sid not in ex]


if __name__ == "__main__":
    import kd_load
    ss = kd_load.load(refresh=True)
    from collections import Counter
    print(len(ss), Counter((s.repo, s.arm) for s in ss))
    print(Counter(s.model for s in ss))
