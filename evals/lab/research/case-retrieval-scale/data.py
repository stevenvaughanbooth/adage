"""Load every de-duplicated RepoBench session (v1 + v2, all arms, all 7 repo slices) with what the
case bank needs: normalised actions (lab `normalize.py`), the harness's failure flag, each call's
output text (for the output-template embedding), and a per-call marginal dollar cost under the
cumulative-context model.

Marginal $ of call j (what removing it would save):
  (a) its share of the API turn that issued it: turn_cost / |batch|, where turn_cost is the
      assistant message's usage priced with `crates/adage-core/src/pricing.toml` rates;
  (b) its request+result tokens (chars / 4) carried in context by every later API turn, at the
      cache-read rate, plus one cache write.
Cached to /tmp/adage-research/case-retrieval-scale/sessions.pkl.
"""

from __future__ import annotations

import json
import pickle
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
LAB = HERE.parent.parent
sys.path.insert(0, str(LAB))

from normalize import Action, normalize_tool, strip_root  # noqa: E402
from traces import RUN  # noqa: E402

BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
CACHE = Path("/tmp/adage-research/case-retrieval-scale")
RATES = {  # $/MTok: input, output, cache_write (5m), cache_read  (pricing.toml)
    "claude-haiku-4-5": (1.0, 5.0, 1.25, 0.10),
    "claude-sonnet-5": (2.0, 10.0, 2.5, 0.20),
}


def rates(model: str):
    for p, r in RATES.items():
        if model.startswith(p):
            return r
    return RATES["claude-haiku-4-5"]


@dataclass
class Step:
    action: Action
    failed: bool
    out: str  # first 600 chars of the tool result
    batch: int
    usd: float = 0.0  # marginal $ if this call were removed


@dataclass
class Session:
    sid: str
    repo: str  # slice (click-sonnet5 ...)
    base: str  # repository (click)
    task: str  # base + task hash: the same task across slices is one task
    arm: str
    version: str  # v1 | v2
    model: str
    passed: bool
    prompt: str
    steps: list = field(default_factory=list)
    usd_total: float = 0.0
    api_turns: int = 0


def _text(cc) -> str:
    if isinstance(cc, str):
        return cc
    return " ".join(y.get("text", "") for y in (cc or []) if isinstance(y, dict))


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
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), _text(x.get("content")))
    msgs, order = {}, []  # message id -> (usage, model, tool uses)
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        mid = m.get("id") or r.get("uuid")
        if mid not in msgs:
            msgs[mid] = {"usage": m.get("usage") or {}, "model": m.get("model") or "", "tu": []}
            order.append(mid)
        else:
            msgs[mid]["usage"] = m.get("usage") or msgs[mid]["usage"]
        for x in m.get("content") or []:
            if isinstance(x, dict) and x.get("type") == "tool_use":
                msgs[mid]["tu"].append(x)
    model = next((msgs[m]["model"] for m in order if msgs[m]["model"].startswith("claude")), "claude-haiku-4-5")
    ri, ro, rw, rr = rates(model)

    def cost(u):
        return (u.get("input_tokens", 0) * ri + u.get("output_tokens", 0) * ro
                + u.get("cache_creation_input_tokens", 0) * rw + u.get("cache_read_input_tokens", 0) * rr) / 1e6

    steps, batch, total = [], -1, 0.0
    turn_idx = []  # API turn index of each step
    for ti, mid in enumerate(order):
        m = msgs[mid]
        c = cost(m["usage"])
        total += c
        if not m["tu"]:
            continue
        batch += 1
        for u in m["tu"]:
            failed, text = results.get(u.get("id"), (False, ""))
            a = normalize_tool(u.get("name", ""), u.get("input") or {})
            s = Step(a, failed, strip_root(text[:600]), batch)
            s.usd = c / len(m["tu"])
            ctx_tok = (len(json.dumps(u.get("input") or {})) + len(text)) / 4
            s._ctx = ctx_tok  # type: ignore[attr-defined]
            steps.append(s)
            turn_idx.append(ti)
    n_turns = len(order)
    for s, ti in zip(steps, turn_idx):
        later = n_turns - ti - 1
        s.usd += s._ctx * (rw + rr * later) / 1e6  # type: ignore[attr-defined]
        del s._ctx  # type: ignore[attr-defined]
    # (a) and (b) overlap (the later turns' cache reads are already inside their turn cost), so
    # the raw marginals sum to ~1.3x the session's cost; rescale so they sum to exactly its cost
    # (conservative: removing every call saves at most everything).
    msum = sum(s.usd for s in steps)
    if msum > 0:
        for s in steps:
            s.usd *= total / msum
    return prompt, steps, model, total, n_turns


def _state(base: Path, repo: str) -> dict:
    try:
        return json.loads((base / repo / "state.json").read_text()).get("runs", {})
    except Exception:
        return {}


def load(refresh: bool = False) -> list[Session]:
    CACHE.mkdir(parents=True, exist_ok=True)
    pk = CACHE / "sessions.pkl"
    if pk.exists() and not refresh:
        return pickle.loads(pk.read_bytes())
    out, seen = [], set()
    for base in BASES:
        ver = "v1" if "repobench" in str(base) else "v2"
        for repo in REPOS:
            state = _state(base, repo)
            for f in sorted((base / repo / "transcripts").glob("*/*.jsonl")):
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
                prompt, steps, model, total, nt = parse(f)
                if not steps:
                    continue
                seen.add(sid)
                b = repo.replace("-sonnet5", "")
                out.append(Session(sid, repo, b, f"{b}-{h}", arm or "train", ver, model, bool(passed), prompt,
                                   steps, total, nt))
    pk.write_bytes(pickle.dumps(out))
    return out


if __name__ == "__main__":
    import collections
    ss = load(refresh=True)
    print(len(ss), "sessions")
    for r in REPOS:
        x = [s for s in ss if s.repo == r]
        print(r, len(x), dict(collections.Counter(s.arm for s in x)), "tasks", len({s.task for s in x}),
              "calls/s %.1f" % (sum(len(s.steps) for s in x) / len(x)),
              "$/s %.3f" % (sum(s.usd_total for s in x) / len(x)),
              "marg$ sum/total %.2f" % (sum(sum(t.usd for t in s.steps) for s in x) / sum(s.usd_total for s in x)))
