"""THE HAND-CODED INSTRUMENT: a Python port of `crates/adage-eval/src/savings_replay.rs`'s action
features (ENV_SIG, EXECUTED, ENV_PROBE, `invocation`), and of `adage-learn/src/outcome.rs`'s
`executed_tests`/`env_signature`.

Used for two things only, as the brief allows:
  1. the **baseline to beat** (the hand-written outcome signal: `failed = is_error or env_sig`,
     `success = executed`), and
  2. the **measurement instrument** of the downstream replay (Sat(d), W(d)), so the learned
     policy is scored by exactly the rules the calibrated Rust replay uses.
Nothing in the learned model reads these.
"""

from __future__ import annotations

import re

ENV_SIG = re.compile(
    r"No module named|ModuleNotFoundError|command not found|: not found\b|Cannot find module|ERR_MODULE_NOT_FOUND|"
    r"executable file not found|is not recognized as|ImportError while loading conftest|"
    r"(python3?|pytest|/bin/\S+|tox|nox|vitest|jest|npx): No such file or directory")
EXECUTED = re.compile(
    r"(?m)collected [1-9]\d* items?|\b\d+ passed\b|\b\d+ failed\b|^\s*Tests:\s+\d|Test Files\s+\d|\bRan [1-9]\d* tests?\b|"
    r"^ok\s+\S+|^--- (PASS|FAIL)|^(PASS|FAIL)\s+\S|test result: (ok|FAILED)|^# tests [1-9]")
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
U11_SIG = re.compile(r"No module named pytest|pytest: command not found|command not found: pytest|"
                     r"No module named '(click|attr|attrs|marshmallow)'|python: command not found")
U11_TEST = re.compile(r"\b(pytest|py\.test|tox|nox)\b")


def segments(cmd: str) -> list[str]:
    out, cur, sq, dq, i = [], "", False, False, 0
    while i < len(cmd):
        c = cmd[i]
        if c == "'" and not dq:
            sq = not sq
        elif c == '"' and not sq:
            dq = not dq
        if not sq and not dq and c in ";|&\n":
            if cur.strip():
                out.append(cur.strip())
            cur = ""
            while i + 1 < len(cmd) and cmd[i + 1] in "&|":
                i += 1
        else:
            cur += c
        i += 1
    if cur.strip():
        out.append(cur.strip())
    return out


def words(seg: str) -> list[str]:
    return [w.strip("\"'") for w in seg.split()]


def base(w: str) -> str:
    return w.rsplit("/", 1)[-1]


def is_python(w: str) -> bool:
    b = base(w)
    return b in ("python", "python3") or bool(re.fullmatch(r"python3\.\d+", b))


def in_venv(w: str) -> bool:
    return ".venv/bin/" in w or w.startswith("venv/bin/") or "/venv/bin/" in w


def invocation(cmd: str):
    """(interp, runner) of the test runner a command invokes, or None (port of the Rust)."""
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
        py_env = "venv" if (in_venv(first) or active) else "system"
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
                nxt = w[m + 1] if m + 1 < len(w) else None
                if nxt in ("pytest", "py.test"):
                    return (py_env, "pytest")
                if nxt == "unittest":
                    return (py_env, "unittest")
                if nxt in ("tox", "nox"):
                    return (nxt, nxt)
            continue
        b = base(first)
        if b in ("pytest", "py.test"):
            return (py_env, "pytest")
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
            t = base(w[i]) if i < len(w) else None
            if t in ("vitest", "jest", "mocha"):
                return ("node", t)
            if t and (t == "test" or t.startswith("test:")):
                return ("node", "pkg-test")
        if b == "node" and len(w) > 1 and w[1] == "--test":
            return ("node", "node-test")
        if b == "go" and len(w) > 1 and w[1] == "test":
            return ("go", "go-test")
        if b == "cargo" and len(w) > 1 and w[1] == "test":
            return ("cargo", "cargo-test")
    return None


def act(c: dict) -> dict:
    """The replay's features of one call (`savings_replay::act`)."""
    tool, out = c["tool"], c["output"]
    a = {"tool": tool, "cmd": "", "is_error": c["is_error"], "run": None, "executed": False, "env_sig": False,
         "env_probe": False, "output": out}
    if tool == "Bash":
        cmd = c["arg"]
        a["cmd"] = cmd
        a["env_sig"] = bool(ENV_SIG.search(out))
        inv = invocation(cmd)
        if inv:
            a["run"] = inv
            a["executed"] = bool(EXECUTED.search(out)) and not a["env_sig"]
        else:
            root_ls = any(words(s)[:1] == ["ls"] and all(x.startswith("-") or x in (".", "./") for x in words(s)[1:])
                          for s in segments(cmd))
            a["env_probe"] = root_ls or bool(ENV_PROBE.search(cmd))
    elif tool == "Read":
        a["env_probe"] = bool(MANIFEST_READ.search(c["arg"]))
    return a


def failed(a: dict) -> bool:
    return a["is_error"] or a["env_sig"]


def env_friction(a: dict) -> bool:
    return a["env_sig"] or a["env_probe"] or (a["run"] is not None and not a["executed"] and a["is_error"])


def u11_dead_end(a: dict) -> bool:
    return a["tool"] == "Bash" and bool(U11_TEST.search(a["cmd"])) and bool(U11_SIG.search(a.get("output", "")))
