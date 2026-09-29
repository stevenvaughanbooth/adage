#!/usr/bin/env python3
"""Skill -> procedure graph: a typed, reviewable structure parsed from a SKILL.md, deterministically.

  python3 procedure.py <SKILL.md> [--json] [--out graph.json]

A procedure graph is the compile-time view of a skill. Each step has a role (lookup, read,
search, edit, regenerate, verify, run, install, vcs, remove), the commands it directs as templates
with typed slots, the commands it forbids (with their failure signature), a precondition (always,
after an edit to a glob, before finishing, or a condition only an agent can judge), and what it
expects to observe. Every step also says whether it can compile to a reflex and, if not, why.

Nothing here calls a model. `llm_structure()` (optional, `--llm`) makes one tool-less `claude -p`
call at compile time and returns the same schema; its output is validated by `validate()` and
merged as proposals, never trusted as is. The runtime never sees skill text.
"""

from __future__ import annotations

import hashlib
import json
import re
import shlex
import sys
from dataclasses import asdict, dataclass, field
from pathlib import Path

SCHEMA = "adage-procedure/0"

# ---------------------------------------------------------------------------------- model


@dataclass
class Slot:
    name: str
    type: str  # work-item-id | repo-path | module | symbol | string
    pattern: str  # anchored regex the value must match
    source: str  # prompt | edit | context | judgment
    example: str = ""


@dataclass
class Command:
    template: str  # "scripts/wi-lookup {id}"
    role: str
    env: str  # make | project-venv | system | repo-script | tool | env-prefixed
    scope: str  # repo | suite | file | test | item | path | none
    slots: list[str] = field(default_factory=list)


@dataclass
class Avoid:
    command: str
    instead: str
    signature: str = ""


@dataclass
class Precondition:
    kind: str  # always | after_edit | before_finish | judgment
    glob: str = ""
    text: str = ""


@dataclass
class Expect:
    exit: str = ""  # zero | nonzero | ""
    contains: list[str] = field(default_factory=list)
    text: str = ""


@dataclass
class Step:
    id: str
    order: int
    title: str
    role: str
    text: str
    commands: list[Command] = field(default_factory=list)
    avoid: list[Avoid] = field(default_factory=list)
    precondition: Precondition = field(default_factory=lambda: Precondition("always"))
    expect: Expect = field(default_factory=Expect)
    compilable: bool = False
    why_not: str = ""


@dataclass
class Graph:
    schema: str
    skill: dict
    trigger: dict
    slots: dict
    steps: list[Step]
    edges: list[dict]
    advice: list[str]

    def to_json(self) -> dict:
        return asdict(self)


# ---------------------------------------------------------------------------- classification

KNOWN_PROGRAMS = {
    "make", "python", "python3", "pytest", "pip", "pip3", "uv", "npm", "npx", "pnpm", "yarn", "node", "go", "cargo",
    "git", "grep", "rg", "find", "ls", "cat", "sed", "head", "tail", "tox", "nox", "rm", "gh", "bash", "sh",
}
VERIFY = ("make test", "make ci", "make check", "make lint", "pytest", "unittest", "tox", "nox", "npm test",
          "npm run test", "pnpm test", "go test", "cargo test", "py_compile")
REGEN = ("make gen", "generate", "codegen", "go generate")


def split_env(argv: list[str]) -> tuple[list[str], bool]:
    i = 0
    while i < len(argv) and re.match(r"^[A-Z_][A-Z0-9_]*=", argv[i]):
        i += 1
    return argv[i:], i > 0


def argv_of(cmd: str) -> list[str]:
    try:
        return shlex.split(cmd)
    except ValueError:
        return cmd.split()


def is_command(code: str) -> bool:
    """Whether an inline code span or a shell line is a command (not a path, flag or identifier)."""
    a = argv_of(code.replace("<", "{").replace(">", "}"))
    if not a:
        return False
    a, _ = split_env(a)
    if not a:
        return False
    p = a[0]
    if p in KNOWN_PROGRAMS or p.startswith(".venv/bin/") or p.startswith("./"):
        return True
    # A repository script with arguments or in a scripts/ bin/ tools/ directory.
    return bool(re.match(r"^(scripts|bin|tools)/[\w.-]+$", p))


def role_of(cmd: str) -> str:
    c = " " + cmd + " "
    a, _ = split_env(argv_of(cmd))
    p = a[0] if a else ""
    if p in ("grep", "rg", "find", "ls"):
        return "search"
    if p in ("cat", "head", "tail") or (p == "sed" and "-n" in a):
        return "read"
    if p == "git":
        return "vcs"
    if "lookup" in c or " --help" in c or re.search(r"\bgh (issue|pr) view\b", c):
        return "lookup"
    inline = p.startswith(("python", ".venv/bin/python")) and "-c" in a
    if (p == "make" and len(a) > 1 and a[1].startswith("gen")) or "generate" in a or "codegen" in a \
            or (not inline and any(re.search(r"(^|/)gen(erate)?\.py$", x) for x in a[1:2])):
        return "regenerate"
    if not inline and any(k in c for k in VERIFY):
        return "verify"
    if p in ("pip", "pip3", "npm", "pnpm", "yarn") and "install" in a or (p == "uv" and "sync" in a):
        return "install"
    if p in ("grep", "rg", "find", "ls"):
        return "search"
    if p in ("cat", "head", "tail") or (p == "sed" and "-n" in a):
        return "read"
    if p == "git":
        return "vcs"
    if p == "rm":
        return "remove"
    if p in ("sed", "perl") and "-i" in " ".join(a):
        return "edit"
    return "run"


def env_of(cmd: str) -> str:
    a, prefixed = split_env(argv_of(cmd))
    p = a[0] if a else ""
    if prefixed:
        return "env-prefixed"
    if p == "make":
        return "make"
    if p.startswith(".venv/bin/") or (p == "uv" and a[1:2] == ["run"]) or "activate" in cmd:
        return "project-venv"
    if p in ("python", "python3", "pytest", "pip", "pip3"):
        return "system"
    if "/" in p or (p in ("python", "python3") and len(a) > 1 and a[1].endswith(".py")):
        return "repo-script"
    return "tool"


def scope_of(cmd: str) -> str:
    a, _ = split_env(argv_of(cmd))
    args = [x for x in a[1:] if not x.startswith("-")]
    if any("::" in x for x in a) or "-k" in a:
        return "test"
    if any(re.match(r"^\{?[A-Z]+-\d+\}?$|^\{id\}$", x) for x in args):
        return "item"
    if any(re.search(r"(^|/)test_[^/]*\.py$|\{module\}", x) for x in args):
        return "file"
    if any(x.rstrip("/") in ("tests", "test") for x in args):
        return "suite"
    if any("/" in x or x.endswith(".py") for x in args if x not in ("-m",)):
        return "path"
    return "repo"


# ------------------------------------------------------------------------------------ slots

PLACEHOLDER = re.compile(r"<([A-Za-z_][\w-]*)>|\{([a-z_][\w]*)\}")


def slot_for(name: str, example: str = "") -> Slot:
    n = name.lower().replace("-", "_")
    if n in ("id", "item", "ticket"):
        pat = r"^[A-Z]{1,5}-\d{1,9}$"
        if example and re.match(r"^[A-Z]+-\d+$", example):
            letters, digits = example.split("-")
            pat = rf"^{letters}-\d{{{len(digits)}}}$"
        return Slot("id", "work-item-id", pat, "prompt", example)
    if n in ("module", "record"):
        return Slot(n, "module", r"^[a-z_][a-z0-9_]{0,63}$", "edit", example)
    if n in ("path", "file"):
        return Slot(n, "repo-path", r"^[A-Za-z0-9_][A-Za-z0-9_./-]{0,199}$", "edit", example)
    if n in ("symbol", "name", "x"):
        return Slot(n, "symbol", r"^[A-Za-z_][A-Za-z0-9_]{0,63}$", "judgment", example)
    return Slot(n, "string", r"^[A-Za-z0-9_.-]{1,64}$", "judgment", example)


def templatize(code: str) -> tuple[str, list[str]]:
    names = []

    def sub(m):
        n = (m.group(1) or m.group(2)).lower()
        n = "id" if n in ("id", "item", "ticket") else n
        names.append(n)
        return "{" + n + "}"

    return PLACEHOLDER.sub(sub, code), names


# -------------------------------------------------------------------------------- markdown

FENCE = re.compile(r"^```(\w*)\s*$")
NUMBERED = re.compile(r"^(\d+)\.\s+(.*)$")
INLINE = re.compile(r"`([^`]+)`")
SENTENCE = re.compile(r"(?<=[.!?])\s+(?=[A-Z*`])")


def front_matter(text: str) -> tuple[dict, str]:
    meta = {}
    if text.startswith("---\n"):
        end = text.find("\n---", 4)
        if end > 0:
            for line in text[4:end].splitlines():
                if ":" in line:
                    k, v = line.split(":", 1)
                    meta[k.strip()] = v.strip()
            text = text[end + 4:]
    return meta, text


def blocks(body: str) -> list[dict]:
    """Numbered items (with their continuation lines and fences), headings, paragraphs and fences."""
    out, cur, fence, heading, fence_blk = [], None, None, "", None
    for line in body.splitlines():
        s = line.strip()
        if fence is not None:
            if FENCE.match(s) and s == "```":
                (cur if cur and cur["kind"] == "item" else fence_blk)["fences"].append(fence)
                if not (cur and cur["kind"] == "item"):
                    out.append(fence_blk)
                fence = None
            else:
                fence["lines"].append(line)
            continue
        m = FENCE.match(s)
        if m:
            fence = {"lang": m.group(1), "lines": []}
            fence_blk = {"kind": "fence", "text": "", "fences": [], "heading": heading}
            continue
        if s.startswith("#"):
            heading = s.lstrip("#").strip()
            out.append({"kind": "heading", "text": heading, "fences": []})
            cur = None
            continue
        n = NUMBERED.match(s) if not line.startswith((" ", "\t")) else None
        if n:
            cur = {"kind": "item", "n": int(n.group(1)), "text": n.group(2), "fences": [], "heading": heading}
            out.append(cur)
        elif s == "":
            if cur and cur["kind"] != "item":
                cur = None
        elif cur and (line.startswith((" ", "\t")) or cur["kind"] == "para"):
            cur["text"] += " " + s
        else:
            cur = {"kind": "para", "text": s, "fences": [], "heading": heading}
            out.append(cur)
    return out


def shell_lines(fences: list[dict]) -> list[str]:
    cmds = []
    for f in fences:
        if f["lang"] not in ("", "bash", "sh", "shell", "console"):
            continue
        joined = "\n".join(f["lines"]).replace("\\\n", " ")
        for ln in joined.splitlines():
            ln = ln.strip().lstrip("$ ").strip()
            if ln and not ln.startswith("#") and is_command(ln):
                cmds.append(re.sub(r"\s+", " ", ln))
    return cmds


def tree_commands(fences: list[dict]) -> list[str]:
    """Commands named inside a plain-text decision tree ("Run: python x.py --help")."""
    out = []
    for f in fences:
        for ln in f["lines"]:
            m = re.search(r"\bRun:\s*(.+)$", ln)
            if m and is_command(m.group(1).strip()):
                out.append(m.group(1).strip())
    return out


# -------------------------------------------------------------------------------- parsing

IMPERATIVE = re.compile(r"\b(run|use|always|never|after|before|instead|install|invoke|execute|call|start|search|"
                        r"look up|check)\b", re.I)
MENTION = re.compile(r"\b(fails?|if you skipped|it prints|is generated)\b", re.I)
EXAMPLE = re.compile(r"\b(for example|e\.g\.|such as)\b", re.I)
AVOID_WORDS = re.compile(r"\b(never|do not|don't|instead of|avoid)\b", re.I)
AFTER_EDIT = re.compile(r"\b(after|whenever|when you)\s+(editing|changing|you (?:edit|change)|modifying)\s+`([^`]+)`",
                        re.I)
FINISH = re.compile(r"\bbefore (you )?(finish|finishing|commit|committing|you are done)\b", re.I)
JUDGE = re.compile(r"^\**\s*if\b|\bif the (item|report|task)\b", re.I)
EXPECT = re.compile(r"\bexpect(?:s)?\s+(?:`([^`]+)`|(.+?)(?:\.|$))", re.I)
FAILS_WITH = re.compile(r"\bfails? with\s+`([^`]+)`", re.I)


def title_of(text: str) -> str:
    m = re.match(r"\*\*(.+?)\*\*", text)
    return (m.group(1) if m else SENTENCE.split(text)[0]).strip().rstrip(".")


def commands_in(s: str) -> list[str]:
    """Commands an imperative sentence directs. A mention ("fails if you skipped `make gen`") is not
    one; in an example ("for example `grep -rn "x" src`") a quoted literal is a slot the agent fills."""
    if not IMPERATIVE.search(s) or MENTION.search(s):
        return []
    out = []
    for c in INLINE.findall(s):
        if not is_command(c):
            continue
        if EXAMPLE.search(s) and re.search(r"\"[^\"]+\"|'[^']+'", c):
            c = re.sub(r"\"[^\"]+\"|'[^']+'", "<symbol>", c)
        out.append(c)
    return out


def directs(text: str) -> list[str]:
    return [c for s in SENTENCE.split(text.replace("**", "")) for c in commands_in(s)]


def parse_step(idx: int, text: str, fences: list[dict]) -> Step:
    sentences = SENTENCE.split(text.replace("**", ""))
    cmds: list[str] = []
    avoid_cmds: list[str] = []
    generic: list[str] = []
    for s in sentences:
        codes = commands_in(s)
        if AVOID_WORDS.search(s):
            # "Never run X or Y directly instead of Z": the commands before "instead" are avoided.
            parts = re.split(r"\binstead of\b|\binstead\b", s, maxsplit=1, flags=re.I)
            avoided = [c for c in INLINE.findall(parts[0]) if is_command(c)]
            instead = [c for c in INLINE.findall(parts[1]) if is_command(c)] if len(parts) > 1 else []
            if avoided:
                avoid_cmds += avoided
                generic += instead
                continue
        cmds += codes
    cmds += shell_lines(fences) + tree_commands(fences)
    # bare program names in an avoid list ("or `pytest` directly") count as commands too
    templates, seen = [], set()
    slot_names: list[str] = []
    examples: dict[str, str] = {}
    for c in cmds:
        t, names = templatize(c)
        if not names:
            # a concrete example of an existing template: `scripts/wi-lookup W-2042`
            for prev in templates:
                pat = re.escape(prev).replace(r"\{id\}", r"([A-Z]+-\d+)")
                m = re.fullmatch(pat, c)
                if m:
                    examples["id"] = m.group(1)
                    break
            else:
                if t not in seen:
                    seen.add(t)
                    templates.append(t)
            continue
        slot_names += names
        if t not in seen:
            seen.add(t)
            templates.append(t)
    # A concrete command that instantiates a template is an example, not a second command.
    concrete = [t for t in templates if "{" not in t]
    for t in [t for t in templates if "{" in t]:
        pat = re.escape(t)
        for n in re.findall(r"\\\{(\w+)\\\}", pat):
            pat = pat.replace(r"\{" + n + r"\}", r"(?P<" + n + r">[\w.-]+)")
        for c in concrete:
            m = re.fullmatch(pat, c)
            if m:
                examples.update(m.groupdict())
                templates.remove(c)
    commands = [Command(t, role_of(t), env_of(t), scope_of(t), sorted(set(re.findall(r"\{(\w+)\}", t))))
                for t in templates]
    pre = Precondition("always")
    m = AFTER_EDIT.search(text)
    if m:
        pre = Precondition("after_edit", glob=m.group(3))
    elif FINISH.search(text):
        pre = Precondition("before_finish", text=FINISH.search(text).group(0))
    elif JUDGE.search(text):
        pre = Precondition("judgment", text=title_of(text))
    exp = Expect()
    sig = ""
    for s in sentences:
        f = FAILS_WITH.search(s)
        if f:
            sig = sig or f.group(1)
        e = EXPECT.search(s)
        if e:
            if e.group(1):
                exp.contains.append(e.group(1))
                exp.exit = "zero"
            else:
                exp.text = e.group(2).strip()
    # Drop a command that is a prefix of a more specific one of the step (the generic "instead").
    commands = [c for c in commands if not any(o.template != c.template and o.template.startswith(c.template + " ")
                                               for o in commands)]
    slot_free = [g for g in generic if "<" not in g] + [c.template for c in commands if not c.slots]
    instead = next(iter(slot_free), commands[0].template if commands else "")
    avoid = [Avoid(a, instead, sig) for a in dict.fromkeys(avoid_cmds)]
    # The step's role: its main command's, else read from the words.
    words = text.lower()
    if commands:
        main = [c for c in commands if c.role != "lookup" or "--help" not in c.template] or commands
        role = main[0].role
        if pre.kind == "before_finish" and role == "verify":
            role = "verify"
    elif re.search(r"\b(edit|change|fix|add)\b", words):
        role = "edit"
    else:
        role = "note"
    if role in ("edit",) and any(c.role == "run" for c in commands):
        role = "run"
    step = Step(f"s{idx}", idx, title_of(text), role, re.sub(r"\s+", " ", text).strip(), commands, avoid, pre, exp)
    step._slot_names = slot_names  # type: ignore[attr-defined]
    step._examples = examples  # type: ignore[attr-defined]
    return step


def compilability(s: Step, slots: dict) -> None:
    """Whether a step can become a reflex, and why not. The shapes are Adage's (docs/packs.md)."""
    if not s.commands and not s.avoid:
        s.compilable, s.why_not = False, "no command: the step is the agent's judgment (an edit or a decision)"
        return
    plain = [c for c in s.commands if not re.search(r"[;|&`$()<>\"']", re.sub(r"\{\w+\}", "", c.template))]
    if s.commands and not plain and not s.avoid:
        s.compilable, s.why_not = False, "not one plain command (quoted shell in its arguments)"
        return
    bad = [c for c in s.commands if c.role in ("remove", "install", "vcs")]
    if bad and len(bad) == len(s.commands) and not s.avoid:
        s.compilable, s.why_not = False, f"{bad[0].role} commands are never directed (effect ceiling)"
        return
    judged = [n for c in s.commands for n in c.slots if slots.get(n, {}).get("source") == "judgment"]
    if judged and not s.avoid:
        s.compilable, s.why_not = False, f"slot {{{judged[0]}}} is chosen by the agent (no stimulus binds it)"
        return
    if s.precondition.kind == "judgment" and not s.commands and not s.avoid:
        s.compilable, s.why_not = False, "its condition needs judgment"
        return
    s.compilable = True


def parse(path: str | Path) -> Graph:
    raw = Path(path).read_text()
    meta, body = front_matter(raw)
    bl = blocks(body)
    items = [b for b in bl if b["kind"] == "item"]
    steps: list[Step] = []
    advice: list[str] = []
    if any(directs(b["text"]) or shell_lines(b["fences"]) for b in items):
        # A numbered procedure: its items are the steps; paragraphs elsewhere are advice.
        for i, b in enumerate(items, 1):
            steps.append(parse_step(i, b["text"], b["fences"]))
        for b in bl:
            if b["kind"] == "para" and not b["text"].startswith("#"):
                if directs(b["text"]):
                    steps.append(parse_step(len(steps) + 1, b["text"], b["fences"]))
                else:
                    advice.append(b["text"])
    else:
        # No numbered procedure: every block that directs a command is a step, in document order.
        i = 0
        for b in bl:
            if b["kind"] in ("para", "fence", "item"):
                has = directs(b["text"]) or shell_lines(b["fences"]) \
                    or tree_commands(b["fences"])
                if has:
                    i += 1
                    steps.append(parse_step(i, b["text"] or (b.get("heading") or "commands"), b["fences"]))
                elif b["text"]:
                    advice.append(b["text"])
                elif b["fences"]:
                    advice.append(f"[{b['fences'][0]['lang'] or 'text'} block under '{b.get('heading', '')}': "
                                  f"{len(b['fences'][0]['lines'])} lines of code to imitate]")
    slots: dict[str, dict] = {}
    for s in steps:
        for n in getattr(s, "_slot_names", []):
            ex = getattr(s, "_examples", {}).get(n, "")
            if n not in slots or (ex and not slots[n]["example"]):
                slots[n] = asdict(slot_for(n, ex))
        # an example in the prose ("such as W-2042") fills an id slot
    for m in re.finditer(r"\b([A-Z]{1,5}-\d{3,9})\b", body):
        if "id" in slots and not slots["id"]["example"]:
            slots["id"] = asdict(slot_for("id", m.group(1)))
    for s in steps:
        for a in ("_slot_names", "_examples"):
            if hasattr(s, a):
                delattr(s, a)
        compilability(s, slots)
    edges = []
    for a, b in zip(steps, steps[1:]):
        edges.append({"from": a.id, "to": b.id, "kind": "next"})
        if b.precondition.kind in ("judgment", "after_edit"):
            nxt = steps[steps.index(b) + 1] if steps.index(b) + 1 < len(steps) else None
            if nxt:
                edges.append({"from": a.id, "to": nxt.id, "kind": "skip-if-not", "when": b.precondition.kind})
    desc = meta.get("description", "")
    kw = sorted({w for w in re.findall(r"[a-z][a-z-]{3,}", (desc + " " + meta.get("name", "")).lower())
                 if w not in STOP})
    trigger = {"description": desc, "keywords": kw,
               "spans": ["ticket"] if "id" in slots and slots["id"]["source"] == "prompt" else []}
    skill = {"name": meta.get("name", Path(path).parent.name), "source": str(path),
             "sha256": hashlib.sha256(raw.encode()).hexdigest(), "bytes": len(raw.encode()),
             "approx_tokens": approx_tokens(raw)}
    return Graph(SCHEMA, skill, trigger, slots, steps, edges, advice)


STOP = {"when", "with", "that", "this", "your", "from", "into", "such", "used", "uses", "work", "about", "code",
        "using", "local", "supports"}


def approx_tokens(text: str) -> int:
    """~4 characters per token (English prose and code); good enough for the relative cost argument."""
    return round(len(text) / 4)


# ------------------------------------------------------------------------------ validation


ROLES = {"lookup", "read", "search", "edit", "regenerate", "verify", "run", "install", "vcs", "remove", "note"}


def validate(g: dict) -> list[str]:
    """Schema checks for a graph from any source (the parser, an LLM, a person's edit)."""
    errs = []
    if g.get("schema") != SCHEMA:
        errs.append("schema")
    for s in g.get("steps", []):
        if s.get("role") not in ROLES:
            errs.append(f"{s.get('id')}: role {s.get('role')!r}")
        for c in s.get("commands", []):
            t = c.get("template", "")
            if not is_command(t.replace("{", "<").replace("}", ">")):
                errs.append(f"{s.get('id')}: not a command {t!r}")
            if re.search(r"[;|&`$()<>]", re.sub(r"\{\w+\}", "", t)) and s.get("compilable"):
                errs.append(f"{s.get('id')}: shell syntax in compilable {t!r}")
            for n in re.findall(r"\{(\w+)\}", t):
                if n not in g.get("slots", {}):
                    errs.append(f"{s.get('id')}: undeclared slot {n}")
        pk = (s.get("precondition") or {}).get("kind")
        if pk not in ("always", "after_edit", "before_finish", "judgment"):
            errs.append(f"{s.get('id')}: precondition {pk!r}")
    for n, sl in g.get("slots", {}).items():
        p = sl.get("pattern", "")
        if not (p.startswith("^") and p.endswith("$")):
            errs.append(f"slot {n}: pattern not anchored")
    return errs


LLM_PROMPT = """You convert an agent skill (markdown) into a JSON procedure graph. Output only JSON with keys:
steps: list of {{id, order, title, role (one of {roles}), commands: [{{template}}], avoid: [{{command, instead, signature}}],
precondition: {{kind: always|after_edit|before_finish|judgment, glob, text}}, expect: {{exit, contains, text}}}}.
Use {{name}} placeholders for values that change per task. Only commands the skill states; no new ones.

SKILL:
{skill}"""


def llm_structure(path: str, model: str = "claude-haiku-4-5-20251001") -> dict:
    """One tool-less compile-time call; the caller validates and reviews the result."""
    import os
    import subprocess
    import tempfile

    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "live"))
    import runner  # the live runner's isolation: fresh HOME and CLAUDE_CONFIG_DIR, host-auth allowlist

    prompt = LLM_PROMPT.format(roles=", ".join(sorted(ROLES)), skill=Path(path).read_text())
    with tempfile.TemporaryDirectory() as home:
        for d in runner.home_dirs(home).values():
            Path(d).mkdir(parents=True, exist_ok=True)
        env = runner.isolation_env(home, os.environ, runner.HOST_AUTH_ENV)
        assert not any(k == "CLAUDECODE" or "SESSION_ID" in k for k in env)
        out = subprocess.run(["claude", "-p", prompt, "--model", model, "--tools", "", "--strict-mcp-config",
                              "--output-format", "json"], capture_output=True, text=True, timeout=300, env=env,
                             cwd=home)
    res = json.loads(out.stdout)
    text = res.get("result", "")
    m = re.search(r"\{.*\}", text, re.S)
    return {"graph": json.loads(m.group(0)) if m else None, "cost_usd": res.get("total_cost_usd")}


# ----------------------------------------------------------------------------------- main


def render(g: Graph) -> str:
    out = [f"skill {g.skill['name']}  ({g.skill['bytes']} bytes, ~{g.skill['approx_tokens']} tokens)",
           f"trigger: spans={g.trigger['spans']} keywords={', '.join(g.trigger['keywords'][:8])}",
           f"slots: " + ", ".join(f"{n}:{s['type']}<-{s['source']}{' e.g. ' + s['example'] if s['example'] else ''}"
                                   for n, s in g.slots.items())]
    for s in g.steps:
        pre = s.precondition.kind + (f"({s.precondition.glob})" if s.precondition.glob else "")
        out.append(f"{s.id} [{s.role:<10}] {pre:<28} {s.title}")
        for c in s.commands:
            out.append(f"      run   {c.template}   ({c.env}, {c.scope})")
        for a in s.avoid:
            out.append(f"      avoid {a.command} -> {a.instead}" + (f"  [{a.signature}]" if a.signature else ""))
        if s.expect.contains or s.expect.text:
            out.append(f"      expect {s.expect.contains or s.expect.text}")
        out.append(f"      {'compilable' if s.compilable else 'not compilable: ' + s.why_not}")
    if g.advice:
        out.append(f"advice (not compiled): {len(g.advice)} block(s)")
    return "\n".join(out)


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__)
        return 2
    g = parse(argv[0])
    if "--out" in argv:
        Path(argv[argv.index("--out") + 1]).write_text(json.dumps(g.to_json(), indent=1) + "\n")
    if "--llm" in argv:
        r = llm_structure(argv[0])
        print(json.dumps({"llm_errors": validate({**(r["graph"] or {}), "schema": SCHEMA,
                                                  "slots": g.slots}), **r}, indent=1))
    print(json.dumps(g.to_json(), indent=1) if "--json" in argv else render(g))
    errs = validate(g.to_json())
    if errs:
        print("validation:", errs, file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
