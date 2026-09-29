"""Load RepoBench sessions with the full content of every tool call (input and output), for data-flow
analysis, plus the per-API-message token usage needed by the cumulative-context cost model.

Sources: RepoBench v1 (/tmp/adage-repobench/runs) and v2 (/tmp/adage-rb2/runs). A run's transcript is
picked by the session id its `state.json` recorded (a rerun leaves the discarded file beside it);
sessions are de-duplicated by id (v2 re-used v1's training traces). Nothing here is a learned or a
hand-written feature: it is the trace's own schema (tool name, input, result text, `is_error`) with the
run-specific sandbox root stripped so paths are repository-relative.

Cached as a pickle in /tmp/adage-research/hindsight-shortcuts/sessions.pkl.
"""

from __future__ import annotations

import json
import pickle
import re
from dataclasses import dataclass, field
from pathlib import Path

CACHE = Path("/tmp/adage-research/hindsight-shortcuts")
BASES = [("v1", Path("/tmp/adage-repobench/runs")), ("v2", Path("/tmp/adage-rb2/runs"))]
ROOTS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
ROOT_RE = re.compile(r"/tmp/adage-[\w.-]+/runs/[\w.-]+/(?:private/)?(?:runs/[\w.-]+/)?(?:repo|bind-sandbox/repo|bind-sandbox)/?")
OUT_CAP = 12000  # characters of a tool result kept for data flow (the size is kept separately)

# Anthropic list prices, $/Mtok (crates/adage-core/src/pricing.toml, as_of 2026-06-24).
PRICES = {"claude-haiku-4-5": (1.0, 5.0, 1.25, 0.10), "claude-sonnet-5": (3.0, 15.0, 3.75, 0.30)}


RUN_RE = re.compile(r"/tmp/adage-[\w.-]+/runs/[\w.-]+/runs/[\w.-]+/")  # the run's own dir (home, tmp)


def strip_root(s: str) -> str:
    return RUN_RE.sub("", ROOT_RE.sub("", s))


@dataclass
class Call:
    tool: str
    inp: dict
    inp_text: str  # the input rendered as text, root stripped
    out: str  # result text (capped), root stripped
    out_chars: int
    is_error: bool
    batch: int  # index of the assistant message (API call) that issued it
    gen_tokens: float  # output tokens of the issuing API call, split over its tool calls


@dataclass
class Session:
    sid: str
    root: str  # e.g. v2/click
    repo: str  # base repository (click-sonnet5 -> click)
    model: str
    run: str
    phase: str
    arm: str
    task: str
    passed: bool | None
    cost_usd: float
    changed_files: list
    prompt: str
    calls: list = field(default_factory=list)
    n_api: int = 0  # API calls (assistant messages)
    api_of_call: list = field(default_factory=list)  # API-call index at which each call's result is first read
    usage: list = field(default_factory=list)  # per API call: (input, cache_write, cache_read, output) tokens


def render_input(tool: str, inp: dict) -> str:
    if tool == "Bash":
        return strip_root(inp.get("command", ""))
    parts = []
    for k, v in inp.items():
        if isinstance(v, (str, int, float)):
            parts.append(f"{k}={strip_root(str(v))}")
        elif isinstance(v, list):
            parts.append(f"{k}={strip_root(json.dumps(v))}")
    return f"{tool} " + " ".join(parts)


def parse(path: Path):
    rows = [json.loads(l) for l in path.open() if l.strip()]
    prompt, results, model = "", {}, ""
    for r in rows:
        if r.get("type") != "user" or r.get("isSidechain"):
            continue
        c = r.get("message", {}).get("content")
        if isinstance(c, str) and not prompt:
            prompt = c
        elif isinstance(c, list):
            for x in c:
                if not isinstance(x, dict):
                    continue
                if x.get("type") == "text" and not prompt:
                    prompt = x.get("text", "")
                if x.get("type") == "tool_result":
                    cc = x.get("content")
                    text = cc if isinstance(cc, str) else " ".join(
                        y.get("text", "") for y in (cc or []) if isinstance(y, dict))
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), text)
    msgs = []  # (message id, usage, [tool_use])
    idx = {}
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        model = model or m.get("model", "")
        mid = m.get("id")
        if mid not in idx:
            idx[mid] = len(msgs)
            msgs.append([mid, m.get("usage") or {}, []])
        for x in m.get("content") or []:
            if isinstance(x, dict) and x.get("type") == "tool_use":
                msgs[idx[mid]][2].append(x)
    calls, api_of_call = [], []
    for b, (_, usage, tus) in enumerate(msgs):
        gen = float(usage.get("output_tokens", 0) or 0)
        for u in tus:
            name = u.get("name", "")
            inp = u.get("input") or {}
            err, text = results.get(u.get("id"), (False, ""))
            text = strip_root(text)
            calls.append(Call(name, inp, render_input(name, inp), text[:OUT_CAP], len(text), err, b,
                              gen / max(len(tus), 1)))
            api_of_call.append(b + 1)
    usage = [(float(u.get('input_tokens', 0) or 0), float(u.get('cache_creation_input_tokens', 0) or 0),
              float(u.get('cache_read_input_tokens', 0) or 0), float(u.get('output_tokens', 0) or 0))
             for _, u, _ in msgs]
    return prompt, calls, len(msgs), api_of_call, model, usage


def _pick(tdir: Path, rid: str, sid: str | None) -> Path | None:
    dirs = [d for d in tdir.glob(f"*-{rid}-repo") if d.is_dir()]
    if not dirs:
        return None
    files = sorted(dirs[0].glob("*.jsonl"))
    for f in files:
        if sid and f.stem == sid:
            return f
    return files[0] if len(files) == 1 else None


def load(refresh: bool = False) -> list[Session]:
    CACHE.mkdir(parents=True, exist_ok=True)
    pk = CACHE / "sessions.pkl"
    if pk.exists() and not refresh:
        return pickle.loads(pk.read_bytes())
    out, seen = [], set()
    for ver, base in BASES:
        for root in ROOTS:
            d = base / root
            if not (d / "state.json").exists():
                continue
            runs = json.loads((d / "state.json").read_text()).get("runs", {})
            for rid, run in sorted(runs.items()):
                if run.get("status") not in ("passed", "failed"):
                    continue
                m = run.get("metrics") or {}
                sid = m.get("session_id")
                f = _pick(d / "transcripts", rid, sid)
                if f is None or f.stem in seen:
                    continue
                prompt, calls, n_api, aoc, model, usage = parse(f)
                if not calls:
                    continue
                seen.add(f.stem)
                sc = run.get("score") or {}
                passed = m.get("hidden_passed", m.get("passed"))
                if passed is None:
                    passed = run.get("status") == "passed"
                out.append(Session(f.stem, f"{ver}/{root}", root.replace("-sonnet5", ""), model, rid,
                                   run.get("phase"), run.get("arm") or "train", run.get("task"), passed,
                                   float(m.get("cost_usd") or 0.0), sc.get("changed_files") or [],
                                   strip_root(prompt), calls, n_api, aoc, usage))
    pk.write_bytes(pickle.dumps(out))
    return out


def price(model: str):
    for p, v in PRICES.items():
        if model.startswith(p):
            return v
    return PRICES["claude-haiku-4-5"]


if __name__ == "__main__":
    import collections
    import load as _self  # pickle the classes under the module name, not __main__
    S = _self.load(refresh=True)
    c = collections.Counter((s.root, s.phase, s.arm) for s in S)
    for k, v in sorted(c.items()):
        print(k, v)
    print(len(S), "sessions", sum(len(s.calls) for s in S), "calls")
