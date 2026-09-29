"""Repository facts: what a static scan of the checkout (plus its git history) says, with no trace.

These are the inputs the grammar needs to label an action relative to the repository (is
`tests/test_core.py` a test of the edited `src/click/core.py`? is `.venv/bin/python` the
project's interpreter?) and the inputs the compiler needs to turn an abstract action back into a
concrete command. Everything here is deterministic and computable at day 0.

- **Environment.** Python: a project `.venv` (in a live checkout: `.venv/pyvenv.cfg`; RepoBench's
  sandboxes had one, made by `setup`, and it was deleted after scoring, so it is taken from
  `evals/repobench/repos.json`), `uv.lock`, `tox.ini`, `noxfile.py`. Node: the lockfile's package
  manager, the `test` script and its runner. Go: `go.mod`.
- **Project packages** (for "does this scratch script import the project?").
- **File → test map**: the naming rule (`test_<stem>.py`, `<stem>.spec.ts`, `<stem>_test.go`),
  git co-change before the task's base commit (commits of < 40 files), and the package's test
  directory (`packages/<pkg>/__tests__`) as the fallback.
"""
from __future__ import annotations

import collections
import functools
import json
import os
import re
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

MIRRORS = Path("/tmp/adage-repobench/mirrors")
MIRROR = {"click": "pallets__click", "attrs": "python-attrs__attrs", "marshmallow": "marshmallow-code__marshmallow",
          "urfave-cli": "urfave__cli", "vue-core": "vuejs__core"}
REPOS_JSON = Path(__file__).resolve().parents[2] / "repobench" / "repos.json"
TEST_RE = re.compile(r"(^|/)(tests?/|__tests__/|testing/)|_test\.go$|\.(spec|test)\.[jt]sx?$|(^|/)test_[^/]*\.py$|(^|/)conftest\.py$")


def is_test(p: str) -> bool:
    return bool(TEST_RE.search(p))


@dataclass
class RepoFacts:
    repo: str
    lang: str  # python | node | go | ?
    venv: str = ""  # ".venv" when the project has one
    uv_lock: bool = False
    tox: bool = False
    pm: str = ""  # npm | pnpm | yarn
    test_script: str = ""  # package.json scripts.test
    packages: list[str] = field(default_factory=list)  # importable project package names
    test_root: str = ""  # tests | packages/*/__tests__ | .
    files: frozenset = frozenset()  # tracked files at the checkout

    def interpreter(self) -> str:
        return f"{self.venv}/bin/python" if self.venv else "python3"

    def test_prefix(self) -> str:
        """The project's test invocation, as a static scan would verify it."""
        if self.lang == "python":
            return f"{self.interpreter()} -m pytest"
        if self.lang == "node":
            runner = self.test_script.split()[0] if self.test_script else "vitest"
            if runner == "vitest":
                return "node_modules/.bin/vitest run"
            return f"{self.pm or 'npm'} test --"
        if self.lang == "go":
            return "go test"
        return ""


def _setup_has_venv(repo: str) -> bool:
    try:
        for r in json.load(open(REPOS_JSON))["repos"]:
            if r["id"] == repo:
                return any(".venv" in s for s in r.get("setup", []))
    except Exception:
        pass
    return False


@functools.lru_cache(maxsize=None)
def tree(repo: str, base: str) -> frozenset:
    """Files tracked at the task's base commit (the checkout itself also holds what agents left)."""
    g = MIRRORS / MIRROR.get(repo, "")
    if not base or not g.exists():
        return frozenset()
    out = subprocess.run(["git", "-C", str(g), "ls-tree", "-r", "--name-only", base], capture_output=True, text=True).stdout
    return frozenset(out.split())


@functools.lru_cache(maxsize=None)
def scan(repo: str, checkout: str, base: str = "") -> RepoFacts:
    root = Path(checkout)
    fs = tree(repo, base)
    if not fs:
        files = []
        for dp, dns, fns in os.walk(root):
            dns[:] = [d for d in dns if d not in (".git", "node_modules", ".venv", "__pycache__", ".pytest_cache", "dist")]
            for f in fns:
                files.append(os.path.relpath(os.path.join(dp, f), root))
        fs = frozenset(files)
    if "go.mod" in fs:
        f = RepoFacts(repo, "go", test_root=".")
    elif "package.json" in fs:
        pj = json.load(open(root / "package.json"))
        pm = "pnpm" if "pnpm-lock.yaml" in fs else "yarn" if "yarn.lock" in fs else "npm"
        f = RepoFacts(repo, "node", pm=pm, test_script=pj.get("scripts", {}).get("test", ""), test_root="packages/*/__tests__")
    elif "pyproject.toml" in fs or "setup.py" in fs:
        pk = sorted({p.split("/")[1] for p in fs if re.match(r"src/[A-Za-z_]\w*/__init__\.py$", p)}
                    | {p.split("/")[0] for p in fs if re.match(r"[A-Za-z_]\w*/__init__\.py$", p) and not p.startswith(("tests", "docs"))})
        venv = ".venv" if ((root / ".venv" / "pyvenv.cfg").exists() or _setup_has_venv(repo)) else ""
        f = RepoFacts(repo, "python", venv=venv, uv_lock="uv.lock" in fs, tox="tox.ini" in fs, packages=pk, test_root="tests")
    else:
        f = RepoFacts(repo, "?")
    f.files = fs
    if f.lang == "node":
        f.packages = sorted({"vue"} | {p.split("/")[1] for p in fs if re.match(r"packages/[^/]+/package\.json$", p)})
    if f.lang == "go":
        f.packages = ["cli"]
    return f


@functools.lru_cache(maxsize=None)
def _history(repo: str, base: str) -> tuple:
    g = MIRRORS / MIRROR.get(repo, "")
    if not base or not g.exists():
        return ()
    log = subprocess.run(["git", "-C", str(g), "log", "--name-only", "--format=format:@@", "-n", "3000", base],
                         capture_output=True, text=True).stdout
    return tuple(frozenset(x.split()) for x in log.split("@@") if x.strip())


def naming_tests(src: str, files) -> list[str]:
    stem = os.path.splitext(os.path.basename(src))[0].lstrip("_")
    if not stem:
        return []
    pat = re.compile(r"(^|/)(test_)?_?" + re.escape(stem) + r"(_test)?\.(py|go)$|(^|/)" + re.escape(stem) + r"\.(spec|test)\.[jt]sx?$")
    return sorted(p for p in files if is_test(p) and pat.search(p))


def package_tests(src: str, files) -> list[str]:
    m = re.match(r"(packages/[^/]+)/", src)
    if m:
        return sorted(p for p in files if p.startswith(m.group(1) + "/__tests__/") and re.search(r"\.(spec|test)\.[jt]sx?$", p))
    if src.endswith(".go"):
        d = os.path.dirname(src)
        return sorted(p for p in files if p.endswith("_test.go") and os.path.dirname(p) == d)
    return []


@functools.lru_cache(maxsize=None)
def cochange(repo: str, base: str, src: str, k: int = 5) -> tuple:
    co = collections.Counter()
    for c in _history(repo, base):
        if src in c and len(c) < 40:
            for f in c:
                if is_test_module(f):
                    co[f] += 1
    return tuple(f for f, _ in sorted(co.items(), key=lambda x: (-x[1], x[0]))[:k])


def is_test_module(p: str) -> bool:
    """A file a test runner collects (not a fixture, stub, typing example or data file)."""
    b = os.path.basename(p)
    return bool(re.match(r"test_.*\.py$|.*_test\.(py|go)$|.*\.(spec|test)\.[jt]sx?$", b))


def tests_for(repo: str, base: str, src: str, files) -> list[str]:
    """Ranked tests for an edited source file: the naming rule, then git co-change."""
    out: list[str] = []
    for t in naming_tests(src, files) + [t for t in cochange(repo, base, src) if t in files]:
        if t not in out:
            out.append(t)
    return out


def related_tests(repo: str, base: str, src: str, files) -> set[str]:
    """Every test the map relates to `src` (for labelling; wider than the ranked list)."""
    return set(naming_tests(src, files)) | set(cochange(repo, base, src, k=8)) | set(package_tests(src, files))
