"""Method (a): a typed semantic action grammar, derived deterministically.

An action is a tuple

    role × env × scope × relation   (+ runner, targets, output filter and outcome as attributes)

computed from the normalised argv (shell.py), the repository facts (facts.py: environment, project
packages, the file→test map) and the session so far (which files were edited). Nothing is learned
here: every rule is a table lookup or a path test, so the label of a call is a pure function of
(call, repo facts, session prefix).

Label levels, coarse to fine (the taxonomy the miner climbs):

    L0 role                            test-run
    L1 role|envclass                   test-run|project
    L2 role|envclass|scope             test-run|project|file
    L3 role|envclass|scope|relation    test-run|project|file|src-test
    L3e  the concrete env in place of the class: test-run|venv|file|src-test

The env class is `project` (.venv, uv, tox, poetry, npx, pnpm, npm scripts, node_modules/.bin,
go) or `system` (an interpreter outside the project: the dead-end variant); it is `-` for roles
that run no project code (search, read, vcs, file edits).

Roles: test-run, repro, lookup (env/runtime probes, `--version`, `which`, `pip show`), build,
lint (linters and type checks), codegen, env-setup (installs, venv creation), vcs-read, vcs-write,
write-file (a file created through the shell), fs-edit (`sed -i`, `rm`, `mv`), read, search,
trivial (`echo`, `pwd`), other.
Envs: system, venv, uv, tox, poetry, npx, pnpm, npm-script, local-bin, go, shell.
Scopes: test runs: repo | package | file | test-id; repros: inline | scratch-file | repo-file;
reads and searches: src | test | doc | config | env | outside | repo.
Relations: test runs: edited-test | src-test | unrelated | pre-edit | all | by-name;
repros: project | stdlib; reads: edited | related | other.
"""
from __future__ import annotations

import os
import re
from dataclasses import dataclass, field

import facts as F
import shell

PY_RE = re.compile(r"^python\d?(\.\d+)?$")
ENVFAIL_RE = re.compile(r"(No module named|command not found|ModuleNotFoundError|Cannot find module|ERR_MODULE_NOT_FOUND|"
                        r"externally-managed-environment|Unsupported URL Type|ERR_PNPM|EBADDEVENGINES|not installed)")
RAN_RE = re.compile(r"(\d+ (passed|failed|errors?)\b|collected \d+ item|Tests? +\d+ (passed|failed)|^(ok|FAIL|---) |^PASS |Test Files )", re.M)
ROLE_PRIORITY = ["test-run", "repro", "codegen", "build", "lint", "env-setup", "vcs-write", "write-file", "fs-edit",
                 "lookup", "vcs-read", "search", "read", "other", "trivial"]
SELECTOR_OPTS = {"-k", "-t", "--testNamePattern", "-run", "--run", "-m"}
PROJECT_ENV = {"venv", "uv", "tox", "poetry", "npx", "pnpm", "npm-script", "local-bin", "go"}
ENV_ROLES = {"test-run", "repro", "lookup", "build", "lint", "codegen", "env-setup"}


@dataclass
class Act:
    role: str
    env: str = "-"
    scope: str = "-"
    relation: str = "-"
    runner: str = "-"
    targets: list = field(default_factory=list)
    selectors: list = field(default_factory=list)
    piped: bool = False
    outcome: str = "ok"
    tool: str = "Bash"

    @property
    def envclass(self) -> str:
        """`project` (the repository's own environment: .venv, uv, npm scripts, npx, node_modules,
        go) or `system` (an interpreter outside it); `-` for roles that run no project code."""
        if self.role not in ENV_ROLES or self.env in ("-", "shell"):
            return "-"
        return "project" if self.env in PROJECT_ENV else "system"

    def intent(self) -> str:
        """The env-free intent: role | scope class | relation class. What the agent meant to do,
        pooled across the ways it spelled it (and across the right and wrong environment, which
        the compiler decides from repo facts, never from the majority of traces)."""
        if self.role == "test-run":
            sc = "all" if self.scope == "repo" else "narrow"
            rc = {"src-test": "edited-area", "edited-test": "edited-area", "all": "all"}.get(self.relation, self.relation)
            return f"test-run|{sc}|{rc}"
        if self.role == "repro":
            return f"repro|{'scratch' if self.scope in ('inline', 'scratch-file') else self.scope}|{self.relation}"
        return self.level(1)

    def level(self, n: int, detail: bool = False) -> str:
        """L0 role, L1 +env class, L2 +scope, L3 +relation; `detail` puts the concrete env in place of the class."""
        parts = [self.role, self.env if detail else self.envclass, self.scope, self.relation][: n + 1]
        return "|".join(parts)


def area(p: str, fx: F.RepoFacts) -> str:
    if not p:
        return "repo"
    if p.startswith(("/tmp", "/root", "/home", "/usr", "~")) or p.startswith("/"):
        return "outside"
    if p.startswith((".venv", "node_modules")) or "site-packages" in p:
        return "env"
    if F.is_test(p):
        return "test"
    if p.endswith((".md", ".rst", ".txt")) or p.startswith(("docs/", "changelogs/")) or "CHANGE" in p:
        return "doc"
    if p.endswith((".toml", ".cfg", ".ini", ".json", ".yaml", ".yml", ".lock", ".mod", ".sum")) or os.path.basename(p).startswith("."):
        return "config"
    if p in fx.files or p.rstrip("/") in {os.path.dirname(f) for f in ()}:
        return "src"
    # a new file at the top level of the checkout, or a scratch name
    base = os.path.basename(p)
    if "/" not in p.rstrip("/") and base.endswith((".py", ".js", ".ts", ".go", ".mjs")):
        return "scratch"
    return "src"


def _path_like(t: str) -> bool:
    return ("/" in t or re.search(r"\.(py|ts|js|go|tsx|mjs)(::|$)", t) is not None or t in (".", "./...")) and not t.startswith("-")


def _unwrap(argv: list[str], seg: shell.Seg) -> tuple[str, list[str]]:
    """Strip the environment wrapper: returns (env, inner argv)."""
    a = list(argv)
    if not a:
        return "shell", a
    p = a[0]
    if p == "uv" and len(a) > 1 and a[1] == "run":
        rest = a[2:]
        while rest and rest[0].startswith("-"):
            rest = rest[2:] if rest[0] in ("--with", "--python", "-p", "--directory", "--project", "--extra", "--group") else rest[1:]
        return "uv", rest
    if p in ("poetry", "pipenv", "pdm", "hatch") and len(a) > 1 and a[1] == "run":
        return "poetry", a[2:]
    if p in ("npx", "bunx", "pnpx"):
        rest = [x for x in a[1:] if x not in ("--yes", "-y", "--no-install")]
        return "npx", rest
    if p == "pnpm" and len(a) > 1 and a[1] in ("exec", "dlx", "vitest", "tsc", "eslint", "tsx"):
        return "pnpm", a[2:] if a[1] in ("exec", "dlx") else a[1:]
    if p.startswith("node_modules/.bin/") or "/node_modules/.bin/" in p:
        return "local-bin", [os.path.basename(p)] + a[1:]
    if p.startswith((".venv/", "venv/", "./.venv/")) or "/.venv/bin/" in p:
        return "venv", [os.path.basename(p)] + a[1:]
    if seg.activated:
        return "venv", a
    if p in ("tox", "nox"):
        return "tox", a
    if p == "go":
        return "go", a
    if p in ("npm", "pnpm", "yarn", "bun"):
        return "npm-script", a
    return "system", a


def _code_imports_project(raw: str, fx: F.RepoFacts) -> bool:
    if fx.lang == "python":
        return any(re.search(rf"(^|\W)(import|from)\s+{re.escape(p)}\b", raw) for p in fx.packages) or "sys.path.insert" in raw
    if fx.lang == "node":
        return bool(re.search(r"require\(['\"](vue|@vue/|\.\./packages|\./packages)|from ['\"](vue|@vue/|\.\./packages|\./packages)", raw))
    if fx.lang == "go":
        return "urfave/cli" in raw or "package cli" in raw
    return False


def classify_seg(seg: shell.Seg, fx: F.RepoFacts, raw: str) -> Act:
    env, a = _unwrap(seg.argv, seg)
    if not a:
        return Act("trivial", env)
    prog = os.path.basename(a[0])
    rest = a[1:]
    act = Act("other", env, piped=bool(seg.piped))

    def test_targets(ops: list[str]) -> Act:
        act.role = "test-run"
        tg, sel, i = [], [], 0
        while i < len(ops):
            t = ops[i]
            if t in SELECTOR_OPTS and i + 1 < len(ops):
                sel.append(ops[i + 1])
                i += 2
                continue
            if re.match(r"(-k|-t|--testNamePattern|-run|--run)=", t):
                sel.append(t.split("=", 1)[1])
            elif _path_like(t):
                path, _, tid = t.partition("::")
                p = os.path.normpath(os.path.join(seg.cwd, path)) if seg.cwd and not path.startswith("/") else path
                tg.append(p.rstrip("/") + ("::" + tid if tid else ""))
            elif not t.startswith("-") and fx.lang == "node" and re.search(r"\.(spec|test)\b|\w", t) and i > 0 and ops[i - 1] not in ("--reporter", "--project", "-c", "--config"):
                # vitest filters by substring: `npm test -- hydration.spec.ts`, `npm test -- runtime-core`
                if not t.isdigit() and "=" not in t:
                    tg.append(t)
            i += 1
        act.targets, act.selectors = tg, sel
        files = [t for t in tg if "::" in t or re.search(r"\.(py|ts|js|go|tsx)$", t)]
        dirs = [t for t in tg if t not in files and t not in ("tests", ".", "./...", "__tests__", "packages")]
        if sel or any("::" in t for t in tg):
            act.scope = "test-id"
        elif files:
            act.scope = "file"
        elif dirs:
            act.scope = "package"
        else:
            act.scope = "repo"
        return act

    if PY_RE.match(prog) or prog in ("pytest", "py.test"):
        act.runner = "python"
        if prog in ("pytest", "py.test"):
            act.runner = "pytest"
            return test_targets(rest)
        if rest[:2] == ["-m", "pytest"] or rest[:2] == ["-m", "py.test"]:
            act.runner = "pytest"
            return test_targets(rest[2:])
        if rest[:2] == ["-m", "unittest"]:
            act.runner = "unittest"
            return test_targets(rest[2:])
        if rest[:1] == ["-m"] and len(rest) > 1 and rest[1] in ("pip", "venv", "ensurepip"):
            act.role = "env-setup" if len(rest) > 2 and rest[2] in ("install", "uninstall") or rest[1] != "pip" else "lookup"
            return act
        if rest[:1] in (["--version"], ["-V"]):
            act.role = "lookup"
            return act
        code = None
        if "-c" in rest:
            code = rest[rest.index("-c") + 1] if rest.index("-c") + 1 < len(rest) else ""
            act.scope = "inline"
        elif seg.stdin_code or not rest:
            code = raw
            act.scope = "inline"
        else:
            script = next((x for x in rest if not x.startswith("-")), "")
            sp = os.path.normpath(os.path.join(seg.cwd, script)) if seg.cwd and not script.startswith("/") else script
            ar = area(sp, fx)
            if ar == "test" and sp in fx.files:
                act.runner = "script"
                return test_targets([sp])
            act.scope = "scratch-file" if ar in ("scratch", "outside") or sp not in fx.files else "repo-file"
            code = raw
            if sp in fx.files and not F.is_test(sp) and sp.startswith(("scripts/", "tools/")):
                act.role = "build"
                return act
        c = code or ""
        if act.scope == "inline" and len(c) < 160 and re.search(r"__version__|__file__|sys\.version|sys\.path\b|import (pytest|pip)\b|print\(\w+\.__", c) \
                and c.count("\n") < 3:
            act.role = "lookup"
            act.relation = "project" if _code_imports_project(c, fx) else "stdlib"
            return act
        act.role = "repro"
        act.relation = "project" if _code_imports_project(raw, fx) else "stdlib"
        return act

    if prog in ("node", "tsx", "ts-node", "deno", "bun") and prog != "bun" or (prog == "bun" and rest[:1] != ["test"]):
        act.runner = prog
        if rest[:1] in (["--version"], ["-v"]):
            act.role = "lookup"
            return act
        if "-e" in rest or "-p" in rest or seg.stdin_code or not rest:
            act.scope = "inline"
        else:
            script = next((x for x in rest if not x.startswith("-")), "")
            if script in fx.files and script.startswith("scripts/"):
                act.role = "build"
                return act
            if "--test" in rest:
                act.runner = "node-test"
                return test_targets([x for x in rest if x != "--test"])
            act.scope = "scratch-file" if script not in fx.files else "repo-file"
        act.role = "repro"
        act.relation = "project" if _code_imports_project(raw, fx) else "stdlib"
        return act

    if prog in ("vitest", "jest"):
        act.runner = prog
        ops = [x for x in rest if x not in ("run", "watch", "related")]
        return test_targets(ops)
    if prog in ("npm", "pnpm", "yarn", "bun"):
        sub = rest[0] if rest else ""
        if sub in ("install", "i", "ci", "add", "rebuild", "link"):
            act.role = "env-setup"
            return act
        if sub in ("list", "ls", "why", "view", "info", "config", "--version", "-v", "root", "bin"):
            act.role = "lookup"
            return act
        script = rest[1] if sub in ("run", "run-script") and len(rest) > 1 else sub
        ops = rest[2:] if sub in ("run", "run-script") else rest[1:]
        ops = [x for x in ops if x != "--"]
        act.runner = "npm:" + script
        if script.startswith("test") or script == "t" or script == "vitest":
            if fx.test_script.startswith("vitest") or script == "vitest":
                act.runner = "vitest"
            return test_targets([x for x in ops if x not in ("run",)])
        if script.startswith("build"):
            act.role = "build"
        elif script in ("check", "lint", "format", "format-check", "typecheck", "tsc", "eslint"):
            act.role = "lint"
        return act
    if prog == "tsc" or prog in ("eslint", "prettier", "ruff", "flake8", "mypy", "black", "pyright", "pylint", "isort", "golangci-lint", "gofmt", "staticcheck"):
        act.role = "lint"
        act.runner = prog
        return act
    if prog == "go":
        sub = rest[0] if rest else ""
        act.runner = "go"
        if sub == "test":
            return test_targets(rest[1:])
        act.role = {"build": "build", "vet": "lint", "fmt": "lint", "run": "repro", "generate": "codegen", "mod": "env-setup",
                    "get": "env-setup", "install": "env-setup", "version": "lookup", "env": "lookup", "doc": "read", "list": "lookup"}.get(sub, "other")
        if act.role == "repro":
            script = next((x for x in rest[1:] if not x.startswith("-")), "")
            act.scope = "scratch-file" if script not in fx.files else "repo-file"
            act.relation = "project" if _code_imports_project(raw, fx) else "stdlib"
        return act
    if prog in ("make", "just", "task"):
        sub = rest[0] if rest else ""
        act.runner = prog
        if sub.startswith("test") or sub == "check":
            return test_targets(rest[1:])
        act.role = "codegen" if sub.startswith("gen") else "lint" if sub in ("lint", "fmt", "format") else "build" if sub.startswith("build") else "other"
        return act
    if prog in ("tox", "nox"):
        act.runner = prog
        return test_targets(rest)
    if prog in ("pip", "pip3") or (prog == "uv" and rest[:1] in (["pip"], ["sync"], ["venv"], ["add"], ["lock"])):
        sub = rest[1] if prog == "uv" and rest[:1] == ["pip"] and len(rest) > 1 else rest[0] if rest else ""
        act.role = "env-setup" if sub in ("install", "uninstall", "sync", "venv", "add", "lock", "download") else "lookup"
        act.env = "uv" if prog == "uv" else env
        return act
    if prog in ("which", "type", "whereis", "command") or (rest[:1] in (["--version"], ["-V"], ["version"]) and prog not in ("git",)):
        act.role = "lookup"
        return act
    if prog == "git":
        sub = next((x for x in rest if not x.startswith("-")), "")
        act.runner = "git"
        act.role = "vcs-read" if sub in ("status", "diff", "log", "show", "blame", "branch", "rev-parse", "ls-files", "grep", "describe", "shortlog", "remote", "config") else "vcs-write"
        if sub == "grep":
            act.role = "search"
        return act
    if prog in ("cat", "head", "tail", "less", "more", "wc", "nl", "bat", "od", "xxd", "file", "stat") or (prog in ("sed", "awk") and "-i" not in rest):
        if prog == "cat" and seg.writes:
            act.role = "write-file"
            act.scope = area(seg.writes, fx)
            return act
        act.role = "read"
        ops = [x for x in rest if not x.startswith("-") and _path_like(x)]
        act.scope = area(ops[-1], fx) if ops else "repo"
        act.targets = ops
        return act
    if prog in ("grep", "rg", "ag", "find", "ls", "tree", "fd", "egrep", "locate", "du"):
        ops = [x for x in rest if not x.startswith("-") and _path_like(x)]
        act.role = "search"
        act.scope = area(ops[0], fx) if ops else "repo"
        if prog == "ls" and any(x.startswith((".venv", "node_modules")) or "site-packages" in x for x in rest):
            act.role = "lookup"
        act.targets = ops
        return act
    if prog in ("sed", "perl", "rm", "mv", "cp", "mkdir", "touch", "chmod", "ln", "patch", "tee", "truncate"):
        act.role = "fs-edit"
        ops = [x for x in rest if not x.startswith("-") and _path_like(x)]
        act.scope = area(ops[-1], fx) if ops else "repo"
        return act
    if prog in ("echo", "printf", "true", "false", "pwd", "sleep", "clear", "date", "exit", "export", "set", "unset", "cd", "source", "wait"):
        act.role = "trivial"
        return act
    if prog in ("bash", "sh", "zsh"):
        act.role = "repro" if seg.stdin_code or "-c" in rest else "other"
        act.scope = "inline"
        return act
    if prog == "curl" or prog == "wget":
        act.role = "lookup"
        return act
    return act


def outcome(call) -> str:
    o = call.out or ""
    if RAN_RE.search(o):
        return "ran"
    if ENVFAIL_RE.search(o):
        return "env-fail"
    if call.err:
        return "fail"
    return "ok"


def other_tool(call, rel: str, fx: F.RepoFacts, edited: set) -> Act:
    t = call.tool
    if t in ("Edit", "MultiEdit", "Write", "NotebookEdit"):
        ar = area(rel, fx)
        new = t == "Write" and rel not in fx.files
        return Act("edit", "-", ar + ("-new" if new else ""), tool=t, targets=[rel])
    if t == "Read":
        ar = area(rel, fx)
        return Act("read", "-", ar, "edited" if rel in edited else "other", tool=t, targets=[rel])
    if t in ("Grep", "Glob", "LS"):
        return Act("search", "-", "tool", tool=t)
    return Act(t.lower(), tool=t)


def relation_of(act: Act, edited_src: list, edited_tests: set, edit_text: str, repo: str, base: str, fx: F.RepoFacts) -> str:
    if act.role != "test-run":
        return act.relation
    if act.scope == "repo":
        return "all"
    if not edited_src and not edited_tests:
        return "pre-edit"
    files = set()
    for t in act.targets:
        p = t.split("::")[0]
        files.add(p)
    rel_tests = set()
    for s in edited_src:
        rel_tests |= F.related_tests(repo, base, s, fx.files)
    hit = "unrelated"
    for p in files:
        if p in edited_tests:
            return "edited-test"
        # vitest substring filters and directories: any related or edited test under or matching it
        cands = [x for x in (rel_tests | edited_tests) if x == p or x.startswith(p.rstrip("/") + "/") or (fx.lang == "node" and p in x)]
        if any(x in edited_tests for x in cands):
            return "edited-test"
        if cands or p in rel_tests:
            hit = "src-test"
    if not files and act.selectors:
        if any(s and s in edit_text for s in act.selectors):
            return "edited-test"
        return "by-name"
    if hit == "unrelated" and fx.lang == "go" and any(p in (".", "./...") or p.endswith(".go") for p in files):
        return "src-test"
    return hit


def label_session(sess, fx: F.RepoFacts, base: str) -> list[Act]:
    """Every call of a session as an Act, with relations computed from the session prefix."""
    acts = []
    edited_src: list[str] = []
    edited_tests: set[str] = set()
    edit_text = ""
    written: dict[str, str] = {}  # scratch files' contents, so `python /tmp/x.py` knows what x.py imports
    for c in sess.calls:
        if c.tool == "Bash" or c.tool == "bash":
            segs = shell.segments(c.command, sess.cwd)
            raw = c.command
            for s in segs:
                for t in s.argv[1:]:
                    if t in written:
                        raw += "\n" + written[t]
                if s.writes:
                    written[s.writes] = c.command
            cands = [classify_seg(s, fx, raw) for s in segs] or [Act("trivial")]
            act = min(cands, key=lambda a: ROLE_PRIORITY.index(a.role))
            act.relation = relation_of(act, edited_src, edited_tests, edit_text, sess.repo, base, fx)
            act.piped = any(s.piped for s in segs)
            act.outcome = outcome(c)
            # shell-written files and in-place edits count as edits
            for s in segs:
                if s.writes and not s.writes.startswith("/"):
                    w = s.writes
                    (edited_tests.add(w) if F.is_test(w) else edited_src.append(w))
        else:
            rel = sess.rel(c.path)
            if c.tool == "Write":
                written[rel] = str(c.input.get("content", ""))
                written[c.path] = written[rel]
            act = other_tool(c, rel, fx, set(edited_src) | edited_tests)
            act.outcome = "fail" if c.err else "ok"
            if act.role == "edit" and not c.err:
                if F.is_test(rel):
                    edited_tests.add(rel)
                elif area(rel, fx) == "src" and rel not in edited_src:
                    edited_src.append(rel)
                edit_text += str(c.input.get("new_string", "")) + str(c.input.get("content", ""))
        acts.append(act)
    return acts
