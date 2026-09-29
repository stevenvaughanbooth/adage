"""Load every RepoBench session (v1 + v2, Haiku + Sonnet, the two synthetic repos) into raw calls.

Nothing here classifies anything: a call is (tool, input, output text, harness is_error, batch).
Repo-specific sandbox roots are stripped so paths are repository-relative. Sessions are
de-duplicated by session id. Sonnet roots are folded into their repository (`click-sonnet5` ->
`click`) because leave-one-repo-out must hold out the whole repository.

Cache: /tmp/adage-research/cross-repo-transfer/sessions.pkl
"""

from __future__ import annotations

import json
import pickle
import re
from dataclasses import dataclass, field
from pathlib import Path

BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
ROOTS = {  # run root -> (repository, model)
    "attrs": ("attrs", "haiku"), "click": ("click", "haiku"), "marshmallow": ("marshmallow", "haiku"),
    "urfave-cli": ("urfave-cli", "haiku"), "vue-core": ("vue-core", "haiku"),
    "click-sonnet5": ("click", "sonnet"), "vue-core-sonnet5": ("vue-core", "sonnet"),
    "gen-py-clinic-s12": ("gen-py-clinic", "haiku"), "gen-node-logistics-s21": ("gen-node-logistics", "haiku"),
}
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "gen-py-clinic", "gen-node-logistics"]
REAL = REPOS[:5]
CACHE = Path("/tmp/adage-research/cross-repo-transfer")
SPLITS = Path(__file__).resolve().parents[3] / "repobench" / "splits"
# The snapshot of each repository a zero-config install would see (the v2 bind sandbox: a fresh
# checkout with its environment set up). The synthetic repositories have none archived.
TREES = {
    "attrs": "/tmp/adage-rb2/runs/attrs/private/bind-sandbox/repo",
    "click": "/tmp/adage-rb2/runs/click/private/bind-sandbox/repo",
    "marshmallow": "/tmp/adage-rb2/runs/marshmallow/private/bind-sandbox/repo",
    "vue-core": "/tmp/adage-rb2/runs/vue-core/private/bind-sandbox/repo",
    "urfave-cli": "/tmp/adage-repobench/runs/urfave-cli/private/instincts-sandbox/repo",
}
ROOT_RE = re.compile(r"/tmp/adage-[\w.-]+/runs/[\w.-]+/(?:private/)?(?:runs/[\w.-]+/)?"
                     r"(?:repo|bind-sandbox/repo|instincts-sandbox/repo|wardline-clinic|[\w-]+)(?=/|\b)")


@dataclass
class Call:
    tool: str
    inp: dict
    out: str
    is_error: bool
    batch: int


@dataclass
class Session:
    sid: str
    repo: str
    model: str
    rid: str
    phase: str
    arm: str
    task: str
    passed: bool | None
    cost: float | None
    calls: list = field(default_factory=list)
    order: int = 0  # chronological rank of the task among training tasks (train only)


def strip_root(s: str) -> str:
    s = ROOT_RE.sub(".", s)
    return re.sub(r"(?<![\w.])\./", "", s)


def parse(path: Path) -> list[Call]:
    rows = [json.loads(l) for l in path.open() if l.strip()]
    res = {}
    for r in rows:
        c = r.get("message", {}).get("content") if r.get("type") == "user" else None
        if isinstance(c, list):
            for x in c:
                if isinstance(x, dict) and x.get("type") == "tool_result":
                    cc = x.get("content")
                    t = cc if isinstance(cc, str) else " ".join(
                        y.get("text", "") for y in (cc or []) if isinstance(y, dict))
                    res[x.get("tool_use_id")] = (bool(x.get("is_error")), t)
    calls, batch, last = [], -1, None
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        tu = [x for x in (r.get("message", {}).get("content") or [])
              if isinstance(x, dict) and x.get("type") == "tool_use"]
        if not tu:
            continue
        mid = r.get("message", {}).get("id")
        if mid != last:
            batch += 1
        last = mid
        for u in tu:
            err, text = res.get(u.get("id"), (False, ""))
            inp = {k: (strip_root(v) if isinstance(v, str) else v) for k, v in (u.get("input") or {}).items()
                   if k in ("command", "file_path", "pattern", "path", "notebook_path")}
            text = strip_root(text)
            if len(text) > 3000:
                text = text[:1500] + "\n...\n" + text[-1500:]
            calls.append(Call(u.get("name", ""), inp, text, err, batch))
    return calls


def load(refresh: bool = False) -> list[Session]:
    CACHE.mkdir(parents=True, exist_ok=True)
    p = CACHE / "sessions.pkl"
    if p.exists() and not refresh:
        return pickle.loads(p.read_bytes())
    out, seen = [], set()
    for base in BASES:
        for root, (repo, model) in ROOTS.items():
            d = base / root
            if not (d / "transcripts").is_dir():
                continue
            try:
                state = json.loads((d / "state.json").read_text()).get("runs", {})
            except Exception:
                state = {}
            ids = sorted(state, key=len, reverse=True)
            for f in sorted((d / "transcripts").glob("*/*.jsonl")):
                if f.stem in seen:
                    continue
                name = f.parent.name
                rid = next((i for i in ids if f"-runs-{i}-" in name or name.endswith(f"-runs-{i}")), None)
                if rid is None:
                    continue
                st = state[rid]
                met = st.get("metrics") or {}
                passed = met.get("hidden_passed", met.get("passed"))
                calls = parse(f)
                if not calls:
                    continue
                seen.add(f.stem)
                out.append(Session(f.stem, repo, model, rid, st.get("phase", ""), st.get("arm") or "train",
                                   st.get("task", ""), passed, met.get("cost_usd"), calls))
    # chronological order of training tasks from the committed split (real repositories)
    for repo in REAL:
        try:
            train = json.loads((SPLITS / f"{repo}.json").read_text())["repos"][repo]["train"]
        except Exception:
            continue
        rank = {t: i for i, t in enumerate(train)}
        for s in out:
            if s.repo == repo:
                s.order = rank.get(s.task, 1000)
    p.write_bytes(pickle.dumps(out))
    return out


def tree(repo: str, depth: int = 3) -> set[str]:
    """Repository-relative paths up to `depth` components (dirs end with '/'). `.git` is skipped."""
    root = TREES.get(repo)
    if not root or not Path(root).is_dir():
        return set()
    root = Path(root)
    out = set()

    def walk(d: Path, rel: str, k: int):
        try:
            entries = list(d.iterdir())
        except Exception:
            return
        for e in entries:
            if e.name == ".git":
                continue
            r = rel + e.name
            isdir = e.is_dir() and not e.is_symlink()
            out.add(r + ("/" if isdir else ""))
            if isdir and k < depth:
                walk(e, r + "/", k + 1)

    walk(root, "", 1)
    return out


if __name__ == "__main__":
    import collections
    ss = load(refresh=True)
    c = collections.Counter((s.repo, s.model, s.arm) for s in ss)
    for k in sorted(c):
        print(k, c[k])
    for r in REAL:
        t = tree(r)
        print(r, len(t), sorted(x for x in t if x.count("/") <= 1 and not x.startswith(("node_modules/", ".venv/lib")))[:12])
