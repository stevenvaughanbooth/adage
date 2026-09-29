"""Method (b): anti-unification (least general generalisation) over argv with typed holes.

Each token of the shell-normalised command (shell.py: heredoc bodies, `cd`, redirections removed;
segments joined by their connectors; pipes kept as `|prog`) is typed the way G1's value types
type a token:

    flag      `-xvs`, `--tb=short`            kept literal (strict) or folded into a set (loose)
    path      `tests/test_core.py`            hole <P:area>, area ∈ test|src|outside|scratch|doc|config|env
    test id   `tests/test_x.py::test_y`       hole <T:area>
    code      a `-c` body or long quoted text hole <CODE>
    number    `50`, `-n 20`'s 20              hole <N>
    value     the value of a selecting option (`-k parser`, `-run TestX`, `-t "name"`)  hole <S>
    word      anything else                   literal (program, subcommand, module)

The anti-unifier of a set of commands with the same skeleton keeps every position where all
members agree and puts the join of the members' types where they differ; so a cluster is
exactly a skeleton, and its template is the LGG of its members. `strict` keeps flags in order
and every hole; `loose` also folds runs of the same hole (`<P:test> <P:test>` → `<P:test>+`),
sorts flags as a set and drops output filters (the G1 star-folding step and the covering
variant's "options that select nothing").
"""
from __future__ import annotations

import re

import facts as F
import grammar as G
import shell

VALUE_OPTS = {"-k", "-t", "-m", "--testNamePattern", "-run", "--run", "-p", "--project", "-n", "--reporter", "--tb", "-c", "-e", "--grep", "-g"}
PY_MODULES = {"pytest", "py.test", "pip", "unittest", "venv", "ensurepip", "http.server", "json.tool", "doctest", "mypy", "black", "ruff"}
SELECT_OPTS = {"-k", "-t", "--testNamePattern", "-run", "--run", "-m"}


def tok_type(t: str, prev: str, fx: F.RepoFacts) -> str | None:
    """None: a literal; otherwise the hole's type."""
    if prev == "-m" and t in PY_MODULES:
        return None  # `python -m pytest`: the module is the identity, not a marker expression
    if prev in SELECT_OPTS:
        return "S"
    if prev in ("-c", "-e", "-p") or "\n" in t or (len(t) > 40 and " " in t):
        return "CODE"
    if re.fullmatch(r"\d+", t):
        return "N"
    if t.startswith("-"):
        return None
    if "::" in t:
        return "T:" + G.area(t.split("::")[0], fx)
    if "/" in t or re.search(r"\.(py|ts|js|go|tsx|mjs|json|toml|md|rst|txt|cfg)$", t) or t in (".", "./..."):
        return "P:" + G.area(t, fx)
    if " " in t or '"' in t or "'" in t or "*" in t or "|" in t:
        return "S"
    if prev in VALUE_OPTS:
        return "S"
    return None


def skeleton(command: str, cwd: str, fx: F.RepoFacts, loose: bool = False) -> str:
    segs = shell.segments(command, cwd)
    parts = []
    for s in segs:
        toks, flags, prev = [], [], ""
        for j, t in enumerate(s.argv):
            ty = None if j == 0 else tok_type(t, prev, fx)  # the program is always a literal
            if ty is None and t.startswith("-"):
                if loose:
                    flags.append(t.split("=")[0])
                else:
                    toks.append(t)
            elif ty is None:
                toks.append(t)
            else:
                h = f"<{ty}>"
                if loose and toks and toks[-1] in (h, h + "+"):
                    toks[-1] = h + "+"
                else:
                    toks.append(h)
            prev = t
        if s.activated:
            toks.insert(0, f"[{s.activated}]")
        if s.cwd:
            toks.insert(0, f"[cd <P:{G.area(s.cwd, fx)}>]")
        if s.stdin_code:
            toks.append("<<CODE")
        if s.writes:
            toks.append(f"> <P:{G.area(s.writes, fx)}>")
        if loose:
            toks += sorted(set(flags))
        elif s.piped:
            toks += ["|" + p for p in s.piped]
        parts.append((s.connector if s.connector in ("&&", "||", ";") else "") + " ".join(toks))
    return " ".join(p for p in parts if p).strip() or "<empty>"


def lgg(members: list[list[str]]) -> list[str]:
    """Plotkin's LGG of token lists of equal length (positions that agree stay; others become holes)."""
    if not members:
        return []
    n = len(members[0])
    out = []
    for i in range(n):
        vals = {m[i] for m in members if i < len(m)}
        out.append(vals.pop() if len(vals) == 1 else "<?>")
    return out


def template(commands: list[tuple[str, str]], fx: F.RepoFacts, loose: bool = False) -> str:
    """The LGG template of commands sharing a skeleton: literal where every member agrees."""
    concrete = []
    for c, cwd in commands:
        segs = shell.segments(c, cwd)
        concrete.append([t for s in segs for t in s.argv])
    sk = skeleton(commands[0][0], commands[0][1], fx, loose).split()
    same_len = [m for m in concrete if len(m) == len(concrete[0])]
    if len(same_len) == len(concrete):
        g = lgg(concrete)
        return " ".join(g)
    return " ".join(sk)
