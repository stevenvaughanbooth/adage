"""Shared helpers for RepoBench-Local: repository definitions, the mirror cache, git plumbing.

Nothing here spends tokens. The cache (`$REPOBENCH_CACHE`, default /tmp/adage-repobench) holds
blobless mirrors of the upstream repositories, package caches and the scorer's environments. It
lives under /tmp, which the live runner's `confine` mode hides from every agent session, so no
agent can read a mirror (and with it the fix it is asked to write).
"""

from __future__ import annotations

import fnmatch
import json
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
REPOS_FILE = HERE / "repos.json"
TASKS_DIR = HERE / "tasks"
CONVENTIONS_DIR = HERE / "conventions"


def cache_dir() -> Path:
    return Path(os.environ.get("REPOBENCH_CACHE") or "/tmp/adage-repobench")


def load_repos() -> dict[str, dict]:
    """repos.json: {id: definition}. See that file's `_doc`."""
    raw = json.loads(REPOS_FILE.read_text())
    return {r["id"]: r for r in raw["repos"]}


def repo(rid: str) -> dict:
    repos = load_repos()
    if rid not in repos:
        raise SystemExit(f"unknown repository {rid!r} (one of {', '.join(sorted(repos))})")
    return repos[rid]


def mirror(r: dict) -> Path:
    return cache_dir() / "mirrors" / r["url"].rstrip("/").split("github.com/")[-1].replace("/", "__")


def git(cwd, *args, check=True, text=True, input=None, env=None) -> str:
    out = subprocess.run(["git", "-C", str(cwd), *args], capture_output=True, text=text, input=input,
                         env=env)
    if check and out.returncode != 0:
        raise RuntimeError(f"git {' '.join(args[:3])}: {out.stderr.strip()[:400] if text else out.stderr[:400]}")
    return out.stdout


def ensure_mirror(r: dict) -> Path:
    """A blobless clone of the upstream repository (blobs are fetched on demand)."""
    m = mirror(r)
    if not (m / ".git").exists() and not (m / "HEAD").exists():
        m.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "clone", "-q", "--filter=blob:none", "--no-checkout", r["url"], str(m)], check=True)
    return m


def matches(path: str, globs) -> bool:
    return any(fnmatch.fnmatch(path, g) for g in globs)


def is_test(r: dict, path: str) -> bool:
    return matches(path, r["test_globs"]) and not matches(path, r.get("test_exclude", []))


def is_source(r: dict, path: str) -> bool:
    return (not is_test(r, path)) and matches(path, r["src_globs"])


def normalise_origin(url: str) -> str:
    """adage-core repo.rs `normalise_origin`: the repository key hooks compute from `origin`."""
    u = url
    for suf in (".git", "/"):
        while u.endswith(suf):
            u = u[: -len(suf)]
    if u.startswith("git@"):
        u = u[4:].replace(":", "/", 1)
    return u.split("://")[-1].lower()


def repo_key(r: dict) -> str:
    """The key Adage stores this repository's tracks under: sandboxes carry the upstream `origin`,
    as a real clone does, so every task's sandbox shares one key."""
    return normalise_origin(r["url"])


def load_tasks(rid: str) -> dict:
    p = TASKS_DIR / f"{rid}.json"
    if not p.exists():
        raise SystemExit(f"no task file {p}; run validate.py first")
    return json.loads(p.read_text())


def task(rid: str, tid: str) -> dict:
    for t in load_tasks(rid)["tasks"]:
        if t["id"] == tid:
            return t
    raise SystemExit(f"{rid}: unknown task {tid!r}")


def is_instinct(entry: dict) -> bool:
    """An index entry ({repo, track, route}) that ships with Adage rather than learned from traces:
    its track is `<repo>/instincts` or its evidence says so. Route ids do not tell (an instinct
    bound as a dead end is `dead-end-<hash>`)."""
    route = entry.get("route") or {}
    origin = ((route.get("evidence") or {}).get("origin") or {}).get("kind")
    return str(entry.get("track", "")).endswith("/instincts") or origin == "instinct"
