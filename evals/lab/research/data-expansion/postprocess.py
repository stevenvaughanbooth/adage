#!/usr/bin/env python3
"""Post-process finished RepoBench sessions of a runner root, for the research corpus.

  python3 postprocess.py <root> [<root> ...] [--keep-tree]

Per finished session (state.json status passed/failed, not `running`) under <root>/runs/<run-id>/:

  calls.jsonl   one line per model call (assistant message, de-duplicated by message id), in order:
                {i, t, msg_id, model, input, output, thinking, cache_read, cache_write, cache_write_1h,
                 usd, cum_usd, tools: [{name, id, input (truncated)}], usage_source}
                Usage is the message's FINAL usage from the session transcript (the stream's copy is
                taken at message start). `usd` prices it at list prices (spend.py PRICES; 1-hour cache
                writes at 2x input); `cum_usd` is the running sum, which matches the session's
                reported total (state.json metrics.cost_usd) to the cent.
  agent.diff    what the agent changed against the sandbox's base commit (tracked and untracked,
                hidden tests excluded, environment dirs excluded), so the tree can be deleted.
  result.json   the stream's final `result` event (usage totals, num_turns, cost, subtype).

Then the sandbox tree (<run>/repo) is deleted unless --keep-tree (disk: a vue-core tree is 16 MB).
Idempotent: sessions that already have calls.jsonl and no tree are skipped. Costs $0.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
from spend import PRICES  # noqa: E402

ENV_DIRS = (".venv/", "node_modules/", "target/", "build/", "__pycache__/")


def price(model: str):
    return PRICES["sonnet" if "sonnet" in (model or "") else "haiku"]


def transcript_usage(transcript: Path | None) -> dict:
    """Final per-message usage from the Claude Code transcript (the stream's assistant events carry
    the usage at the start of the message: output tokens there are not final)."""
    out = {}
    if transcript and transcript.exists():
        for line in transcript.read_text().splitlines():
            try:
                m = json.loads(line)
            except ValueError:
                continue
            if m.get("type") == "assistant":
                msg = m.get("message") or {}
                if msg.get("id"):
                    out[msg["id"]] = msg.get("usage") or {}
    return out


def calls(stream: Path, model: str, transcript: Path | None = None) -> tuple[list[dict], dict]:
    pi, po, pr, pw = price(model)
    pw1h = 2 * pi  # 1-hour cache writes (what Claude Code 2.x uses) cost 2x input; 5-minute ones 1.25x
    final = transcript_usage(transcript)
    out, idx, result = [], {}, {}
    for line in stream.read_text().splitlines():
        try:
            w = json.loads(line)
            m = json.loads(w["line"])
        except (ValueError, KeyError, TypeError):
            continue
        if m.get("type") == "result":
            result = m
            continue
        if m.get("type") != "assistant":
            continue
        msg = m.get("message") or {}
        mid = msg.get("id")
        tools = [{"name": c.get("name"), "id": c.get("id"), "input": json.dumps(c.get("input"))[:600]}
                 for c in msg.get("content") or [] if c.get("type") == "tool_use"]
        if mid in idx:  # one stream event per content block; same message id and usage
            rec = out[idx[mid]]
            rec["tools"] += [x for x in tools if x["id"] not in {y["id"] for y in rec["tools"]}]
            continue
        u = final.get(mid) or msg.get("usage") or {}
        cc = u.get("cache_creation") or {}
        w1h = cc.get("ephemeral_1h_input_tokens", 0)
        rec = {"i": len(out), "t": w.get("t"), "msg_id": mid, "model": msg.get("model"),
               "input": u.get("input_tokens", 0), "output": u.get("output_tokens", 0),
               "cache_read": u.get("cache_read_input_tokens", 0),
               "cache_write": u.get("cache_creation_input_tokens", 0), "cache_write_1h": w1h,
               "thinking": (u.get("output_tokens_details") or {}).get("thinking_tokens", 0), "tools": tools,
               "usage_source": "transcript" if mid in final else "stream"}
        rec["usd"] = round(rec["input"] * pi + rec["output"] * po + rec["cache_read"] * pr
                           + (rec["cache_write"] - w1h) * pw + w1h * pw1h, 6)
        idx[mid] = len(out)
        out.append(rec)
    cum = 0.0
    for r in out:
        cum += r["usd"]
        r["cum_usd"] = round(cum, 6)
    return out, result


def agent_diff(tree: Path, exclude: set) -> str:
    q = ["git", "-C", str(tree), "--no-optional-locks"]
    roots = subprocess.run(q + ["rev-list", "--max-parents=0", "HEAD"], capture_output=True, text=True).stdout.split()
    if not roots:
        return ""
    # Stage everything (intent-to-add for untracked) in a throwaway index so new files show.
    env_idx = tree.parent / ".pp-index"
    env = {"GIT_INDEX_FILE": str(env_idx), "PATH": "/usr/bin:/bin"}
    subprocess.run(q + ["read-tree", "HEAD"], env=env, capture_output=True)
    subprocess.run(q + ["add", "-A", "--", "."], env=env, capture_output=True)
    names = subprocess.run(q + ["diff", "--cached", "--name-only", roots[-1]], env=env, capture_output=True,
                           text=True).stdout.split("\n")
    keep = [n for n in names if n and n not in exclude and not any(d in n + "/" or n.startswith(d) for d in ENV_DIRS)]
    diff = subprocess.run(q + ["diff", "--cached", roots[-1], "--", *keep], env=env, capture_output=True,
                          text=True).stdout if keep else ""
    env_idx.unlink(missing_ok=True)
    return diff


_TASKS: dict = {}


def task_tests(tid: str) -> list[str]:
    """The task's hidden test files (the scorer wrote them over the agent's tree)."""
    if not _TASKS:
        for p in (HERE.parents[2] / "repobench" / "tasks").glob("*.json"):
            for t in json.loads(p.read_text())["tasks"]:
                _TASKS[t["id"]] = t["test_files"]
    return _TASKS.get(tid, [])


def process(root: Path, keep_tree: bool = False, redo: bool = False) -> int:
    st = json.loads((root / "state.json").read_text())
    model = st.get("model") or ""
    n = 0
    for rid, r in st["runs"].items():
        if r.get("status") not in ("passed", "failed"):
            continue
        rd = root / "runs" / rid
        if not rd.is_dir():
            continue
        tree = rd / "repo"
        if (rd / "calls.jsonl").exists() and not tree.exists() and not redo:
            continue
        sid = (r.get("metrics") or {}).get("session_id")
        tr = next(iter((root / "private" / "transcripts").glob(f"*/{sid}.jsonl")), None) if sid else None
        if (rd / "stream.jsonl").exists():
            cs, result = calls(rd / "stream.jsonl", model, tr)
            (rd / "calls.jsonl").write_text("".join(json.dumps(c) + "\n" for c in cs))
            (rd / "result.json").write_text(json.dumps(result))
        if tree.is_dir():
            hidden = set(task_tests(r["task"]))
            (rd / "agent.diff").write_text(agent_diff(tree, hidden))
            if not keep_tree:
                subprocess.run(["chmod", "-R", "u+w", str(tree)], capture_output=True)
                shutil.rmtree(tree, ignore_errors=True)
        n += 1
    return n


def main(argv):
    keep, redo = "--keep-tree" in argv, "--redo" in argv
    for a in [x for x in argv if not x.startswith("--")]:
        print(f"{a}: {process(Path(a), keep, redo)} session(s) processed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
