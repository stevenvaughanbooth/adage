"""Parse RepoBench Claude Code transcripts into request-level records with measured token usage.

One *request* is one assistant API message (rows sharing `message.id`), carrying its real `usage`
(input, cache write 5m/1h, cache read, output) and the tool calls it emitted. Each tool call keeps
its input, its full result text and `is_error`. The per-session cache is written as JSON lines to
`/tmp/adage-research/waste-anatomy/sessions.jsonl.gz` (held-out control + train + adage sessions,
de-duplicated by session id).

Also recovered per session: the run's outcome (hidden tests), the scorer's `changed_files` (the
files in the final diff), the reported `total_cost_usd`, the turn cap (30 in v1, 60 in v2), and the
original contents (`git show HEAD:<file>`, the task's base commit) of every file the agent edited,
so the final diff can be replayed from the Edit/Write calls.
"""

from __future__ import annotations

import gzip
import json
import re
import subprocess
import sys
from pathlib import Path

BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
OUT = Path("/tmp/adage-research/waste-anatomy")
RUN = re.compile(r"^(train|eval)-(.+)-([0-9a-f]{8})-r(\d+)(?:-(.+))?$")
ROOT = re.compile(r"/tmp/adage-[\w.-]+/runs/[\w.-]+/(?:private/)?(?:runs/[\w.-]+/)?(?:repo|bind-sandbox)/?")


def strip(s: str) -> str:
    return ROOT.sub("", s) if isinstance(s, str) else s


def strip_obj(o):
    if isinstance(o, str):
        return strip(o)
    if isinstance(o, dict):
        return {k: strip_obj(v) for k, v in o.items()}
    if isinstance(o, list):
        return [strip_obj(v) for v in o]
    return o


def parse(path: Path) -> dict:
    rows = [json.loads(l) for l in path.open() if l.strip()]
    results, prompt = {}, ""
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
                    text = cc if isinstance(cc, str) else "\n".join(
                        y.get("text", "") for y in (cc or []) if isinstance(y, dict))
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), strip(text))
    reqs, order = {}, []
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        m = r.get("message", {})
        mid = m.get("id")
        if mid not in reqs:
            reqs[mid] = {"mid": mid, "model": m.get("model"), "usage": {}, "text": "", "calls": []}
            order.append(mid)
        q = reqs[mid]
        u = m.get("usage") or {}
        if u:
            cc = u.get("cache_creation") or {}
            q["usage"] = {
                "in": u.get("input_tokens", 0),
                "cw": u.get("cache_creation_input_tokens", 0),
                "cw1h": cc.get("ephemeral_1h_input_tokens", 0),
                "cr": u.get("cache_read_input_tokens", 0),
                "out": u.get("output_tokens", 0),
            }
        for x in m.get("content") or []:
            if not isinstance(x, dict):
                continue
            if x.get("type") == "text":
                q["text"] += strip(x.get("text", "")) + "\n"
            elif x.get("type") == "tool_use":
                err, res = results.get(x.get("id"), (False, ""))
                q["calls"].append({"id": x.get("id"), "name": x.get("name", ""),
                                   "input": strip_obj(x.get("input") or {}), "err": err, "res": res})
    return {"prompt": prompt, "requests": [reqs[k] for k in order if reqs[k]["model"] != "<synthetic>"]}


def originals(repo_dir: Path, files: set[str]) -> dict:
    out = {}
    try:  # agents often `git commit` their work, so HEAD is not the task's base: use the root commit
        root = subprocess.run(["git", "-C", str(repo_dir), "rev-list", "--max-parents=0", "HEAD"],
                              capture_output=True, text=True, timeout=10).stdout.split()[0]
    except Exception:
        root = "HEAD"
    for f in files:
        try:
            p = subprocess.run(["git", "-C", str(repo_dir), "show", f"{root}:{f}"], capture_output=True,
                               text=True, timeout=10)
            out[f] = p.stdout if p.returncode == 0 else None
        except Exception:
            out[f] = None
    return out


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    seen = set()
    n = 0
    with gzip.open(OUT / "sessions.jsonl.gz", "wt") as fo:
        for base in BASES:
            for repo in REPOS:
                sp = base / repo / "state.json"
                if not sp.exists():
                    continue
                state = json.loads(sp.read_text()).get("runs", {})
                for f in sorted((base / repo / "transcripts").glob("*/*.jsonl")):
                    sid = f.stem
                    if sid in seen:
                        continue
                    d = f.parent.name
                    rid = re.sub(r"^.*-runs-(?=(train|eval)-)", "", d)
                    rid = re.sub(r"-repo$", "", rid)
                    m = RUN.match(rid)
                    if not m:
                        continue
                    phase, _, h, rnd, arm = m.groups()
                    arm = arm or "train"
                    st = state.get(rid, {})
                    met = st.get("metrics") or {}
                    passed = met.get("hidden_passed", met.get("passed"))
                    if passed is None and st.get("status") in ("passed", "failed"):
                        passed = st["status"] == "passed"
                    p = parse(f)
                    if not p["requests"]:
                        continue
                    seen.add(sid)
                    edited = set()
                    for q in p["requests"]:
                        for c in q["calls"]:
                            fp = c["input"].get("file_path") if isinstance(c["input"], dict) else None
                            if c["name"] in ("Edit", "Write", "MultiEdit") and isinstance(fp, str) \
                                    and not fp.startswith("/"):
                                edited.add(fp)
                    sc = st.get("score") or {}
                    changed = sc.get("changed_files") or []
                    rdir = base / repo / "runs" / rid / "repo"
                    orig = originals(rdir, edited | set(changed)) if rdir.is_dir() else {}
                    reason = (met.get("status_reason") or "")
                    cap = 30 if "(30)" in reason else 60 if "(60)" in reason else (30 if "repobench" in str(base) else 60)
                    rec = {"sid": sid, "base": base.parent.name, "repo_dir": repo,
                           "repo": repo.replace("-sonnet5", ""), "rid": rid, "phase": phase, "arm": arm,
                           "task": f"{repo.replace('-sonnet5', '')}-{h}", "round": int(rnd),
                           "passed": passed, "state_sid": met.get("session_id"), "cost_reported": met.get("cost_usd"),
                           "censored": "max_turns" in reason, "cap": cap,
                           "changed_files": changed, "touched_gold": sc.get("touched_gold"),
                           "originals": orig, **p}
                    fo.write(json.dumps(rec) + "\n")
                    n += 1
    print("sessions", n, file=sys.stderr)


if __name__ == "__main__":
    main()
