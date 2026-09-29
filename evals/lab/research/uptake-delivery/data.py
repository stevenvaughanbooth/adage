"""Load every live A/B session that logged Adage decisions, with its tool calls and its
delivered (`directives`) and withheld (`would_have`) directives aligned to the call sequence.

Sources (read-only):
  primary   RepoBench v1 (/tmp/adage-repobench/runs/<repo>), v2 (/tmp/adage-rb2/runs/<repo>) and the
            click slice (scratchpad/ld/slice/click-slice); arms `adage` (delivered) and `control`
            (hooks on, directives withheld, `would_have` logged at the same trigger).
  secondary other live slices on disk that logged decisions (click-a3slice, skill/obligation slices
            on click and a generated `acme` repo, delegation slices, the vue-core learning curve).
            They add hook types (UserPromptSubmit, Stop) and directive kinds the primary set lacks;
            used for the p_follow model only, never for the causal A/B numbers unless they have a
            clean control arm.

Each run's `state.json` (runner-written) gives arm, task, model, outcome, cost and the per-session
Adage log `metrics.adage` (`directives` / `would_have` / judgements with ms timestamps). Sessions are
de-duplicated by session id (v1 and v2 share training traces).
"""
from __future__ import annotations

import glob
import json
import os
import pickle
import re
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

SP = "/tmp/claude-0/-home-user-adage/d6ae021f-492b-5125-b4ff-8e8a0676a66b/scratchpad"
PRIMARY = sorted(glob.glob("/tmp/adage-rb2/runs/*")) + sorted(glob.glob("/tmp/adage-repobench/runs/*")) + [
    f"{SP}/ld/slice/click-slice"]
SECONDARY = ["/tmp/adage-a3slice/click-a3slice", f"{SP}/skl/acme", f"{SP}/skl/click", "/tmp/adage-deleg/acme",
             "/tmp/adage-deleg/gen", "/tmp/adage-repobench/curves/vue-core/adage",
             "/tmp/adage-repobench/curves/vue-core/control", "/tmp/adage-repobench/curves/click/adage",
             "/tmp/adage-repobench/curves/click/control"]
CACHE = Path("/tmp/adage-research/uptake-delivery")

# Haiku 4.5 / Sonnet list prices, $ per million tokens: input, output, cache write (5m), cache read.
PRICES = {"haiku": (1.0, 5.0, 1.25, 0.10), "sonnet": (3.0, 15.0, 3.75, 0.30)}


@dataclass
class Call:
    idx: int
    tid: str
    name: str
    inp: dict
    ts: float          # tool_use timestamp (s)
    rts: float         # tool_result timestamp (s)
    err: bool
    out: str           # result text (first 4000 chars)
    turn_usd: float    # $ of the assistant message that issued it (shared across a batch, split evenly)
    ctx_tokens: int    # prompt tokens at that assistant message (cache read + write + input)
    res_tokens: int    # rough tokens of call + result (chars/4), what it adds to later context
    mid: str = ""      # assistant message id (calls with the same id are one batch)


@dataclass
class Event:
    kind: str          # directives | would_have | stop_request | followed | ignored | obligation_met
    hook: str          # SessionStart | UserPromptSubmit | PostToolUse(Failure) | PostToolBatch | Stop
    ts: float
    route: str
    strength: str
    text: str
    pos: int = 0       # index of the first call issued after the event


@dataclass
class Sess:
    sid: str
    root: str
    pool: str          # primary | secondary
    repo: str
    model: str
    arm: str
    task: str
    run: str
    passed: bool | None
    cost: float
    cwd: str
    prompt: str
    calls: list = field(default_factory=list)
    events: list = field(default_factory=list)
    reflex_text: dict = field(default_factory=dict)  # route -> full delivered <adage-reflex> block


def _ts(s: str) -> float:
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def _text(c) -> str:
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        return "\n".join(x.get("text", "") if isinstance(x, dict) else str(x) for x in c)
    return ""


def parse_transcript(path: str, family: str):
    rows = []
    for l in open(path):
        l = l.strip()
        if l:
            try:
                rows.append(json.loads(l))
            except Exception:
                pass
    pr = PRICES[family]
    results, calls, reflex, prompt, cwd = {}, [], {}, "", ""
    for r in rows:
        cwd = cwd or r.get("cwd", "")
        if r.get("type") == "user":
            c = r.get("message", {}).get("content")
            if isinstance(c, str) and not prompt:
                prompt = c
            if isinstance(c, list):
                for x in c:
                    if isinstance(x, dict) and x.get("type") == "tool_result":
                        results[x.get("tool_use_id")] = (bool(x.get("is_error")), _text(x.get("content"))[:4000],
                                                         _ts(r["timestamp"]))
                    elif isinstance(x, dict) and x.get("type") == "text" and not prompt:
                        prompt = x.get("text", "")
        a = r.get("attachment") or {}
        if a.get("type") == "hook_additional_context":
            body = "\n".join(a.get("content") or [])
            for m in re.finditer(r'<adage-(?:reflex|signal)[^>]*route="([^"]+)"[^>]*>(.*?)</adage-(?:reflex|signal)>',
                                 body, re.S):
                reflex.setdefault(m.group(1), m.group(0))
    seen_mid = {}
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        msg = r.get("message", {})
        tus = [x for x in msg.get("content") or [] if isinstance(x, dict) and x.get("type") == "tool_use"]
        if not tus:
            continue
        u = msg.get("usage") or {}
        mid = msg.get("id")
        usd = (u.get("input_tokens", 0) * pr[0] + u.get("output_tokens", 0) * pr[1]
               + u.get("cache_creation_input_tokens", 0) * pr[2] + u.get("cache_read_input_tokens", 0) * pr[3]) / 1e6
        ctx = u.get("input_tokens", 0) + u.get("cache_creation_input_tokens", 0) + u.get("cache_read_input_tokens", 0)
        if mid in seen_mid:  # one row per content block; usage repeated -> charge once
            usd = 0.0
        seen_mid[mid] = True
        ts = _ts(r["timestamp"])
        for i, t in enumerate(tus):
            err, out, rts = results.get(t.get("id"), (False, "", ts))
            inp = t.get("input") or {}
            calls.append(Call(len(calls), t.get("id"), t.get("name", ""), inp, ts, rts, err, out,
                              usd / len(tus), ctx, (len(json.dumps(inp)) + len(out)) // 4, mid or ""))
    return prompt, cwd, calls, reflex


def _find_transcripts(root: str) -> dict:
    out = {}
    for f in glob.glob(f"{root}/private/transcripts/*/*.jsonl") + glob.glob(f"{root}/transcripts/*/*.jsonl"):
        out.setdefault(os.path.basename(f)[:-6], f)
    return out


def _repo_label(root: str) -> str:
    b = os.path.basename(root.rstrip("/"))
    if "/curves/" in root:
        return root.split("/curves/")[1].split("/")[0]
    if "/skl/" in root:
        return "skl-" + b
    if b in ("acme", "gen"):
        return "deleg-" + b
    return b


def load(refresh=False) -> list[Sess]:
    CACHE.mkdir(parents=True, exist_ok=True)
    cp = CACHE / "sessions.pkl"
    if cp.exists() and not refresh:
        return pickle.load(open(cp, "rb"))
    out, seen = [], set()
    for pool, roots in (("primary", PRIMARY), ("secondary", SECONDARY)):
        for root in roots:
            try:
                st = json.load(open(f"{root}/state.json"))
            except Exception:
                continue
            model = st.get("model") or ""
            fam = "sonnet" if "sonnet" in model else "haiku"
            tr = _find_transcripts(root)
            for rid, v in st.get("runs", {}).items():
                m = v.get("metrics") or {}
                sid = m.get("session_id")
                if not sid or sid in seen or sid not in tr:
                    continue
                if v.get("phase") != "eval":
                    continue
                seen.add(sid)
                prompt, cwd, calls, reflex = parse_transcript(tr[sid], fam)
                evs = []
                for x in m.get("adage") or []:
                    evs.append(Event(x["kind"], x.get("event", ""), x.get("ts", 0) / 1000.0, x.get("route", ""),
                                     x.get("strength", ""), x.get("text", "")))
                for e in evs:
                    e.pos = next((c.idx for c in calls if c.ts > e.ts), len(calls))
                passed = m.get("hidden_passed", m.get("passed"))
                out.append(Sess(sid, root, pool, _repo_label(root), fam, v.get("arm", ""), v.get("task", ""), rid,
                                passed, float(m.get("cost_usd") or 0.0), cwd, prompt, calls, evs, reflex))
    pickle.dump(out, open(cp, "wb"))
    return out


if __name__ == "__main__":
    import collections
    import data as _d
    S = _d.load(refresh=True)
    C = collections.Counter()
    for s in S:
        C[(s.pool, s.repo, s.model, s.arm)] += 1
        for e in s.events:
            C[(s.pool, s.arm, e.kind, e.hook)] += 1
    for k, v in sorted(C.items(), key=str):
        print(k, v)
