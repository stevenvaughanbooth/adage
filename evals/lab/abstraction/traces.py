"""Load RepoBench Claude Code transcripts into a small, method-neutral session model.

Layout (evals/live/runner.py, evals/repobench): a run root holds `runs/<repo>[-<model>]/runs/<run>/`
with `stream.jsonl` (the `claude -p --output-format stream-json` lines, each wrapped as
`{"t": secs, "line": "<json>"}`) and `repo/` (the sandbox checkout; `.venv` and `node_modules`
are deleted after scoring). `runs/<repo>/state.json` names each run's task, phase, arm and
status.

Roots, newest first: /tmp/adage-rb2 (RepoBench v2: held-out eval runs, 60-turn cap) and
/tmp/adage-repobench (v1: the training runs and 30-turn eval runs). Only the five real
repositories are loaded (the generated `gen-*` sandboxes and the pilot are left out).
"""
from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass, field
from pathlib import Path

ROOTS = [("v2", Path("/tmp/adage-rb2")), ("v1", Path("/tmp/adage-repobench"))]
REPOS = ("attrs", "click", "marshmallow", "urfave-cli", "vue-core")
TASKS_DIR = Path(__file__).resolve().parents[2] / "repobench" / "tasks"


@dataclass
class Call:
    idx: int
    tool: str
    input: dict
    out: str = ""
    err: bool = False
    turn: int = 0

    @property
    def command(self) -> str:
        return str(self.input.get("command", "")) if self.tool == "Bash" else ""

    @property
    def path(self) -> str:
        return str(self.input.get("file_path") or self.input.get("path") or self.input.get("notebook_path") or "")


@dataclass
class Session:
    sid: str  # "<root>/<repo-dir>/<run>"
    root: str  # v1 | v2
    repo: str  # attrs, click, ...
    model: str  # haiku | sonnet
    run: str
    task: str  # e.g. click-0cd28d65
    phase: str  # train | eval
    arm: str  # train | control | adage | agents-md
    status: str  # passed | failed | ...
    cwd: str
    calls: list[Call] = field(default_factory=list)
    prompt: str = ""

    def rel(self, p: str) -> str:
        """A path relative to the sandbox checkout when it is inside it."""
        if not p:
            return p
        c = self.cwd.rstrip("/") + "/"
        return p[len(c):] if p.startswith(c) else p


def _text(b) -> str:
    c = b.get("content")
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        return "\n".join(x.get("text", "") for x in c if isinstance(x, dict))
    return ""


def parse_stream(path: Path) -> tuple[str, str, list[Call], str]:
    cwd, model, calls, by_id, turn, prompt = "", "", [], {}, 0, ""
    seen_msgs = set()
    with open(path) as f:
        for line in f:
            try:
                m = json.loads(json.loads(line)["line"])
            except Exception:
                continue
            t = m.get("type")
            if t == "system" and m.get("subtype") == "init":
                cwd = m.get("cwd", cwd)
                model = m.get("model", model)
            elif t == "assistant":
                msg = m.get("message", {})
                mid = msg.get("id")
                if mid not in seen_msgs:
                    seen_msgs.add(mid)
                    turn += 1
                for b in msg.get("content", []) or []:
                    if isinstance(b, dict) and b.get("type") == "tool_use":
                        c = Call(idx=len(calls), tool=b["name"], input=b.get("input") or {}, turn=turn)
                        by_id[b["id"]] = c
                        calls.append(c)
            elif t == "user":
                cont = m.get("message", {}).get("content")
                if not prompt and isinstance(cont, str):
                    prompt = cont
                if not prompt and isinstance(cont, list) and not any(isinstance(b, dict) and b.get("type") == "tool_result" for b in cont):
                    prompt = "\n".join(b.get("text", "") for b in cont if isinstance(b, dict))
                if isinstance(cont, list):
                    for b in cont:
                        if isinstance(b, dict) and b.get("type") == "tool_result" and b.get("tool_use_id") in by_id:
                            c = by_id[b["tool_use_id"]]
                            c.out = _text(b)
                            c.err = bool(b.get("is_error"))
    return cwd, model, calls, prompt


def load(roots=ROOTS, repos=REPOS) -> list[Session]:
    out: list[Session] = []
    for tag, root in roots:
        runs_root = root / "runs"
        if not runs_root.is_dir():
            continue
        for rdir in sorted(runs_root.iterdir()):
            repo = rdir.name.replace("-sonnet5", "")
            if repo not in repos:
                continue
            state = {}
            sp = rdir / "state.json"
            if sp.exists():
                state = json.load(open(sp)).get("runs", {})
            for run in sorted((rdir / "runs").iterdir()) if (rdir / "runs").is_dir() else []:
                st = run / "stream.jsonl"
                if not st.exists():
                    continue
                meta = state.get(run.name, {})
                m = re.match(r"(train|eval)-(.+?-[0-9a-f]{8})-r\d+(?:-(.+))?$", run.name)
                if not m:
                    continue
                cwd, model, calls, prompt = parse_stream(st)
                if not calls:
                    continue
                out.append(Session(
                    sid=f"{tag}/{rdir.name}/{run.name}", root=tag, repo=repo,
                    model="sonnet" if "sonnet" in (model or rdir.name) else "haiku",
                    run=run.name, task=meta.get("task", m.group(2)), phase=m.group(1),
                    arm=meta.get("arm", m.group(3) or "train"), status=meta.get("status", "?"),
                    cwd=cwd or str(run / "repo"), calls=calls, prompt=prompt or meta.get("prompt", "")))
    return out


def tasks(repo: str) -> dict[str, dict]:
    p = TASKS_DIR / f"{repo}.json"
    if not p.exists():
        return {}
    return {t["id"]: t for t in json.load(open(p))["tasks"]}


if __name__ == "__main__":
    ss = load()
    from collections import Counter
    print(len(ss), "sessions", sum(len(s.calls) for s in ss), "calls")
    print(Counter((s.root, s.repo, s.model, s.phase) for s in ss))
    print(sum(1 for s in ss for c in s.calls if c.tool == "Bash"), "bash calls")
