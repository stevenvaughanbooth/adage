"""Normalise a Bash tool call into segments of argv (method-neutral; every method starts here).

What is removed, and kept as modifiers rather than identity:
- the sandbox's absolute path (repo-relative paths; `$R` for the checkout root itself);
- heredoc bodies (the command keeps `<<HEREDOC` and a `stdin_code` flag);
- `cd <dir>` segments (the directory is kept as `cwd` of the following segments);
- `export X=…`, leading `X=… ` assignments, `timeout N`, `time`, `env`;
- `source …/activate` (it marks the following segments as running in that environment);
- redirections (`2>&1`, `>/dev/null`, `> log`), with `> file` kept as `writes`;
- pipeline tails that only filter output (`| head -50`, `| tail`, `| grep x`), kept as `piped`.

A call becomes a list of `Seg`; its **principal** segment is chosen by role priority in grammar.py.
"""
from __future__ import annotations

import re
import shlex
from dataclasses import dataclass, field

FILTERS = {"head", "tail", "grep", "egrep", "rg", "sort", "uniq", "wc", "cat", "less", "more", "tee", "cut", "awk", "sed", "tr", "xargs", "jq", "column", "nl"}
WRAPPERS = {"timeout", "time", "env", "nice", "stdbuf", "command"}
HEREDOC_RE = re.compile(r"<<-?\s*(['\"]?)(\w+)\1")


@dataclass
class Seg:
    argv: list[str]
    cwd: str = ""  # repo-relative directory a preceding `cd` moved to ("" = root, "/tmp" = outside)
    activated: str = ""  # environment activated by a preceding `source …/activate`
    stdin_code: bool = False  # a heredoc or here-string feeds it
    piped: list[str] = field(default_factory=list)  # output filters after it
    writes: str = ""  # `> file` target
    env_assign: list[str] = field(default_factory=list)
    connector: str = ""  # how it joins the previous segment: && || ; |

    @property
    def text(self) -> str:
        return " ".join(self.argv)


def strip_heredocs(cmd: str) -> tuple[str, bool]:
    """Drop heredoc bodies: keep the line that opens one, skip to its terminator."""
    if "<<" not in cmd:
        return cmd, False
    out, lines, i, had = [], cmd.split("\n"), 0, False
    while i < len(lines):
        ln = lines[i]
        m = HEREDOC_RE.search(ln)
        out.append(ln)
        i += 1
        if m and "<<<" not in ln:
            had = True
            term = m.group(2)
            while i < len(lines) and lines[i].strip() != term:
                i += 1
            # the terminator line may carry a continuation (`EOF` then `&& …` is rare; `)` from $(cat <<EOF) too)
            i += 1
    return "\n".join(out), had


def relpath(tok: str, cwd: str) -> str:
    c = cwd.rstrip("/")
    if not c:
        return tok
    if tok == c or tok == c + "/":
        return "$R"
    return tok.replace(c + "/", "")


def tokenize(cmd: str) -> list[str]:
    try:
        lex = shlex.shlex(cmd, posix=True, punctuation_chars=";&|<>")
        lex.whitespace_split = True
        lex.commenters = ""
        return list(lex)
    except ValueError:
        return cmd.split()


def segments(command: str, cwd: str) -> list[Seg]:
    cmd, had_heredoc = strip_heredocs(command)
    cmd = relpath(cmd, cwd)
    # $(cat <<'EOF' …) commit messages: the body is gone; drop the dangling subshell
    cmd = cmd.replace("$(cat <<", "<<")
    toks = tokenize(cmd.replace("\n", " ; "))
    segs: list[Seg] = []
    cur: list[str] = []
    conn = ""
    state = {"cwd": "", "activated": ""}
    pending_pipe = False

    def flush(next_conn: str):
        nonlocal cur, conn, pending_pipe
        argv = cur
        cur = []
        if not argv:
            conn = next_conn or conn
            return
        seg = Seg(argv=[], cwd=state["cwd"], activated=state["activated"], connector=conn)
        # redirections and here-strings
        a, i = [], 0
        while i < len(argv):
            t = argv[i]
            if t in (">", ">>", "<", "<<", "<<<", ">&", "&>") or re.fullmatch(r"\d?>>?|\d?>&\d?|&>>?", t):
                nxt = argv[i + 1] if i + 1 < len(argv) else ""
                if t in (">", ">>", "&>") and nxt and not nxt.startswith("&") and nxt != "/dev/null":
                    seg.writes = nxt
                if t in ("<<", "<<<"):
                    seg.stdin_code = True
                i += 2 if nxt and not re.fullmatch(r"&\d?|\d", nxt) or t in ("<<", "<<<", ">", ">>", "<") else 1
                # `2>&1` tokenises as 2 > & 1 in punctuation mode
                continue
            if re.fullmatch(r"&?\d", t) and a and a[-1] in (">", "&"):
                i += 1
                continue
            a.append(t)
            i += 1
        # leading assignments and wrappers
        while a and (re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", a[0]) or a[0] in WRAPPERS):
            if a[0] in WRAPPERS:
                a.pop(0)
                while a and re.fullmatch(r"-\w+|\d+[smh]?", a[0]):
                    a.pop(0)
            else:
                seg.env_assign.append(a.pop(0).split("=")[0])
        seg.argv = [x for x in a if x not in ("&", "1", "2") or len(a) == 1]
        if not seg.argv:
            conn = next_conn
            return
        head = seg.argv[0]
        if head == "cd":
            d = seg.argv[1] if len(seg.argv) > 1 else ""
            state["cwd"] = "" if d in ("$R", "", ".", cwd.rstrip("/")) else d
        elif head in ("source", ".") and len(seg.argv) > 1 and "activate" in seg.argv[1]:
            state["activated"] = seg.argv[1].replace("/bin/activate", "").strip("/") or "venv"
        elif head == "export":
            pass
        elif pending_pipe and segs and head in FILTERS:
            segs[-1].piped.append(head)
        else:
            if had_heredoc and "<<" in command and not seg.stdin_code:
                pass
            segs.append(seg)
        pending_pipe = next_conn == "|"
        conn = next_conn

    i = 0
    while i < len(toks):
        t = toks[i]
        if t in ("&&", "||", ";", "|", ";;", "&"):
            flush(t)
        else:
            cur.append(t)
        i += 1
    flush("")
    if had_heredoc:
        for s in segs:
            if s.argv and re.match(r"^(\S*/)?(python\d?(\.\d+)?|node|bash|sh)$", s.argv[0]) and len(s.argv) == 1:
                s.stdin_code = True
    return segs


if __name__ == "__main__":
    for c in [
        "cd /r && .venv/bin/python -m pytest tests/test_core.py::test_x -xvs 2>&1 | head -50",
        "uv run pytest -k parser",
        "source .venv/bin/activate && python -m pytest tests/ -q",
        "npx vitest run packages/runtime-core",
        "python3 << 'EOF'\nimport click\nprint(1)\nEOF",
        "cat > /tmp/t.py << 'EOF'\nx\nEOF\npython /tmp/t.py",
        "pip install pytest -q && python -m pytest tests/ -x -k \"cached\" --tb=short 2>&1 | head -150",
        "git add a.go && git commit -m \"$(cat <<'EOF'\nFix\n\nbody\nEOF\n)\"",
        "export PATH=\"$PATH:/x/bin\" && python -m pytest tests/test_a.py -q 2>&1 | tail -20",
        "timeout 60 go test -run 'TestA|TestB' -v ./... 2>&1",
    ]:
        print(repr(c[:60]))
        for s in segments(c, "/r"):
            print("   ", s)
