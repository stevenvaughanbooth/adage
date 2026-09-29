#!/usr/bin/env python3
"""Claude Code transcripts -> sequences of ABSTRACT actions (role, environment, scope).

  python3 abstract.py <transcript.jsonl>...

An abstract action keeps what matters for aligning a trajectory to a procedure and drops what
does not: `.venv/bin/python -m pytest tests/test_termui.py -q -x` and
`.venv/bin/python -m pytest tests/test_termui.py::test_prompt -xvs` are both
"verify via project-venv on a file"; `python3 -m unittest` is "verify via system on the repo".
Each action keeps its concrete command (for slot learning), its repository-relative path, whether
it failed, and the head of its output (for expected observations).
"""

from __future__ import annotations

import json
import re
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path

from procedure import env_of, role_of, scope_of, split_env, argv_of

EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
READ_TOOLS = {"Read"}
SEARCH_TOOLS = {"Grep", "Glob", "LS"}


@dataclass
class Action:
    i: int
    tool: str
    role: str  # lookup read search edit regenerate verify run install vcs remove other
    env: str
    scope: str
    cmd: str = ""  # the (first meaningful segment of the) shell command
    path: str = ""  # repository-relative path of a file tool
    failed: bool = False
    out: str = ""  # head of the output
    piped: bool = False

    def key(self) -> str:
        return f"{self.role}/{self.env}/{self.scope}"


@dataclass
class Session:
    id: str
    prompt: str
    cwd: str
    actions: list[Action] = field(default_factory=list)
    directives: list[str] = field(default_factory=list)  # adage-reflex route ids delivered


def segments(cmd: str) -> list[str]:
    """The meaningful segments of a compound shell line: `cd x && make test 2>&1 | tail` -> [make test]."""
    cmd = re.sub(r"\s+\d?>&\d|\s+2>/dev/null|\s+>/dev/null", "", cmd)
    parts = re.split(r"\s*(?:&&|\|\||;)\s*", cmd)
    out = []
    for p in parts:
        p = p.split(" | ")[0].strip()
        if not p or p.startswith(("cd ", "echo ", "source ", "export ")) or p in ("cd",):
            continue
        out.append(p)
    return out


def relpath(p: str, cwd: str) -> str:
    if cwd and p.startswith(cwd.rstrip("/") + "/"):
        return p[len(cwd.rstrip("/")) + 1:]
    return p


def classify_bash(cmd: str, cwd: str) -> tuple[str, str, str, str]:
    """(role, env, scope, the segment that decided) for one Bash call."""
    segs = segments(cmd.replace(cwd.rstrip("/") + "/", "") if cwd else cmd) or [cmd]
    rank = ["verify", "regenerate", "lookup", "run", "edit", "install", "remove", "search", "read", "vcs"]
    best = None
    for s in segs:
        r = role_of(s)
        a, _ = split_env(argv_of(s))
        p = a[0] if a else ""
        if r == "run" and (p in ("python", "python3") or p.startswith(".venv/bin/python")) and "-c" in a:
            r = "run"
        if p != "git" and re.search(r"(cat\s*>|cat\s*<<|tee\s|>\s*[\w./-]+\.py)", s):
            r = "edit"
        if best is None or rank.index(r) < rank.index(best[0]):
            best = (r, env_of(s), scope_of(s), s)
    return best


def read_session(path: str | Path) -> Session:
    lines = [json.loads(x) for x in Path(path).read_text().splitlines() if x.strip()]
    prompt, cwd, sid = "", "", Path(path).stem
    for m in lines:
        if m.get("type") == "user" and not prompt:
            c = m.get("message", {}).get("content")
            if isinstance(c, str):
                prompt, cwd = c, m.get("cwd", "")
    sess = Session(sid, prompt, cwd)
    by_id: dict[str, Action] = {}
    for m in lines:
        if m.get("type") == "assistant":
            for c in m.get("message", {}).get("content", []) or []:
                if c.get("type") != "tool_use":
                    continue
                name, inp = c.get("name"), c.get("input") or {}
                if name == "Bash":
                    cmd = inp.get("command", "")
                    role, env, scope, seg = classify_bash(cmd, cwd)
                    a = Action(len(sess.actions), name, role, env, scope, cmd=seg, piped=" | " in cmd)
                elif name in EDIT_TOOLS:
                    a = Action(len(sess.actions), name, "edit", "tool", "file", path=relpath(inp.get("file_path", ""), cwd))
                elif name in READ_TOOLS:
                    a = Action(len(sess.actions), name, "read", "tool", "file", path=relpath(inp.get("file_path", ""), cwd))
                elif name in SEARCH_TOOLS:
                    a = Action(len(sess.actions), name, "search", "tool", "path",
                               path=relpath(inp.get("path", "") or "", cwd), cmd=inp.get("pattern", ""))
                else:
                    a = Action(len(sess.actions), name, "other", "tool", "none")
                sess.actions.append(a)
                by_id[c.get("id")] = a
        elif m.get("type") == "user":
            for c in m.get("message", {}).get("content", []) or []:
                if isinstance(c, dict) and c.get("type") == "tool_result" and c.get("tool_use_id") in by_id:
                    a = by_id[c["tool_use_id"]]
                    a.failed = bool(c.get("is_error"))
                    body = c.get("content")
                    if isinstance(body, list):
                        body = " ".join(x.get("text", "") for x in body if isinstance(x, dict))
                    a.out = (body or "")[:600]
                    # A piped failure exits 0: read the signature from the output.
                    if a.piped and re.search(r"No module named|Traceback|RuntimeError|FAILED|Error:", a.out or ""):
                        a.failed = a.failed or bool(re.search(r"No module named|ACME_ENV is not set", a.out))
        elif m.get("type") == "system" or m.get("type") == "attachment":
            t = json.dumps(m)
            sess.directives += re.findall(r'route=\\"([a-z0-9-]+)\\"', t)
    # Hook output lands in user/attachment records too.
    raw = Path(path).read_text()
    sess.directives = sorted(set(sess.directives) | set(re.findall(r'route=\\\\?"([a-z0-9-]+)\\\\?"', raw)))
    return sess


def main(argv):
    for p in argv:
        s = read_session(p)
        print(f"# {s.id}  {s.prompt[:60]!r}  directives={s.directives}")
        for a in s.actions:
            print(f"  {a.i:>2} {a.key():<34} {'FAIL ' if a.failed else ''}{a.cmd or a.path}"[:150])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
