"""The pre-registered protocol for Adage on external benchmarks, as code (stdlib only).

1. Per-task metadata comes from the Harbor task directory: SWE-bench-family adapters (swebenchpro,
   swebench-verified, swegym, swebench_multilingual) write the upstream record to
   `tests/config.json` (repo, base_commit, patch, test_patch, FAIL_TO_PASS / fail_to_pass, ...).
2. Split, per repository, chronologically: tasks are ordered by their base commit's committer date
   (`git show -s --format=%cI <sha>` in a cached upstream clone), ties by base commit then task id.
   The oldest `train_frac` go to training, the next `buffer` (2-3) are discarded, the rest are the
   test set. Training never sees a commit newer than any test task's base commit; the buffer
   absorbs near-simultaneous PRs that touch the same code.
3. Leakage: nothing Adage learned may name a test task's gold-patch file or test id unless the
   training transcripts themselves touched it (then it is legitimately learned repo knowledge).
4. Patch cleanliness: the diff an arm is scored on must not contain Adage's or a harness's files
   (.adage/, .claude/, .codex/, .opencode/), so the arms are compared on the same kind of patch.

`split_digest` is the SHA-256 of the canonical split: publish it before any eval job runs.
"""

from __future__ import annotations

import ast
import hashlib
import json
import math
import re
import subprocess
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Callable, Iterable

PROTOCOL_VERSION = "adage-external/1"
DEFAULT_TRAIN_FRAC = 0.5
DEFAULT_BUFFER = 3
MIN_TEST = 5
FORBIDDEN_DIRS = (".adage", ".claude", ".codex", ".opencode")


# ------------------------------------------------------------------------------ task metadata

@dataclass
class TaskMeta:
    task_id: str
    repo: str | None
    base_commit: str | None
    gold_files: list[str] = field(default_factory=list)
    test_files: list[str] = field(default_factory=list)
    test_ids: list[str] = field(default_factory=list)


def _as_list(v) -> list[str]:
    if v is None:
        return []
    if isinstance(v, str):
        v = v.strip()
        if not v:
            return []
        try:
            v = json.loads(v)
        except ValueError:
            try:  # swebenchpro stores Python-literal lists: "['a::b']"
                v = ast.literal_eval(v)
            except (ValueError, SyntaxError):
                return [v]
    return [str(x) for x in v] if isinstance(v, list) else [str(v)]


_DIFF_GIT = re.compile(r"^diff --git a/(\S+) b/(\S+)$", re.M)
_PLUS = re.compile(r"^\+\+\+ b/(\S+)", re.M)
_MINUS = re.compile(r"^--- a/(\S+)", re.M)


def patch_files(diff: str | None) -> list[str]:
    """Paths a unified diff touches (both sides of renames), in first-seen order."""
    out: dict[str, None] = {}
    for m in _DIFF_GIT.finditer(diff or ""):
        out[m.group(1)] = None
        out[m.group(2)] = None
    for rx in (_PLUS, _MINUS):
        for m in rx.finditer(diff or ""):
            out[m.group(1)] = None
    return [p for p in out if p != "/dev/null"]


def meta_from_record(task_id: str, rec: dict) -> TaskMeta:
    """TaskMeta from an upstream SWE-bench-style record (either key case)."""
    get = lambda *ks: next((rec[k] for k in ks if rec.get(k) not in (None, "")), None)  # noqa: E731
    return TaskMeta(
        task_id=task_id,
        repo=get("repo"),
        base_commit=get("base_commit"),
        gold_files=patch_files(get("patch")),
        test_files=patch_files(get("test_patch")),
        test_ids=_as_list(get("FAIL_TO_PASS", "fail_to_pass")) + _as_list(get("PASS_TO_PASS", "pass_to_pass")),
    )


def load_task_meta(task_dir: str | Path, task_id: str | None = None) -> TaskMeta:
    """Metadata for one downloaded Harbor task directory."""
    d = Path(task_dir)
    tid = task_id or d.name
    cfg = d / "tests" / "config.json"
    if cfg.exists():
        return meta_from_record(tid, json.loads(cfg.read_text()))
    return TaskMeta(task_id=tid, repo=None, base_commit=None)


def registry_entry(registry: list | dict, name: str, version: str | None = None) -> dict:
    """One dataset from Harbor's registry.json (a list of {name, version, description, tasks})."""
    items = registry if isinstance(registry, list) else registry.get("datasets", [])
    hits = [d for d in items if d.get("name") == name and (version is None or str(d.get("version")) == str(version))]
    if not hits:
        raise KeyError(f"dataset {name}@{version} not in registry")
    return hits[-1]


_PRO = re.compile(r"^instance_(?P<owner>[^_]+(?:_[^_]+)*?)__(?P<name>.+?)-[0-9a-f]{40}")
_SWEB = re.compile(r"^(?P<owner>[A-Za-z0-9_.-]+?)__(?P<name>[A-Za-z0-9_.-]+)-\d+$")
_SMITH = re.compile(r"^(?P<owner>[A-Za-z0-9_.-]+?)__(?P<name>[A-Za-z0-9_-]+)\.[0-9a-f]{8}\.")


def repo_from_task_name(name: str) -> str | None:
    """Repository slug (owner/name) from a Harbor task name, when the metadata lacks one:
    `instance_ansible__ansible-<sha>-...` (swebenchpro), `django__django-15098` (swebench family),
    `oauthlib__oauthlib.1fd52536...` (swesmith), `biomejs__biome-7314` (swe-gen-js)."""
    for rx in (_PRO, _SMITH, _SWEB):
        m = rx.match(name)
        if m:
            return f"{m.group('owner')}/{m.group('name')}"
    return None


def repo_slug(repo: str) -> str:
    return repo.replace("/", "__")


# ----------------------------------------------------------------------------- commit dates

class CommitDates:
    """Committer dates of base commits, from bare partial clones under `clone_dir`, cached in
    `cache_json` so a split is reproducible offline once computed."""

    def __init__(self, clone_dir: str | Path, cache_json: str | Path,
                 url_of: Callable[[str], str] = lambda repo: f"https://github.com/{repo}.git",
                 run: Callable[..., subprocess.CompletedProcess] = subprocess.run):
        self.clone_dir, self.cache_json, self.url_of, self.run = Path(clone_dir), Path(cache_json), url_of, run
        self.cache: dict[str, str] = json.loads(self.cache_json.read_text()) if self.cache_json.exists() else {}

    def _git(self, *args, check=True):
        return self.run(["git", *args], capture_output=True, text=True, check=check)

    def clone(self, repo: str) -> Path:
        d = self.clone_dir / f"{repo_slug(repo)}.git"
        if not d.exists():
            d.parent.mkdir(parents=True, exist_ok=True)
            self._git("clone", "--bare", "--filter=blob:none", self.url_of(repo), str(d))
        return d

    def __call__(self, repo: str, sha: str) -> str:
        key = f"{repo}@{sha}"
        if key in self.cache:
            return self.cache[key]
        d = self.clone(repo)
        r = self._git("-C", str(d), "show", "-s", "--format=%cI", sha, check=False)
        if r.returncode != 0:  # not on a fetched ref (e.g. a PR base): fetch the commit itself
            self._git("-C", str(d), "fetch", "--filter=blob:none", "origin", sha)
            r = self._git("-C", str(d), "show", "-s", "--format=%cI", sha)
        self.cache[key] = r.stdout.strip()
        self.save()
        return self.cache[key]

    def save(self):
        self.cache_json.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.cache_json.with_suffix(".tmp")
        tmp.write_text(json.dumps(self.cache, indent=1, sort_keys=True))
        tmp.replace(self.cache_json)


def _utc(iso: str) -> datetime:
    dt = datetime.fromisoformat(iso.replace("Z", "+00:00"))
    return (dt if dt.tzinfo else dt.replace(tzinfo=timezone.utc)).astimezone(timezone.utc)


# ------------------------------------------------------------------------------------ split

def chronological_split(tasks: Iterable[TaskMeta], date_of: Callable[[str, str], str],
                        train_frac: float = DEFAULT_TRAIN_FRAC, buffer: int = DEFAULT_BUFFER,
                        min_test: int = MIN_TEST) -> dict:
    """Per-repository chronological split. Deterministic: the same tasks and dates give the same
    split whatever order the tasks come in. Repositories too small for a train set, the buffer and
    `min_test` test tasks are listed under `excluded`, never silently merged."""
    if not 0 < train_frac < 1:
        raise ValueError("train_frac must be in (0, 1)")
    if buffer < 0:
        raise ValueError("buffer must be >= 0")
    by_repo: dict[str, list[TaskMeta]] = {}
    unassigned = []
    for t in tasks:
        if not t.repo or not t.base_commit:
            unassigned.append(t.task_id)
            continue
        by_repo.setdefault(t.repo, []).append(t)
    repos, excluded = {}, {}
    for repo in sorted(by_repo):
        dated = sorted(((_utc(date_of(repo, t.base_commit)), t.base_commit, t.task_id) for t in by_repo[repo]))
        order = [tid for _, _, tid in dated]
        n = len(order)
        n_train = math.floor(n * train_frac)
        if n_train < 1 or n - n_train - buffer < min_test:
            excluded[repo] = {"n": n, "reason": f"needs >= 1 train + {buffer} buffer + {min_test} test"}
            continue
        repos[repo] = {
            "train": order[:n_train],
            "buffer": order[n_train:n_train + buffer],
            "test": order[n_train + buffer:],
            "cutoff": dated[n_train - 1][0].isoformat(),
            "first_test": dated[n_train + buffer][0].isoformat(),
            "dates": {tid: d.isoformat() for d, _, tid in dated},
        }
    return {"protocol": PROTOCOL_VERSION, "train_frac": train_frac, "buffer": buffer, "min_test": min_test,
            "repos": repos, "excluded": excluded, "unassigned": sorted(unassigned)}


def split_digest(split: dict) -> str:
    """SHA-256 of the split's task lists (not the dates' formatting): the pre-registration hash."""
    core = {r: {k: v[k] for k in ("train", "buffer", "test")} for r, v in split["repos"].items()}
    body = json.dumps({"protocol": split["protocol"], "train_frac": split["train_frac"], "buffer": split["buffer"],
                       "repos": core}, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(body.encode()).hexdigest()


def leave_repo_out(repos: list[str], seed: int = 0) -> dict[str, str]:
    """For the leave-repo-out control: each repository gets another repository's index (a
    deterministic derangement: sorted, rotated by 1 + seed mod (n-1))."""
    rs = sorted(repos)
    if len(rs) < 2:
        raise ValueError("leave-repo-out needs at least two repositories")
    k = 1 + seed % (len(rs) - 1)
    return {r: rs[(i + k) % len(rs)] for i, r in enumerate(rs)}


# ---------------------------------------------------------------------------------- leakage

def artifact_texts(adage_home: str | Path) -> list[tuple[str, str]]:
    """(where, text) for everything learned that can reach an agent: each index route as JSON
    (its summary, trigger, step and slots are what directives are rendered from), and every track
    file (track.toml, routes/*.yaml) as raw text."""
    home = Path(adage_home)
    out = []
    idx = home / "index.json"
    if idx.exists():
        for i, r in enumerate(json.loads(idx.read_text()).get("routes") or []):
            rid = (r.get("route") or {}).get("id", str(i))
            out.append((f"index.json:{r.get('repo')}:{rid}", json.dumps(r, ensure_ascii=False)))
    tracks = home / "tracks"
    if tracks.exists():
        for f in sorted(tracks.rglob("*")):
            if f.is_file() and f.suffix in (".yaml", ".toml", ".json"):
                out.append((f.relative_to(home).as_posix(), f.read_text(errors="replace")))
    return out


def _mentions(text: str, item: str) -> bool:
    """`item` appears in `text` as a whole path/id (not inside a longer identifier). JSON-escaped
    text is searched too."""
    for t in (text, text.replace("\\/", "/")):
        start = 0
        while True:
            i = t.find(item, start)
            if i < 0:
                break
            before = t[i - 1] if i > 0 else " "
            after = t[i + len(item)] if i + len(item) < len(t) else " "
            if not (before.isalnum() or before in "_-") and not (after.isalnum() or after in "_"):
                return True
            start = i + 1
    return False


def leakage_items(test_tasks: Iterable[TaskMeta], train_texts: Iterable[str]) -> list[dict]:
    """Gold-patch files, test files and test ids of the test tasks that no training transcript
    mentions: naming one of these can only have come from outside training."""
    corpus = "\n".join(train_texts)
    items: dict[tuple[str, str], set] = {}
    for t in test_tasks:
        for kind, vals in (("gold_file", t.gold_files), ("test_file", t.test_files), ("test_id", t.test_ids)):
            for v in vals:
                if v and len(v) >= 4:
                    items.setdefault((kind, v), set()).add(t.task_id)
    return [{"kind": k, "item": v, "tasks": sorted(ts)} for (k, v), ts in sorted(items.items())
            if not _mentions(corpus, v)]


def check_leakage(artifacts: list[tuple[str, str]], test_tasks: Iterable[TaskMeta],
                  train_texts: Iterable[str]) -> dict:
    """Findings: each learned artifact that names a test-only item. `ok` is True when there are none."""
    items = leakage_items(test_tasks, train_texts)
    findings = []
    for where, text in artifacts:
        for it in items:
            if _mentions(text, it["item"]):
                findings.append({"where": where, **it})
    return {"ok": not findings, "checked_items": len(items), "artifacts": len(artifacts), "findings": findings}


# --------------------------------------------------------------------------- patch cleanliness

def _forbidden(path: str) -> bool:
    parts = [p for p in path.strip().strip('"').split("/") if p]
    return any(p in FORBIDDEN_DIRS for p in parts)


def status_paths(status: str) -> list[str]:
    """Paths from `git status --porcelain=v1` output (both sides of a rename)."""
    out = []
    for line in (status or "").splitlines():
        if len(line) < 4:
            continue
        rest = line[3:]
        out.extend(p.strip() for p in rest.split(" -> ")) if " -> " in rest else out.append(rest.strip())
    return out


def check_patch_clean(diff: str | None = None, status: str | None = None, untracked: str | None = None) -> dict:
    """Violations: paths under .adage/, .claude/, .codex/ or .opencode/ (at any depth) in the
    scored diff, the worktree status, or the untracked-file list."""
    paths = patch_files(diff) + status_paths(status or "") + [
        p.strip() for p in (untracked or "").splitlines() if p.strip()]
    bad = sorted({p for p in paths if _forbidden(p)})
    return {"ok": not bad, "violations": bad}


def to_json(obj) -> str:
    return json.dumps(asdict(obj) if hasattr(obj, "__dataclass_fields__") else obj, indent=1, sort_keys=True)
