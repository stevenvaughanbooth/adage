"""Normalise one tool call into an action at three levels.

- `key`: the command level. `Read`, `Edit`, `Grep`, `Bash:grep`, `Bash:.venv/bin/python -m pytest`,
  `Bash:npm run test`, `Bash:go test`.
- `template`: the key plus typed argument slots, e.g. `Bash:.venv/bin/python -m pytest {test_file}`,
  `Read {src_file}`. This is the shape Adage's typed directives take: the slot is filled from a
  learned value type, never from free text.
- `full`: the key plus the concrete normalised arguments, e.g. `Read src/click/core.py`,
  `Bash:.venv/bin/python -m pytest tests/test_options.py`.

Run-specific sandbox roots (`/tmp/adage-*/runs/<repo>/runs/<run>/repo`) are stripped so paths are
repository-relative and comparable across sessions. The shell is split with `shlex` on the first
real stage (after `cd … &&`, env assignments, and before pipes/redirections); this is a small
subset of what `adage-core`'s tree-sitter normaliser does, sufficient for these traces.
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass

ROOT = re.compile(r"/tmp/adage-[\w.-]+/runs/[\w.-]+/(?:private/)?(?:runs/[\w.-]+/)?(?:repo|bind-sandbox)")
EXPLORE_BASH = {"ls", "find", "cat", "grep", "rg", "head", "tail", "tree", "sed", "wc", "awk", "file", "stat",
                "echo", "pwd", "which", "diff", "git-log", "git-show", "git-diff", "git-status", "git-grep"}
EXPLORE_TOOLS = {"Read", "Grep", "Glob", "LS", "NotebookRead"}
EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
DESTRUCTIVE = re.compile(r"^Bash:(rm|rmdir|mv|dd|chmod|chown|sudo|kill|pkill|truncate|shred|"
                         r"git (reset|checkout|clean|push|rebase|stash|restore|branch|switch|rm))\b")
SUBCMD = {"npm", "pnpm", "yarn", "npx", "go", "git", "cargo", "uv", "pip", "pip3", "make", "tox", "nox", "hatch",
          "poetry", "pre-commit", "vitest", "jest", "tsc", "node", "bun", "pnpx"}
TWO_WORD = {("npm", "run"), ("pnpm", "run"), ("yarn", "run"), ("uv", "run"), ("npm", "exec"), ("pnpm", "exec"),
            ("poetry", "run"), ("hatch", "run")}


@dataclass(frozen=True)
class Action:
    tool: str
    key: str
    template: str
    full: str
    explore: bool
    edit: bool
    text: str  # a short readable rendering for embedding

    @property
    def directive_eligible(self) -> bool:
        """Could this be a directive at all? Commands that change or check something, except
        destructive or history-rewriting ones (never suggested, whatever the traces did);
        exploratory shell (grep/find/cat…) is not (its pattern is free text)."""
        if self.tool == "Bash":
            return not self.explore and not DESTRUCTIVE.match(self.key)
        return self.tool in ("Read", "Edit", "Write", "MultiEdit")

    @property
    def directive_key(self) -> str:
        """The level a directive is matched at: Bash commands by template (the agent fills the
        slot), file tools by the concrete file (a directive to "read {src_file}" says nothing).
        Flags and free-text arguments are the agent's choice, not part of the directive:
        `python -m pytest -v tests/x.py` follows "run `python -m pytest {test_file}`"."""
        if self.tool != "Bash":
            return self.full
        parts = self.template.split(" ")
        n = len(self.key.split(" "))
        slots = [p for p in parts[n:] if p.startswith("{") and p not in ("{arg}", "{num}")]
        return " ".join(parts[:n] + slots)


def strip_root(s: str) -> str:
    s = ROOT.sub(".", s)
    s = re.sub(r"(?<![\w.])\./", "", s)
    return s


def slot(arg: str) -> str:
    a = arg.strip("'\"")
    if re.fullmatch(r"-?\d+(\.\d+)?", a):
        return "{num}"
    if re.search(r"(^|/)(tests?|__tests__|spec)(/|$)|(^|/)test_[\w]+\.py|_test\.go$|\.(spec|test)\.[jt]sx?$", a):
        return "{test_file}" if re.search(r"\.\w+(::.*)?$", a) else "{test_dir}"
    if re.search(r"\.(py|go|ts|tsx|js|jsx|vue|rs|json|toml|cfg|ini|md|rst|yaml|yml|txt|mod|sum)$", a):
        return "{src_file}"
    if "/" in a or a in (".", ".."):
        return "{dir}"
    return "{arg}"


def _head(tok: str) -> str:
    if tok.startswith((".venv/", "venv/", "node_modules/.bin/", "./")):
        return tok.removeprefix("./")
    base = tok.rsplit("/", 1)[-1]
    return re.sub(r"^python3\.\d+$", "python3", base)


def _segments(cmd: str) -> list[str]:
    # A heredoc body or a multi-line `-c` script is the argument, not more commands.
    first_line, _, body = cmd.partition("\n")
    if "<<" in first_line:
        cmd = first_line.replace("<<", " HEREDOC ").split("HEREDOC")[0] + " <<heredoc"
    elif body and first_line.count('"') % 2 == 0 and first_line.count("'") % 2 == 0:
        cmd = cmd.replace("\n", " ; ")
    parts = re.split(r"\s*(?:&&|\|\||;)\s*", cmd)
    out = []
    for p in parts:
        p = p.strip()
        if not p or re.match(r"^(?:(?:cd|export|set|source|pushd|popd)\b|\.\s)", p):
            continue
        out.append(p)
    return out


def normalize_bash(cmd: str) -> tuple[str, str, str, bool]:
    cmd = strip_root(cmd)
    segs = _segments(cmd)
    if not segs:
        return "Bash:cd", "Bash:cd", "Bash:cd", True
    first = re.split(r"\s\|\s?|\|(?!\|)", segs[0])[0]
    heredoc = first.endswith("<<heredoc")
    first = first.removesuffix("<<heredoc")
    first = re.sub(r"\d?>&?\d?\s*\S+|2>&1|<\s*\S+", " ", first) if ">" in first or "<" in first else first
    try:
        toks = shlex.split(first, posix=True)
    except ValueError:
        toks = first.split()
    while toks and re.match(r"^[A-Z_][A-Z0-9_]*=", toks[0]):
        toks = toks[1:]
    if toks and toks[0] in ("timeout",):
        toks = toks[2:] if len(toks) > 2 and re.match(r"\d", toks[1]) else toks[1:]
    if not toks:
        return "Bash:?", "Bash:?", "Bash:?", False
    head = _head(toks[0])
    rest = toks[1:]
    key = [head]
    if re.search(r"(^|/)python3?$", head) or head.endswith("/python"):
        if rest[:1] == ["-m"] and len(rest) > 1:
            key += ["-m", rest[1]]
            rest = rest[2:]
        elif heredoc and not rest:
            key += ["<<heredoc"]
        elif rest[:1] == ["-c"]:
            key += ["-c"]
            rest = []
    elif head in SUBCMD:
        nf = [t for t in rest if not t.startswith("-")]
        if nf:
            key.append(nf[0])
            rest = rest[rest.index(nf[0]) + 1:]
            if (head, nf[0]) in TWO_WORD and rest:
                nf2 = [t for t in rest if not t.startswith("-")]
                if nf2:
                    key.append(nf2[0])
                    rest = rest[rest.index(nf2[0]) + 1:]
    explore = head in EXPLORE_BASH or (head == "git" and len(key) > 1 and f"git-{key[1]}" in EXPLORE_BASH)
    k = "Bash:" + " ".join(key)
    args = [t for t in rest if not t.startswith("-")]
    flags = sorted({t.split("=")[0] for t in rest if t.startswith("-")} & {"-x", "-q", "-v", "--run", "-run"})
    slots = []
    for a in args:
        s = slot(a)
        if s not in slots:
            slots.append(s)
    template = " ".join([k] + flags + slots)
    full_args = [re.sub(r"::.*$", "", a) if slot(a) != "{arg}" else "{arg}" for a in args]
    full = " ".join([k] + flags + full_args)
    return k, template, full, explore


def normalize_tool(name: str, inp: dict) -> Action:
    if name == "Bash":
        cmd = inp.get("command", "")
        k, t, f, ex = normalize_bash(cmd)
        text = strip_root(cmd)[:160]
        return Action("Bash", k, t, f, ex, False, f"run {text}")
    if name in ("Read", "Edit", "Write", "MultiEdit", "NotebookEdit", "NotebookRead"):
        p = strip_root(inp.get("file_path") or inp.get("notebook_path") or "")
        s = slot(p)
        return Action(name, name, f"{name} {s}", f"{name} {p}", name in EXPLORE_TOOLS, name in EDIT_TOOLS,
                      f"{name.lower()} {p}")
    if name in ("Grep", "Glob"):
        pat = inp.get("pattern", "")
        path = strip_root(inp.get("path", "") or "")
        return Action(name, name, f"{name} {{pattern}}", f"{name} {pat} {path}".strip(), True, False,
                      f"{name.lower()} {pat} {path}"[:160])
    return Action(name, name, name, name, name in EXPLORE_TOOLS, name in EDIT_TOOLS, name.lower())
