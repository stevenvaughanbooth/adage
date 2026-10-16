"""Load RepoBench sessions with what the file-prefetch question needs: every tool call's file
paths, result size, the assistant message (API request) it belongs to and that request's usage.

A *request* is one assistant message id: under the cumulative-context cost model its cost is its
own usage (input + cache write + cache read + output). Each tool call belongs to one request.

Cached as a pickle in /tmp/adage-research/context-prefetch/sessions.pkl.
"""

from __future__ import annotations

import json
import pickle
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

LAB = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(LAB))
from normalize import ROOT, normalize_tool  # noqa: E402

CACHE = Path("/tmp/adage-research/context-prefetch")
BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
RUNDIRS = {  # run directory -> (repo, model)
    "attrs": ("attrs", "haiku"), "click": ("click", "haiku"), "marshmallow": ("marshmallow", "haiku"),
    "urfave-cli": ("urfave-cli", "haiku"), "vue-core": ("vue-core", "haiku"),
    "click-sonnet5": ("click", "sonnet"), "vue-core-sonnet5": ("vue-core", "sonnet"),
}
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
RUN = re.compile(r"^(train|eval)-(.+)-([0-9a-f]{8})-r(\d+)(?:-(.+))?$")
# USD per million tokens (Anthropic list prices; 1h cache writes, which these runs used).
PRICE = {"haiku": dict(inp=1.0, out=5.0, cw=2.0, cr=0.10), "sonnet": dict(inp=3.0, out=15.0, cw=6.0, cr=0.30)}


@dataclass
class Call:
    tool: str
    inp: dict
    req: int  # index of the request (assistant message) that issued it
    failed: bool
    result_chars: int
    result: str  # truncated result text (for Grep/Glob file lists)
    key: str
    explore: bool
    paths: list = field(default_factory=list)  # repository-relative paths named by the call


@dataclass
class Request:
    inp: int
    cw: int
    cr: int
    out: int


@dataclass
class Session:
    sid: str
    repo: str
    model: str
    run: str
    phase: str
    arm: str
    task: str
    passed: object
    prompt: str
    calls: list
    reqs: list

    def cost(self, i: int) -> float:
        p = PRICE[self.model]
        r = self.reqs[i]
        return (r.inp * p["inp"] + r.out * p["out"] + r.cw * p["cw"] + r.cr * p["cr"]) / 1e6

    @property
    def dollars(self) -> float:
        """Billed-equivalent session $, from the common harness's validated cost model."""
        return hcost(self.sid).total


def rel(p: str) -> str:
    p = ROOT.sub(".", p or "")
    p = re.sub(r"^\./", "", p)
    return p


def parse(path: Path):
    rows = [json.loads(l) for l in path.open() if l.strip()]
    prompt, results = "", {}
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
                    cc = x.get("content")
                    text = cc if isinstance(cc, str) else " ".join(
                        y.get("text", "") for y in (cc or []) if isinstance(y, dict))
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), text)
    calls, reqs, mids = [], [], {}
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        mid = m.get("id")
        if mid not in mids:
            u = m.get("usage") or {}
            mids[mid] = len(reqs)
            reqs.append(Request(u.get("input_tokens", 0) or 0, u.get("cache_creation_input_tokens", 0) or 0,
                                u.get("cache_read_input_tokens", 0) or 0, u.get("output_tokens", 0) or 0))
        else:  # rows of one message repeat the usage; keep the largest output count
            u = m.get("usage") or {}
            q = reqs[mids[mid]]
            q.out = max(q.out, u.get("output_tokens", 0) or 0)
        for x in m.get("content") or []:
            if not (isinstance(x, dict) and x.get("type") == "tool_use"):
                continue
            name, inp = x.get("name", ""), x.get("input") or {}
            failed, text = results.get(x.get("id"), (False, ""))
            a = normalize_tool(name, inp)
            paths = []
            for k in ("file_path", "notebook_path", "path"):
                if inp.get(k):
                    paths.append(rel(inp[k]))
            calls.append(Call(name, inp, mids[mid], failed, len(text), rel(text[:4000]), a.key, a.explore, paths))
    return prompt, calls, reqs


_H: dict = {}


def hsession(sid: str):
    """The common harness's Session for this id (same transcript, same call order)."""
    if not _H:
        sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
        from harness import load as hload
        _H.update({s.sid: s for s in hload().sessions})
    return _H[sid]


def hcost(sid: str):
    h = hsession(sid)
    from harness import costmodel as CM
    return CM.of(h)


def harness_sids() -> set:
    """The sessions the common harness keeps (it drops 35 discarded/retried-run transcripts that a
    directory scan mislabels with the rerun's outcome)."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
    from harness import load as hload
    return {s.sid for s in hload().sessions}


def load(refresh: bool = False) -> list:
    f = CACHE / "sessions.pkl"
    if f.exists() and not refresh:
        return pickle.loads(f.read_bytes())
    out, seen = [], set()
    for base in BASES:
        for rd, (repo, model) in RUNDIRS.items():
            state = {}
            try:
                state = json.loads((base / rd / "state.json").read_text()).get("runs", {})
            except Exception:
                pass
            for tf in sorted((base / rd / "transcripts").glob("*/*.jsonl")):
                if tf.stem in seen:
                    continue
                rid = re.sub(r"^.*-runs-(?=(train|eval)-)", "", tf.parent.name)
                rid = re.sub(r"-repo$", "", rid)
                m = RUN.match(rid)
                if not m:
                    continue
                phase, _, h, rnd, arm = m.groups()
                st = state.get(rid, {})
                met = st.get("metrics") or {}
                passed = met.get("hidden_passed", met.get("passed"))
                if passed is None and st.get("status") in ("passed", "failed"):
                    passed = st["status"] == "passed"
                prompt, calls, reqs = parse(tf)
                if not calls:
                    continue
                seen.add(tf.stem)
                out.append(Session(tf.stem, repo, model, rid, phase, arm or "train", f"{repo}-{h}", passed,
                                   prompt, calls, reqs))
    keep = harness_sids()
    out = [s for s in out if s.sid in keep]
    CACHE.mkdir(parents=True, exist_ok=True)
    f.write_bytes(pickle.dumps(out))
    return out


if __name__ == "__main__":
    import collections
    S = load(refresh=True)
    c = collections.Counter((s.repo, s.model, s.arm) for s in S)
    for k, v in sorted(c.items()):
        print(k, v)
