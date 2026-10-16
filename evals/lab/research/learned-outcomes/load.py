"""Load every tool call of every RepoBench session, with its full output.

`evals/lab/traces.py` keeps only an error line for failed calls; this study needs the output text
of every call. One record per tool call, cached as gzipped JSONL in
`/tmp/adage-research/learned-outcomes/calls.jsonl.gz`.

Sessions are de-duplicated by session id (v2 re-used v1's training traces). When a run's state
names its session id, only that transcript is kept (a rerun leaves the discarded one beside it),
as `evals/replay/manifest.py` does.
"""

from __future__ import annotations

import gzip
import json
import re
import sys
from pathlib import Path

CACHE = Path("/tmp/adage-research/learned-outcomes")
OUT = CACHE / "calls.jsonl.gz"
BASES = [("v2", Path("/tmp/adage-rb2/runs")), ("v1", Path("/tmp/adage-repobench/runs"))]
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
RUN = re.compile(r"^(train|eval)-(.+)-([0-9a-f]{8})-r(\d+)(?:-(.+))?$")
ROOT = re.compile(r"/tmp/adage-[\w.-]+/runs/[\w.-]+/(?:private/)?(?:runs/[\w.-]+/)?(?:repo|bind-sandbox)/?")
HEAD, TAIL = 3000, 1500


def strip(s: str) -> str:
    return ROOT.sub("", s)


def clip(s: str) -> str:
    return s if len(s) <= HEAD + TAIL else s[:HEAD] + "\n…\n" + s[-TAIL:]


def result_text(cc) -> str:
    if isinstance(cc, str):
        return cc
    return "\n".join(y.get("text", "") for y in (cc or []) if isinstance(y, dict) and y.get("type") == "text")


def parse(path: Path):
    rows = [json.loads(l) for l in path.open() if l.strip()]
    results = {}
    for r in rows:
        if r.get("type") != "user":
            continue
        c = r.get("message", {}).get("content")
        if isinstance(c, list):
            for x in c:
                if isinstance(x, dict) and x.get("type") == "tool_result":
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), result_text(x.get("content")))
    calls, batch, last = [], -1, None
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        c = r.get("message", {}).get("content") or []
        tu = [x for x in c if isinstance(x, dict) and x.get("type") == "tool_use"]
        if not tu:
            continue
        mid = r.get("message", {}).get("id")
        if mid != last:
            batch += 1
        last = mid
        for u in tu:
            err, text = results.get(u.get("id"), (False, ""))
            inp = u.get("input") or {}
            name = u.get("name", "")
            if name == "Bash":
                arg = inp.get("command", "")
            elif name in ("Grep", "Glob"):
                arg = f"{inp.get('pattern', '')} {inp.get('path', '') or ''} {inp.get('glob', '') or ''}".strip()
            else:
                arg = inp.get("file_path") or inp.get("notebook_path") or inp.get("description") or ""
            calls.append({"tool": name, "arg": strip(str(arg))[:2000], "is_error": err,
                          "output": clip(strip(text)), "out_len": len(text), "batch": batch})
    return calls


def build():
    seen, n = set(), 0
    CACHE.mkdir(parents=True, exist_ok=True)
    with gzip.open(OUT, "wt") as f:
        for tag, base in BASES:
            for repo in REPOS:
                root = base / repo
                try:
                    state = json.loads((root / "state.json").read_text()).get("runs", {})
                except Exception:
                    continue
                for p in sorted((root / "private" / "transcripts").glob("*/*.jsonl")):
                    sid = p.stem
                    if sid in seen:
                        continue
                    d = re.sub(r"^.*-runs-(?=(train|eval)-)", "", p.parent.name)
                    rid = re.sub(r"-repo$", "", d)
                    m = RUN.match(rid)
                    if not m:
                        continue
                    phase, _, h, rnd, arm = m.groups()
                    st = state.get(rid, {})
                    met = st.get("metrics") or {}
                    if met.get("session_id") and met["session_id"] != sid:
                        continue
                    passed = met.get("hidden_passed", met.get("passed"))
                    if passed is None and st.get("status") in ("passed", "failed"):
                        passed = st["status"] == "passed"
                    calls = parse(p)
                    if not calls:
                        continue
                    seen.add(sid)
                    base_repo = repo.replace("-sonnet5", "")
                    sess = {"sid": sid, "repo": repo, "base_repo": base_repo, "bench": tag, "run": rid,
                            "phase": phase, "arm": arm or "train", "task": st.get("task") or f"{base_repo}-{h}",
                            "passed": passed, "cost_usd": met.get("cost_usd", met.get("total_cost_usd")),
                            "model": "sonnet5" if "sonnet5" in repo else "haiku", "calls": calls}
                    f.write(json.dumps(sess) + "\n")
                    n += 1
    print(f"{n} sessions -> {OUT}", file=sys.stderr)


def sessions():
    with gzip.open(OUT, "rt") as f:
        for l in f:
            yield json.loads(l)


if __name__ == "__main__":
    build()
