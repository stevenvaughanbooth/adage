"""Load every RepoBench v1 + v2 session with outcome, arm, model, bench and per-call detail.

Differs from `evals/lab/traces.py` in three ways: it includes the Sonnet 5 slices, it keeps each
call's result text (head + tail, for learned output clustering) and exit flag, and it keeps the
runner's metrics (hidden_passed, censoring, cost, changed files). Sessions are de-duplicated by
session id; the first base that holds a session (v1, then v2) labels its bench, except that a
session whose run appears in v2's state.json with a v2-only run id is v2.

Cached to /tmp/adage-research/success-uplift/sessions.pkl.
"""

from __future__ import annotations

import json
import pickle
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # evals/lab
from normalize import normalize_tool, strip_root  # noqa: E402

BASES = [("v1", Path("/tmp/adage-repobench/runs")), ("v2", Path("/tmp/adage-rb2/runs"))]
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
CACHE = Path("/tmp/adage-research/success-uplift")
RUN = re.compile(r"^(train|eval)-(.+)-([0-9a-f]{8})-r(\d+)(?:-(.+))?$")


def _text(cc) -> str:
    if isinstance(cc, str):
        return cc
    return " ".join(y.get("text", "") for y in (cc or []) if isinstance(y, dict))


def parse(path: Path) -> list[dict]:
    rows = [json.loads(l) for l in path.open() if l.strip()]
    res = {}
    for r in rows:
        if r.get("type") != "user":
            continue
        c = r.get("message", {}).get("content")
        if isinstance(c, list):
            for x in c:
                if isinstance(x, dict) and x.get("type") == "tool_result":
                    t = strip_root(_text(x.get("content")))
                    res[x.get("tool_use_id")] = (bool(x.get("is_error")), t)
    steps, batch, last = [], -1, None
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        tu = [x for x in (m.get("content") or []) if isinstance(x, dict) and x.get("type") == "tool_use"]
        if not tu:
            continue
        if m.get("id") != last:
            batch += 1
        last = m.get("id")
        for u in tu:
            err, out = res.get(u.get("id"), (False, ""))
            inp = u.get("input") or {}
            a = normalize_tool(u.get("name", ""), inp)
            raw = strip_root(inp.get("command", "")) if u.get("name") == "Bash" else ""
            path_ = strip_root(inp.get("file_path") or inp.get("path") or "")
            steps.append(dict(tool=a.tool, key=a.key, template=a.template, full=a.full, text=a.text,
                              raw=raw[:400], path=path_, err=err, out_head=out[:300], out_tail=out[-300:],
                              out_len=len(out), batch=batch))
    return steps


def load(refresh: bool = False) -> list[dict]:
    CACHE.mkdir(parents=True, exist_ok=True)
    pk = CACHE / "sessions.pkl"
    if pk.exists() and not refresh:
        return pickle.loads(pk.read_bytes())
    out, seen = [], set()
    for bench, base in BASES:
        for repo in REPOS:
            try:
                state = json.loads((base / repo / "state.json").read_text()).get("runs", {})
            except Exception:
                continue
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
                if passed is None or (met.get("session_id") and met["session_id"] != sid):
                    continue  # no outcome, or a discarded run superseded by a rerun (v2 click/attrs)
                steps = parse(f)
                if not steps:
                    continue
                seen.add(sid)
                base_repo = repo.replace("-sonnet5", "")
                out.append(dict(
                    sid=sid, repo=base_repo, slice=repo, model="sonnet5" if "sonnet5" in repo else "haiku45",
                    bench=bench, run=rid, phase=phase, arm=arm or "train", task=f"{base_repo}-{h}",
                    round=int(rnd), passed=bool(passed),
                    censored=(met.get("result_subtype") == "error_max_turns"),
                    turns=met.get("turns"), cost=met.get("cost_usd"), tool_calls=met.get("tool_calls"),
                    changed=(st.get("score") or {}).get("changed_files") or [],
                    touched_gold=(st.get("score") or {}).get("touched_gold"),
                    prompt=st.get("prompt", ""), steps=steps))
    pk.write_bytes(pickle.dumps(out))
    return out


if __name__ == "__main__":
    import collections
    S = load(refresh=True)
    print(len(S), "sessions")
    c = collections.Counter((s["bench"], s["slice"], s["arm"]) for s in S)
    for k in sorted(c):
        print(k, c[k])
