"""Load RepoBench transcripts as raw action sequences with per-call $ attribution.

Nothing here is semantic: a step is (tool name, its argument text with the sandbox root stripped,
the harness's own is_error flag, the characters it added to the context, and the API turn that
produced it). No tool taxonomy, no error-string lists. `strip_root` (lab/normalize.py) removes the
run-specific sandbox path so that paths are repository-relative; it is a measurement artefact of
the benchmark (a real user's repository has one fixed root), not a feature.

Cost model (cumulative-context, the brief's target metric): the session's true $ is the sum of its
API turns priced with crates/adage-core/src/pricing.toml. Removing call j saves
  (a) its share of the API turn that emitted it (turn $ / calls in that turn), and
  (b) the tokens it added (tool_use input + tool_result, chars/4) re-read from cache at every later
      API turn, plus one cache write.

Cache: /tmp/adage-research/sequence-models/sessions.pkl
"""

from __future__ import annotations

import json
import pickle
import re
import sys
from pathlib import Path

LAB = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(LAB))
from normalize import strip_root  # noqa: E402  (path normalisation only)

BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
SETS = {"attrs": "attrs", "click": "click", "marshmallow": "marshmallow", "urfave-cli": "urfave-cli",
        "vue-core": "vue-core", "click-sonnet5": "click", "vue-core-sonnet5": "vue-core"}
CACHE = Path("/tmp/adage-research/sequence-models")
RUN = re.compile(r"^(train|eval)-(.+)-([0-9a-f]{8})-r(\d+)(?:-(.+))?$")

PRICES = {  # $/M tokens: input, output, cache_write, cache_read (pricing.toml)
    "claude-haiku-4-5": (1.0, 5.0, 1.25, 0.10),
    "claude-sonnet-5": (2.0, 10.0, 2.5, 0.20),
    "claude-sonnet-4-6": (3.0, 15.0, 3.75, 0.30),
    "claude-sonnet-4-5": (3.0, 15.0, 3.75, 0.30),
    "claude-opus-4-5": (5.0, 25.0, 6.25, 0.50),
}


def rates(model: str):
    m = (model or "").lower()
    best = max((p for p in PRICES if m.startswith(p)), key=len, default=None)
    return PRICES[best] if best else PRICES["claude-haiku-4-5"]


def arg_text(name: str, inp: dict) -> str:
    """The call's own argument text: the command for Bash, the path for file tools, else the
    concatenated string values. No per-tool semantics beyond picking the text fields."""
    if "command" in inp:
        s = str(inp["command"])
    else:
        vals = [str(v) for k, v in inp.items() if isinstance(v, str) and k not in
                ("old_string", "new_string", "content", "description", "timeout")]
        s = " ".join(vals)
    s = strip_root(s)
    first, _, rest = s.partition("\n")
    return (first + (" <<more" if rest else ""))[:240]


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
    turns, steps, model = {}, [], ""
    order = []
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        msg = r.get("message", {})
        mid = msg.get("id")
        model = msg.get("model") or model
        if mid not in turns:
            turns[mid] = {"usage": {}, "calls": 0}
            order.append(mid)
        if msg.get("usage"):
            turns[mid]["usage"] = msg["usage"]
        for u in msg.get("content") or []:
            if isinstance(u, dict) and u.get("type") == "tool_use":
                failed, text = results.get(u.get("id"), (False, ""))
                inp = u.get("input") or {}
                steps.append({"tool": u.get("name", ""), "arg": arg_text(u.get("name", ""), inp),
                              "err": failed, "turn": order.index(mid),
                              "chars": len(json.dumps(inp)) + len(text),
                              "out": strip_root(text[:300])})
                turns[mid]["calls"] += 1
    ri, ro, rw, rr = rates(model)
    tcost = []
    for mid in order:
        u = turns[mid]["usage"]
        tcost.append((u.get("input_tokens", 0) * ri + u.get("output_tokens", 0) * ro +
                      u.get("cache_creation_input_tokens", 0) * rw + u.get("cache_read_input_tokens", 0) * rr) / 1e6)
    n_turns = len(order)
    for s in steps:
        t = s["turn"]
        share = tcost[t] / max(turns[order[t]]["calls"], 1)
        carry = s["chars"] / 4 * (rw + rr * max(n_turns - t - 1, 0)) / 1e6
        s["usd"] = share + carry
    # batch index = API turn among turns with tool calls
    tt = sorted({s["turn"] for s in steps})
    bmap = {t: i for i, t in enumerate(tt)}
    for s in steps:
        s["batch"] = bmap[s["turn"]]
    return prompt, steps, model, sum(tcost), n_turns


def load(refresh: bool = False):
    CACHE.mkdir(parents=True, exist_ok=True)
    pk = CACHE / "sessions.pkl"
    if pk.exists() and not refresh:
        return pickle.loads(pk.read_bytes())
    out, seen = [], set()
    for base in BASES:
        for setname, repo in SETS.items():
            d = base / setname
            try:
                state = json.loads((d / "state.json").read_text()).get("runs", {})
            except Exception:
                state = {}
            tdir = d / "transcripts"
            if not tdir.is_dir():
                continue
            for f in sorted(tdir.glob("*/*.jsonl")):
                sid = f.stem
                if sid in seen:
                    continue
                rid = re.sub(r"^.*-runs-(?=(train|eval)-)", "", f.parent.name)
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
                prompt, steps, model, usd, nt = parse(f)
                if not steps:
                    continue
                seen.add(sid)
                out.append({"sid": sid, "repo": repo, "set": setname, "phase": phase, "arm": arm or "train",
                            "task": f"{repo}-{h}", "passed": passed, "prompt": prompt[:1000], "model": model,
                            "steps": steps, "usd": usd, "api_turns": nt})
    pk.write_bytes(pickle.dumps(out))
    return out


if __name__ == "__main__":
    import collections
    S = load(refresh=True)
    c = collections.Counter((s["repo"], s["phase"], s["arm"], s["model"][:17]) for s in S)
    for k, v in sorted(c.items()):
        print(k, v)
    import numpy as np
    for arm in ("train", "control"):
        x = [s for s in S if s["arm"] == arm]
        print(arm, len(x), "calls/sess", np.mean([len(s["steps"]) for s in x]).round(1),
              "$/sess", np.mean([s["usd"] for s in x]).round(3),
              "attributed/true", round(sum(st["usd"] for s in x for st in s["steps"]) / sum(s["usd"] for s in x), 3))
