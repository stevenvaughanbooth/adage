"""The repository at each task's base commit, from the RepoBench upstream mirrors (read-only).

- `tasks(repo)`: the task records (prompt, base sha, gold files) from evals/repobench/tasks.
- `snapshot(repo, base)`: {path: blob oid} for the tracked text files at `base`, and the blob texts
  (deduplicated by oid across bases; cached).
- `history(repo, base)`: the file sets of the last 2,000 non-merge commits reachable from `base`
  (only the past of the task: no leakage of the task's own commit), as the learner's testmap reads
  them (`crates/adage-learn/src/testmap.rs`: HISTORY = 2000, MAX_COMMIT_FILES = 40).
"""

from __future__ import annotations

import json
import pickle
import subprocess
from functools import lru_cache
from pathlib import Path

RB = Path(__file__).resolve().parents[3] / "repobench"
MIRRORS = Path("/tmp/adage-repobench/mirrors")
CACHE = Path("/tmp/adage-research/context-prefetch/repo")
MAX_BYTES = 300_000
HISTORY, MAX_COMMIT_FILES = 2000, 40


def git(m: Path, *args, inp: bytes | None = None) -> bytes:
    return subprocess.run(["git", "-C", str(m), *args], input=inp, capture_output=True, check=True).stdout


@lru_cache(None)
def taskfile(repo: str) -> dict:
    return json.loads((RB / "tasks" / f"{repo}.json").read_text())


def tasks(repo: str) -> dict:
    return {t["id"]: t for t in taskfile(repo)["tasks"]}


def split(repo: str) -> dict:
    return json.loads((RB / "splits" / f"{repo}.json").read_text())["repos"][repo]


def mirror(repo: str) -> Path:
    url = taskfile(repo)["url"].rstrip("/")
    owner, name = url.split("/")[-2:]
    return MIRRORS / f"{owner}__{name}"


_blobs: dict = {}


def _blob_store(repo: str) -> dict:
    if repo not in _blobs:
        f = CACHE / f"{repo}.blobs.pkl"
        _blobs[repo] = pickle.loads(f.read_bytes()) if f.exists() else {}
    return _blobs[repo]


def save():
    CACHE.mkdir(parents=True, exist_ok=True)
    for repo, d in _blobs.items():
        (CACHE / f"{repo}.blobs.pkl").write_bytes(pickle.dumps(d))


@lru_cache(None)
def snapshot(repo: str, base: str) -> dict:
    """{path: oid} of the text files at base (binary and > 300 kB files left out)."""
    f = CACHE / f"{repo}.{base[:12]}.tree.json"
    if f.exists():
        return json.loads(f.read_text())
    m = mirror(repo)
    rows = git(m, "ls-tree", "-r", "-l", base).decode().splitlines()
    tree = {}
    for r in rows:
        meta, path = r.split("\t", 1)
        mode, typ, oid, size = meta.split()
        if typ != "blob" or size == "-" or int(size) > MAX_BYTES:
            continue
        tree[path] = oid
    store = _blob_store(repo)
    need = [o for o in set(tree.values()) if o not in store]
    if need:
        out = git(m, "cat-file", "--batch", inp=("\n".join(need) + "\n").encode())
        i = 0
        while i < len(out):
            nl = out.index(b"\n", i)
            oid, _, size = out[i:nl].decode().split()
            size = int(size)
            data = out[nl + 1: nl + 1 + size]
            i = nl + 1 + size + 1
            store[oid] = None if b"\0" in data[:8000] else data.decode("utf-8", "replace")
    tree = {p: o for p, o in tree.items() if store.get(o) is not None}
    CACHE.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(tree))
    return tree


def text(repo: str, oid: str) -> str:
    return _blob_store(repo)[oid]


@lru_cache(None)
def history(repo: str, base: str) -> list:
    f = CACHE / f"{repo}.{base[:12]}.hist.json"
    if f.exists():
        return json.loads(f.read_text())
    log = git(mirror(repo), "log", "--no-merges", "--name-only", "--format=@@%ct", "-n", str(HISTORY), base).decode()
    commits = []
    for blk in log.split("@@")[1:]:
        lines = [l for l in blk.splitlines() if l.strip()]
        ts, files = int(lines[0]), lines[1:]
        if 0 < len(files) <= MAX_COMMIT_FILES:
            commits.append([ts, files])
    f.write_text(json.dumps(commits))
    return commits


if __name__ == "__main__":
    import sys
    for repo in sys.argv[1:] or ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]:
        T = tasks(repo)
        for tid, t in T.items():
            tr = snapshot(repo, t["base"])
            h = history(repo, t["base"])
        save()
        print(repo, len(T), "tasks; files at last base", len(tr), "; history", len(h),
              "; blobs", len(_blob_store(repo)))
