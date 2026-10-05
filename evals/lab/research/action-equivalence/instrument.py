"""MEASUREMENT INSTRUMENT ONLY: a Python port of the replay metric's per-call features and its
Sat(d) / W(d) / S(d) scoring (`crates/adage-eval/src/savings_replay.rs`, `evals/replay/README.md`).

Everything here is hand-written (runner names, environment signatures, probe regexes), exactly as
in the Rust replay. It is used to *score* directives on held-out traces, never by a learner.
Directive cost C(d) is approximated by the replay's measured 0.02 calls per delivered directive
(its table: 0.01-0.03 at ~100 tokens).
"""
from __future__ import annotations

import re
from dataclasses import dataclass, field

ENV_SIG = re.compile(r"No module named|ModuleNotFoundError|command not found|: not found\b|Cannot find module|ERR_MODULE_NOT_FOUND|executable file not found|is not recognized as|ImportError while loading conftest|(python3?|pytest|/bin/\S+|tox|nox|vitest|jest|npx): No such file or directory")
EXECUTED = re.compile(r"(?m)collected [1-9]\d* items?|\b\d+ passed\b|\b\d+ failed\b|^\s*Tests:\s+\d|Test Files\s+\d|\bRan [1-9]\d* tests?\b|^ok\s+\S+|^--- (PASS|FAIL)|^(PASS|FAIL)\s+\S|test result: (ok|FAILED)|^# tests [1-9]")
ENV_PROBE = re.compile(r"""(?x)
    \bpip3?\s+(install|list|show|freeze)\b | -m\s+pip\b | \buv\s+(pip|sync|venv|add|lock)\b | -m\s+venv\b
    | \bvirtualenv\b | (^|[;&|]\s*)which\s | \bcommand\s+-v\s | \btype\s+(-\w+\s+)?(python|pytest|pip)
    | \b(python3?|pytest|pip3?|node|npm|pnpm|tox|uv)\s+(--version|-V)\b
    | \bls\b[^\n|;&]*(\.venv|venv/|site-packages|/bin\b)
    | \bfind\b[^\n]*(venv|site-packages|bin/python|-name\s+["']?(pytest|pyproject|setup\.py|setup\.cfg|tox\.ini))
    | ^\s*(source|\.)\s+\S*activate\s*$
    | \b(npm|pnpm|yarn)\s+(install|ci|i)\b
    | \b(cat|head|less|grep\s.*)\s+[^\n|;&]*\b(pyproject\.toml|setup\.cfg|tox\.ini|pytest\.ini|noxfile\.py|requirements[\w.-]*\.txt)\b
    """)
MANIFEST_READ = re.compile(r"(^|/)(pyproject\.toml|setup\.cfg|tox\.ini|pytest\.ini|noxfile\.py|requirements[\w.-]*\.txt)$")
TESTISH = re.compile(r"(?i)test|spec\b|__tests__")


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
    return b in ("python", "python3") or (b.startswith("python3.") and b[8:].isdigit())


def in_venv(w: str) -> bool:
    return ".venv/bin/" in w or w.startswith("venv/bin/") or "/venv/bin/" in w


def invocation(cmd: str):
    """(interp, runner) of the test runner a command runs, or None."""
    active = False
    for seg in segments(cmd):
        w = words(seg)
        while w:
            f = w[0]
            wrapper = f in ("env", "time", "exec", "nice", "command") or ("=" in f and not f.startswith("-") and "/" not in f) or f == "timeout"
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
        py_env = lambda p: "venv" if in_venv(p) or active else "system"
        if first == "uv" and len(w) > 1 and w[1] == "run":
            i = 2
            while i < len(w) and w[i].startswith("-"):
                i += 1
            r = invocation(" ".join(w[i:]))
            if r:
                return ("venv", r[1])
            continue
        if is_python(first):
            if "-m" in w:
                m = w.index("-m")
                mod = w[m + 1] if m + 1 < len(w) else ""
                if mod in ("pytest", "py.test"):
                    return (py_env(first), "pytest")
                if mod == "unittest":
                    return (py_env(first), "unittest")
                if mod in ("tox", "nox"):
                    return (mod, mod)
            continue
        b = base(first)
        if b in ("pytest", "py.test"):
            return (py_env(first), "pytest")
        if b in ("tox", "nox"):
            return (b, b)
        if b in ("vitest", "jest", "mocha"):
            return ("node", b)
        if b in ("npx", "pnpm", "yarn", "npm", "bunx"):
            i = 1
            while i < len(w) and (w[i].startswith("-") or w[i] in ("exec", "run", "--")):
                if w[i] in ("--filter", "-F", "-C", "--dir"):
                    i += 1
                i += 1
            t = base(w[i]) if i < len(w) else ""
            if t in ("vitest", "jest", "mocha"):
                return ("node", t)
            if t == "test" or t.startswith("test:"):
                return ("node", "pkg-test")
        if b == "node" and len(w) > 1 and w[1] == "--test":
            return ("node", "node-test")
        if b == "go" and len(w) > 1 and w[1] == "test":
            return ("go", "go-test")
        if b == "cargo" and len(w) > 1 and w[1] == "test":
            return ("cargo", "cargo-test")
    return None


@dataclass
class Act:
    tool: str
    cmd: str
    is_error: bool
    run: tuple | None = None
    executed: bool = False
    env_sig: bool = False
    env_probe: bool = False
    discovery: bool = False

    @property
    def failed(self) -> bool:
        return self.is_error or self.env_sig

    @property
    def env_friction(self) -> bool:
        return self.env_sig or self.env_probe or (self.run is not None and not self.executed and self.is_error)


def act(r) -> Act:
    a = Act(tool=r.tool, cmd=r.cmd if r.tool == "Bash" else "", is_error=r.is_error)
    if r.tool == "Bash":
        a.env_sig = bool(ENV_SIG.search(r.out))
        inv = invocation(a.cmd)
        if inv:
            a.run = inv
            a.executed = bool(EXECUTED.search(r.out)) and not a.env_sig
        else:
            segs = segments(a.cmd)
            root_ls = any((w := words(s)) and w[0] == "ls" and all(x.startswith("-") or x in (".", "./") for x in w[1:]) for s in segs)
            a.env_probe = root_ls or bool(ENV_PROBE.search(a.cmd))
            first = next((words(s)[0] for s in segs if words(s) and words(s)[0] != "cd"), "")
            search = base(first) in ("find", "grep", "rg", "ls", "fd", "tree", "ag") or "git grep" in a.cmd or "git ls-files" in a.cmd
            a.discovery = search and bool(TESTISH.search(a.cmd))
    elif r.tool == "Read":
        a.env_probe = bool(MANIFEST_READ.search(r.cmd))
    elif r.tool in ("Grep", "Glob"):
        a.discovery = bool(TESTISH.search(r.cmd))
    return a


def dclass(cmd: str):
    """The replay's directive class of a `run <cmd>` directive."""
    inv = invocation(cmd)
    if inv:
        return ("runner", inv[1], inv[0])
    head = " ".join([w for w in words(cmd) if not w.startswith("{")][:2])
    return ("command", head) if head else ("unscored", "")


def sat(dc, a: Act, rule: str = "goal") -> bool:
    k, v = dc[0], dc[1]
    if k == "runner":
        # goal (calibrated on live U11): the runner executed tests in any environment;
        # strict: also in the directed interpreter (venv vs system)
        return a.run is not None and a.run[1] == v and a.executed and (rule == "goal" or a.run[0] == dc[2])
    if k == "command":
        return not a.failed and any(s == v or s.startswith(v + " ") for s in segments(a.cmd))
    return False


def waste(dc, a: Act) -> bool:
    return a.env_friction if dc[0] == "runner" else (a.failed if dc[0] == "command" else False)


def contradicts(dc, a: Act) -> bool:
    return dc[0] == "runner" and a.run is not None and a.run[1] != dc[1] and a.executed


def own(dc, a: Act) -> bool:
    return dc[0] == "runner" and a.run is not None and a.run[1] == dc[1] and a.executed


COST = 0.02


@dataclass
class Scored:
    t: int
    dc: tuple
    pre: bool
    useful: bool = False
    harmful: bool = False
    superseded: bool = False
    saved: float = 0.0
    cost: float = 0.0


def score(given: list[tuple[int, tuple, bool]], acts: list[Act], passed, rule: str = "goal") -> list[Scored]:
    """given: (t, directive class, pre-emptive) in order. The replay's `score`."""
    n, out, windows = len(acts), [], []
    for t, dc, pre in given:
        frm = max(t + 1, 0)
        ts = next((k for k in range(frm, n) if sat(dc, acts[k], rule)), None)
        sup = any(a <= t < b for a, b in windows)
        s = Scored(t=t, dc=dc, pre=pre, useful=ts is not None, superseded=sup)
        s.harmful = (not s.useful) and passed is True and any(contradicts(dc, a) for a in acts[frm:]) and not any(own(dc, a) for a in acts[frm:])
        if ts is not None and not sup:
            s.saved = sum(1 for a in acts[frm:ts] if waste(dc, a))
            if pre and t >= 0 and not sat(dc, acts[t], rule) and acts[t].failed:
                s.saved += 1
            windows.append((t, ts))
        s.cost = 0.0 if sup else COST
        out.append(s)
    return out
