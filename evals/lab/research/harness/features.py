"""Per-call features Sat(d) and W(d) are defined on: a line-for-line port of
`crates/adage-eval/src/savings_replay.rs` (`act`, `invocation`, the signature regexes).

HAND-WRITTEN MEASUREMENT INSTRUMENT. Everything in this module is a hand-written taxonomy (tool
names, runner spellings, error signatures). Under the sprint's principle 1 it may be used only to
*measure* a policy (Sat/W classes, the calibrated metric), never inside a learner. A policy that
imports from here to decide what to say is hard-coded, and `score()` cannot tell; the adversarial
round checks imports.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

ENV_SIG = re.compile(
    r"No module named|ModuleNotFoundError|command not found|: not found\b|Cannot find module|ERR_MODULE_NOT_FOUND"
    r"|executable file not found|is not recognized as|ImportError while loading conftest"
    r"|(python3?|pytest|/bin/\S+|tox|nox|vitest|jest|npx): No such file or directory")

EXECUTED = re.compile(
    r"collected [1-9]\d* items?|\b\d+ passed\b|\b\d+ failed\b|^\s*Tests:\s+\d|Test Files\s+\d|\bRan [1-9]\d* tests?\b"
    r"|^ok\s+\S+|^--- (PASS|FAIL)|^(PASS|FAIL)\s+\S|test result: (ok|FAILED)|^# tests [1-9]", re.M)

ENV_PROBE = re.compile(r"""
    \bpip3?\s+(install|list|show|freeze)\b | -m\s+pip\b | \buv\s+(pip|sync|venv|add|lock)\b | -m\s+venv\b
    | \bvirtualenv\b | (^|[;&|]\s*)which\s | \bcommand\s+-v\s | \btype\s+(-\w+\s+)?(python|pytest|pip)
    | \b(python3?|pytest|pip3?|node|npm|pnpm|tox|uv)\s+(--version|-V)\b
    | \bls\b[^\n|;&]*(\.venv|venv/|site-packages|/bin\b)
    | \bfind\b[^\n]*(venv|site-packages|bin/python|-name\s+["']?(pytest|pyproject|setup\.py|setup\.cfg|tox\.ini))
    | ^\s*(source|\.)\s+\S*activate\s*$
    | \b(npm|pnpm|yarn)\s+(install|ci|i)\b
    | \b(cat|head|less|grep\s.*)\s+[^\n|;&]*\b(pyproject\.toml|setup\.cfg|tox\.ini|pytest\.ini|noxfile\.py|requirements[\w.-]*\.txt)\b
    """, re.X)

MANIFEST_READ = re.compile(r"(^|/)(pyproject\.toml|setup\.cfg|tox\.ini|pytest\.ini|noxfile\.py|requirements[\w.-]*\.txt)$")
TESTISH = re.compile(r"(?i)test|spec\b|__tests__")

# The live dead-end counter (`row().dead_end` in Rust, `evals/launch/deadends.py`).
U11_SIG = re.compile(r"No module named pytest|pytest: command not found|command not found: pytest"
                     r"|No module named '(click|attr|attrs|marshmallow)'|python: command not found")
U11_TEST = re.compile(r"\b(pytest|py\.test|tox|nox)\b")


@dataclass(frozen=True)
class Invocation:
    interp: str  # system | venv | tox | nox | node | go | cargo
    runner: str  # pytest | unittest | tox | nox | vitest | jest | mocha | pkg-test | node-test | go-test | cargo-test


@dataclass
class Act:
    """One tool call a_k and its features (Rust `Act`)."""
    tool: str
    cmd: str = ""
    is_error: bool = False
    run: Invocation | None = None
    targets: tuple = ()
    executed: bool = False
    env_sig: bool = False
    env_probe: bool = False
    discovery: bool = False
    edited: str | None = None
    rejected: bool = False  # output starts with <tool_use_error>: no hook fired

    @property
    def failed(self) -> bool:
        return self.is_error or self.env_sig

    @property
    def env_friction(self) -> bool:
        return self.env_sig or self.env_probe or (self.run is not None and not self.executed and self.is_error)


def segments(cmd: str) -> list[str]:
    out, cur, sq, dq, i = [], [], False, False, 0
    while i < len(cmd):
        c = cmd[i]
        if c == "'" and not dq:
            sq = not sq
        elif c == '"' and not sq:
            dq = not dq
        if not sq and not dq and c in ";|&\n":
            s = "".join(cur).strip()
            if s:
                out.append(s)
            cur = []
            while i + 1 < len(cmd) and cmd[i + 1] in "&|":
                i += 1
        else:
            cur.append(c)
        i += 1
    s = "".join(cur).strip()
    if s:
        out.append(s)
    return out


def words(seg: str) -> list[str]:
    return [w.strip("\"'") for w in seg.split()]


def base(w: str) -> str:
    return w.rsplit("/", 1)[-1]


def is_python(w: str) -> bool:
    b = base(w)
    return b in ("python", "python3") or (b.startswith("python3.") and (b[8:] == "" or b[8:].isascii() and b[8:].isdigit()))


def in_venv(w: str) -> bool:
    return ".venv/bin/" in w or w.startswith("venv/bin/") or "/venv/bin/" in w


def invocation(cmd: str):
    """(Invocation, args after the runner) or None (Rust `invocation`)."""
    active = False
    for seg in segments(cmd):
        w = words(seg)
        while w:
            f = w[0]
            wrapper = f in ("env", "time", "exec", "nice", "command") or (
                "=" in f and not f.startswith("-") and "/" not in f) or f == "timeout"
            if not wrapper:
                break
            w.pop(0)
            if f == "timeout" and w and w[0][:1].isdigit():
                w.pop(0)
        if not w:
            continue
        first = w[0]
        if first in ("source", ".") and len(w) > 1 and w[1].endswith("bin/activate"):
            active = True
            continue
        py_env = lambda p: "venv" if (in_venv(p) or active) else "system"  # noqa: E731
        rest = lambda i: w[i:]  # noqa: E731
        if first == "uv" and len(w) > 1 and w[1] == "run":
            i = 2
            while i < len(w) and w[i].startswith("-"):
                i += 1
            r = invocation(" ".join(w[i:]))
            if r:
                return Invocation("venv", r[0].runner), r[1]
            continue
        if is_python(first):
            if "-m" in w:
                m = w.index("-m")
                mod = w[m + 1] if m + 1 < len(w) else None
                if mod in ("pytest", "py.test"):
                    return Invocation(py_env(first), "pytest"), rest(m + 2)
                if mod == "unittest":
                    return Invocation(py_env(first), "unittest"), rest(m + 2)
                if mod == "tox":
                    return Invocation("tox", "tox"), rest(m + 2)
                if mod == "nox":
                    return Invocation("nox", "nox"), rest(m + 2)
            continue
        b = base(first)
        if b in ("pytest", "py.test"):
            return Invocation(py_env(first), "pytest"), rest(1)
        if b == "tox":
            return Invocation("tox", "tox"), rest(1)
        if b == "nox":
            return Invocation("nox", "nox"), rest(1)
        if b in ("vitest", "jest", "mocha"):
            return Invocation("node", b), rest(1)
        if b in ("npx", "pnpm", "yarn", "npm", "bunx"):
            i = 1
            while i < len(w) and (w[i].startswith("-") or w[i] in ("exec", "run", "--")):
                if w[i] in ("--filter", "-F", "-C", "--dir"):
                    i += 1
                i += 1
            t = base(w[i]) if i < len(w) else None
            if t in ("vitest", "jest", "mocha"):
                return Invocation("node", t), rest(i + 1)
            if t is not None and (t == "test" or t.startswith("test:")):
                return Invocation("node", "pkg-test"), rest(i + 1)
        if b == "node" and len(w) > 1 and w[1] == "--test":
            return Invocation("node", "node-test"), rest(2)
        if b == "go" and len(w) > 1 and w[1] == "test":
            return Invocation("go", "go-test"), rest(2)
        if b == "cargo" and len(w) > 1 and w[1] == "test":
            return Invocation("cargo", "cargo-test"), rest(2)
    return None


def rel(p: str, cwd: str) -> str:
    c = cwd.rstrip("/")
    pre = c + "/"
    r = p[len(pre):] if p.startswith(pre) else p
    while r.startswith("./"):
        r = r[2:]
    return r


def targets(args, cwd):
    out = set()
    for a in args:
        if a.startswith("-"):
            continue
        p = a.split("::")[0]
        if ("/" in p or "." in p) and TESTISH.search(p) and "=" not in p:
            out.add(rel(p, cwd))
    return tuple(sorted(out))


GO_VALUED = {"-run", "-count", "-timeout", "-tags", "-p", "-parallel", "-coverprofile", "-bench", "-skip", "-cpu"}


def go_packages(args, cwd):
    out, skip = set(), False
    for a in args:
        if skip:
            skip = False
            continue
        if a.startswith("-"):
            skip = a in GO_VALUED
            continue
        p = rel(a, cwd)
        while p.startswith("./"):
            p = p[2:]
        p = p.rstrip("/")
        out.add("pkg:" + ("" if p == "." else p))
    return tuple(sorted(out or {"pkg:"}))


def act(tool: str, inp: dict, output: str, is_error: bool, cwd: str) -> Act:
    """Rust `act()`: features of one call, computed on the full output."""
    a = Act(tool=tool, is_error=is_error, rejected=output.lstrip().startswith("<tool_use_error>"))
    c = cwd.rstrip("/")
    inp = inp if isinstance(inp, dict) else {}
    s = lambda k: inp.get(k) if isinstance(inp.get(k), str) else ""  # noqa: E731
    if tool == "Bash":
        raw = s("command")
        a.cmd = raw.replace(c + "/", "").replace(c, ".") if c else raw
        a.env_sig = bool(ENV_SIG.search(output))
        inv = invocation(a.cmd)
        if inv:
            a.run = inv[0]
            a.targets = go_packages(inv[1], cwd) if inv[0].runner == "go-test" else targets(inv[1], cwd)
            a.executed = bool(EXECUTED.search(output)) and not a.env_sig
        else:
            segs = segments(a.cmd)
            root_ls = any((lambda w: bool(w) and w[0] == "ls" and all(x.startswith("-") or x in (".", "./") for x in w[1:]))(words(sg))
                          for sg in segs)
            a.env_probe = root_ls or bool(ENV_PROBE.search(a.cmd))
            first = ""
            for sg in segs:
                w = words(sg)
                if w and w[0] != "cd":
                    first = w[0]
                    break
            search = base(first) in ("find", "grep", "rg", "ls", "fd", "tree", "ag") or "git grep" in a.cmd or "git ls-files" in a.cmd
            a.discovery = search and bool(TESTISH.search(a.cmd))
    elif tool in ("Grep", "Glob"):
        a.discovery = bool(TESTISH.search(f"{s('pattern')} {s('path')} {s('glob')}"))
    elif tool == "Read":
        a.env_probe = bool(MANIFEST_READ.search(rel(s("file_path"), cwd)))
    elif tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
        p = s("file_path") or s("notebook_path")
        if p:
            a.edited = rel(p, cwd)
    return a


def dead_end(a: Act, output: str) -> bool:
    return a.tool == "Bash" and bool(U11_TEST.search(a.cmd)) and bool(U11_SIG.search(output))
