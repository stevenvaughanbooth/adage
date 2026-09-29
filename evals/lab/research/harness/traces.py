"""De-duplicated, cached loader for RepoBench v1/v2 (and the data-expansion corpus).

    from harness import load
    corpus = load()                       # every session, de-duplicated, cached
    corpus.held_out(repo="click")         # held-out control sessions (the test set)
    corpus.train(repo="click")            # training-phase sessions on training tasks

A `Session` keeps what the hooks saw, in order: prompts and model turns (each turn = one API
response's tool calls), every call's input, output (as the model saw it; long outputs are clipped
to head+tail for memory, features are computed on the full text first) and failure flag, plus the
Rust replay's per-call features (`acts`), each API response's token usage (for the cost model),
the model, the outcome and the split.

Which sessions count (and why the older lab loader over-counts): a runner root's `state.json` names
each finished run's `session_id`. A run that was discarded and rerun (`dropped_contaminated`, a
retry) leaves its transcript beside the rerun's, so the transcript directory alone is not the set
of sessions. Only transcripts `state.json` points at are loaded; the rest are counted in
`corpus.excluded`. v2 re-used v1's training transcripts (and both archives hold copies of the
same sessions): de-duplication is by transcript `sessionId`, the first (v2) copy wins, and every
duplicate is recorded with whether its outcome agreed (`corpus.duplicates`).

Cache: `$ADAGE_HARNESS_CACHE` (default /tmp/adage-research/harness/cache), one pickle keyed on the
transcript files' paths, sizes and mtimes. A cold load of ~650 transcripts takes ~30 s; warm < 2 s.
"""

from __future__ import annotations

import hashlib
import json
import os
import pickle
import re
from collections import Counter, defaultdict
from dataclasses import dataclass, field
from pathlib import Path

from . import features as F

HERE = Path(__file__).resolve().parent
ADAGE = HERE.parents[3]
SPLITS = ADAGE / "evals" / "repobench" / "splits"
TASKS = ADAGE / "evals" / "repobench" / "tasks"
V1 = Path(os.environ.get("ADAGE_RB1", "/tmp/adage-repobench/runs"))
V2 = Path(os.environ.get("ADAGE_RB2", "/tmp/adage-rb2/runs"))
DX = ADAGE / "evals" / "lab" / "research" / "data-expansion" / "corpus.json"
CACHE = Path(os.environ.get("ADAGE_HARNESS_CACHE", "/tmp/adage-research/harness/cache"))
ROOTS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
CLIP = 6000  # characters of each output kept (head and tail halves)
LOADER_VERSION = 6


@dataclass
class Call:
    tool: str
    input: dict
    output: str  # as the model saw it (clipped to CLIP chars: head + tail)
    is_error: bool
    hook_output: str | None = None  # what a PostToolUse hook payload carried, when it differs (None: same)
    out_chars: int = 0  # full length of the output


@dataclass
class Response:
    """One API response (model turn), priced by its own usage."""
    model: str
    cache_read: int
    cache_write: int
    input: int
    output: int
    calls: tuple  # indices (into Session.calls) of the tool calls it issued
    cache_write_1h: int = 0  # of cache_write, tokens written with the 1-hour TTL (billed 2x input, not 1.25x)


@dataclass
class Session:
    sid: str  # transcript sessionId
    run: str  # runner run id (eval-click-0cd28d65-r1-control)
    repo: str  # base repository (click)
    slice: str  # runner root (click-sonnet5)
    source: str  # v2 | v1 | dx
    task: str
    arm: str  # train | control | adage | agents-md | ...
    phase: str  # train | eval
    split: str  # train | buffer | test | unknown (the task's split)
    model: str
    passed: bool | None
    cost_usd: float | None  # the harness's total_cost_usd
    stopped: bool
    cwd: str
    prompt: str
    items: list  # ("prompt", text) | ("turn", [call indices])
    calls: list  # [Call]
    acts: list  # [features.Act]
    responses: list  # [Response]
    transcript: str
    steered_routes: tuple = ()  # routes delivered live (hook attachments with <adage-reflex route=…>)
    delivered_at: tuple = ()  # (route, index of the last call before the delivery) for each delivery

    @property
    def n_calls(self) -> int:
        return len(self.calls)

    @property
    def cluster(self) -> str:
        return f"{self.slice}/{self.task}"

    def turns(self):
        for kind, v in self.items:
            if kind == "turn":
                yield v


def _text_of(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(i.get("text", "") for i in content if isinstance(i, dict) and isinstance(i.get("text"), str))
    return ""


def _response_text(v) -> str:
    if isinstance(v, str):
        return v
    if isinstance(v, dict):
        s = []
        for k in ("stdout", "stderr", "error", "content", "output"):
            x = v.get(k)
            if isinstance(x, str) and x:
                s.append(x + "\n")
            elif isinstance(x, list):
                s.extend(it.get("text") + "\n" for it in x if isinstance(it, dict) and isinstance(it.get("text"), str))
        return "".join(s)
    if v is None:
        return ""
    return json.dumps(v)


def _synthetic(s: str) -> bool:
    t = s.lstrip()
    return t.startswith(("<local-command", "<command-", "<system-reminder")) or not t


def _clip(s: str) -> str:
    return s if len(s) <= CLIP else s[: CLIP // 2] + "\n…[clipped]…\n" + s[-CLIP // 2:]


ROUTE = re.compile(r'route=\\?"([A-Za-z0-9_.-]+)')


def parse(path: Path) -> dict | None:
    """Rust `transcript::parse` (items grouped by message id, sidechains skipped), plus usage and
    live deliveries. None without a session id, cwd or prompt."""
    sid, cwd, model = "", None, ""
    raw = []  # ["prompt", text] | ["turn", mid, [(tid, name, input)]]
    results, hook_results = {}, {}
    usage = {}  # mid -> usage dict
    deliveries = []  # (route, number of tool uses seen so far)
    n_uses = 0
    seen_mids = set()
    with path.open(errors="replace") as fh:
        lines = fh.readlines()
    for line in lines:
        try:
            rec = json.loads(line)
        except Exception:
            continue
        if rec.get("isSidechain") is True:
            continue
        sid = sid or (rec.get("sessionId") or "")
        if cwd is None and isinstance(rec.get("cwd"), str):
            cwd = rec["cwd"]
        typ = rec.get("type") or ""
        if typ in ("attachment", "system"):
            hook = (str((rec.get("attachment") or {}).get("type") or "").startswith("hook") if typ == "attachment"
                    else "hook" in str(rec.get("subtype") or ""))
            if hook and "<adage-reflex" in line:
                for r in ROUTE.findall(line):
                    deliveries.append((r, n_uses))
            continue
        if typ == "user":
            content = (rec.get("message") or {}).get("content")
            meta = rec.get("isMeta") is True
            if isinstance(content, str):
                if not meta and not _synthetic(content):
                    raw.append(["prompt", content])
            elif isinstance(content, list):
                tids, texts = [], []
                for it in content:
                    if not isinstance(it, dict):
                        continue
                    if it.get("type") == "tool_result":
                        tid = it.get("tool_use_id") or ""
                        results[tid] = (_text_of(it.get("content")), bool(it.get("is_error")))
                        tids.append(tid)
                    elif isinstance(it.get("text"), str):
                        texts.append(it["text"])
                if "toolUseResult" in rec and len(tids) == 1:
                    hook_results[tids[0]] = _response_text(rec["toolUseResult"])
                if not tids and not meta:
                    t = "\n".join(texts)
                    if not _synthetic(t):
                        raw.append(["prompt", t])
        elif typ == "assistant":
            msg = rec.get("message") or {}
            mid = msg.get("id") or ""
            if msg.get("model") and not model and msg.get("model") != "<synthetic>":
                model = msg["model"]
            if mid and isinstance(msg.get("usage"), dict):
                usage.setdefault(mid, (msg.get("model") or "", msg["usage"]))
            uses = [(it.get("id") or "", it.get("name") or "", it.get("input"))
                    for it in (msg.get("content") or []) if isinstance(it, dict) and it.get("type") == "tool_use"]
            if not uses:
                if mid and mid not in seen_mids:
                    raw.append(["text", mid])
                    seen_mids.add(mid)
                continue
            n_uses += len(uses)
            if mid and mid in seen_mids and not (raw and raw[-1][1] == mid):
                # a text-only row of this message was recorded earlier: drop that placeholder
                raw[:] = [r for r in raw if not (r[0] == "text" and r[1] == mid)]
            seen_mids.add(mid)
            if raw and raw[-1][0] == "turn" and raw[-1][1] == mid and mid:
                raw[-1][2].extend(uses)
            elif raw and raw[-1][0] == "text" and raw[-1][1] == mid and mid:
                raw[-1] = ["turn", mid, list(uses)]
            else:
                raw.append(["turn", mid, list(uses)])
    if cwd is None or not sid or not any(r[0] == "prompt" for r in raw):
        return None
    items, calls, acts, responses = [], [], [], []
    for r in raw:
        if r[0] == "prompt":
            items.append(("prompt", r[1]))
            continue
        mid = r[1]
        idx = []
        if r[0] == "turn":
            for tid, name, inp in r[2]:
                out, err = results.get(tid, ("", False))
                hook = hook_results.get(tid)
                acts.append(F.act(name, inp if isinstance(inp, dict) else {}, out, err, cwd))
                calls.append(Call(name, inp if isinstance(inp, dict) else {}, _clip(out), err,
                                  None if hook is None or hook == out else _clip(hook), len(out)))
                idx.append(len(calls) - 1)
            items.append(("turn", idx))
        m, u = usage.get(mid, ("", {}))
        if u:
            responses.append(Response(m or model, int(u.get("cache_read_input_tokens") or 0),
                                      int(u.get("cache_creation_input_tokens") or 0), int(u.get("input_tokens") or 0),
                                      int(u.get("output_tokens") or 0), tuple(idx),
                                      int((u.get("cache_creation") or {}).get("ephemeral_1h_input_tokens") or 0)))
    delivered = tuple((route, n - 1) for route, n in deliveries)
    return {"sid": sid, "cwd": cwd, "model": model, "items": items, "calls": calls, "acts": acts,
            "responses": responses, "steered_routes": tuple(sorted({d[0] for d in delivered})), "delivered_at": delivered}


def _split_of() -> dict:
    """task → (repo, split) from evals/repobench/splits."""
    out = {}
    for p in sorted(SPLITS.glob("*.json")):
        for repo, sp in json.loads(p.read_text())["repos"].items():
            for part in ("train", "buffer", "test"):
                for t in sp.get(part, []):
                    out[t] = (repo, part)
    return out


def _base_repo(root_name: str) -> str:
    for r in sorted(("attrs", "click", "marshmallow", "urfave-cli", "vue-core"), key=len, reverse=True):
        if root_name == r or root_name.startswith(r + "-"):
            return r
    return root_name


def _transcript_for(root: Path, run_id: str, sid: str | None) -> Path | None:
    tdir = root / "private" / "transcripts"
    if not tdir.is_dir():
        tdir = root / "transcripts"
    dirs = [d for d in tdir.glob(f"*-{run_id}-repo") if d.is_dir()]
    if not dirs:
        return None
    files = sorted(dirs[0].glob("*.jsonl"))
    for f in files:
        if sid and f.stem == sid:
            return f
    # Only a run whose state names no session id may fall back to its directory's sole transcript:
    # when it names one and that file is missing, the file that is there is a discarded run's.
    return files[0] if (len(files) == 1 and not sid) else None


def _discover(bases) -> tuple[list[dict], list[dict]]:
    """(entries to load, excluded transcripts) from runner roots' state.json."""
    entries, excluded = [], []
    for source, base in bases:
        for name in ROOTS:
            root = base / name
            st = root / "state.json"
            if not st.exists():
                continue
            runs = json.loads(st.read_text()).get("runs", {})
            used = set()
            for run_id, run in sorted(runs.items()):
                if run.get("status") not in ("passed", "failed"):
                    continue
                m = run.get("metrics") or {}
                f = _transcript_for(root, run_id, m.get("session_id"))
                if f is None:
                    excluded.append({"root": str(root), "run": run_id, "why": "no transcript for the run's session_id"})
                    continue
                used.add(str(f))
                passed = m.get("hidden_passed", m.get("passed"))
                if passed is None:
                    passed = run.get("status") == "passed"
                entries.append({"path": str(f), "source": source, "state_sid": m.get("session_id"), "slice": name, "repo": _base_repo(name),
                                "run": run_id, "task": run.get("task"), "arm": run.get("arm") or "train",
                                "phase": run.get("phase") or ("train" if run_id.startswith("train") else "eval"),
                                "passed": passed, "cost_usd": m.get("cost_usd"),
                                "stopped": m.get("result_subtype") == "success"})
            tdir = root / "private" / "transcripts"
            for f in sorted(tdir.glob("*/*.jsonl")) if tdir.is_dir() else []:
                if str(f) not in used:
                    excluded.append({"root": str(root), "path": str(f), "why": "not the session state.json names (discarded or retried run)"})
    return entries, excluded


def _dx_entries() -> list[dict]:
    """The data-expansion corpus manifest (`adage-dx-corpus/1`, evals/lab/research/data-expansion/
    manifest.py), when it exists: finished sessions with a transcript. `train-extra` tasks (older
    than the published split) count as training. Sessions on the RepoBench repositories get their
    own slice (`click-dx`) so the published held-out sets do not change under anyone's feet."""
    if not DX.exists():
        return []
    try:
        d = json.loads(DX.read_text())
    except Exception:
        return []
    rows = d.get("sessions", d) if isinstance(d, dict) else d
    out = []
    for r in rows:
        p = r.get("transcript") or r.get("path")
        if not p or r.get("status", "passed") not in ("passed", "failed") or not os.path.exists(p):
            continue
        repo = r.get("repo")
        sp = r.get("split")
        out.append({"path": p, "source": "dx", "state_sid": r.get("session_id"),
                    "slice": r.get("slice") or (f"{repo}-dx" if repo in ("attrs", "click", "marshmallow", "urfave-cli", "vue-core") else repo),
                    "repo": repo, "run": r.get("run_id") or r.get("id") or Path(p).stem, "task": r.get("task"),
                    "arm": r.get("arm") or "control", "phase": r.get("phase") or "eval", "passed": r.get("passed"),
                    "cost_usd": r.get("cost_usd"), "stopped": r.get("result_subtype", "success") == "success",
                    "split": "train" if sp == "train-extra" else sp})
    return out


@dataclass
class Corpus:
    sessions: list
    duplicates: list = field(default_factory=list)
    excluded: list = field(default_factory=list)

    def select(self, repo=None, slice=None, arm=None, phase=None, split=None, source=None, model=None):
        def ok(v, want):
            return want is None or (v in want if isinstance(want, (list, tuple, set)) else v == want)
        return [s for s in self.sessions if ok(s.repo, repo) and ok(s.slice, slice) and ok(s.arm, arm)
                and ok(s.phase, phase) and ok(s.split, split) and ok(s.source, source)
                and (model is None or model in s.model)]

    def held_out(self, repo=None, slice=None, arm="control", model=None, source=None):
        """Held-out sessions: eval phase, on a test task, in `arm` (control: no directive steered them).
        `source`: "v2", "v1", "dx" or a list (default: all, including the data-expansion corpus)."""
        return self.select(repo=repo, slice=slice, arm=arm, phase="eval", split="test", model=model, source=source)

    def train(self, repo=None, slice=None, source=None):
        """Training memory: training-phase sessions on training-split tasks."""
        return self.select(repo=repo, slice=slice, phase="train", split="train", source=source)

    def slices(self):
        return sorted({s.slice for s in self.sessions})

    def summary(self) -> str:
        c = Counter((s.slice, s.phase, s.arm) for s in self.sessions)
        lines = [f"{len(self.sessions)} sessions; {len(self.duplicates)} duplicate copies dropped; {len(self.excluded)} transcripts excluded"]
        for k in sorted(c):
            lines.append(f"  {k[0]:<18} {k[1]:<5} {k[2]:<10} {c[k]}")
        return "\n".join(lines)


def _key(entries) -> str:
    h = hashlib.sha1(str(LOADER_VERSION).encode())
    for e in entries:
        st = os.stat(e["path"])
        h.update(f"{e['path']}|{st.st_size}|{int(st.st_mtime)}|{e['passed']}|{e['arm']}".encode())
    return h.hexdigest()[:16]


_MEMO: dict = {}


def load(bases=None, include_dx: bool = True, cache: bool = True) -> Corpus:
    """Every finished session in RepoBench v2, then v1 (and the data-expansion corpus), de-duplicated
    by transcript sessionId (first copy wins: v2 before v1)."""
    bases = bases or [("v2", V2), ("v1", V1)]
    entries, excluded = _discover(bases)
    if include_dx:
        entries += _dx_entries()
    key = _key(entries)
    if key in _MEMO:
        return _MEMO[key]
    cp = CACHE / f"corpus-{key}.pkl"
    if cache and cp.exists():
        with cp.open("rb") as f:
            corpus = pickle.load(f)
        _MEMO[key] = corpus
        return corpus
    split = _split_of()
    by_sid: dict[str, Session] = {}
    dups = []
    for e in entries:
        p = parse(Path(e["path"]))
        if p is None:
            excluded.append({"path": e["path"], "why": "unparseable (no session id, cwd or prompt)"})
            continue
        if e.get("state_sid") and p["sid"] != e["state_sid"]:
            excluded.append({"path": e["path"], "why": "transcript's sessionId is not the one state.json names"})
            continue
        if p["sid"] in by_sid:
            first = by_sid[p["sid"]]
            dups.append({"sid": p["sid"], "kept": f"{first.source}:{first.slice}:{first.run}",
                         "dropped": f"{e['source']}:{e['slice']}:{e['run']}",
                         "outcome_agrees": first.passed == e["passed"], "same_task": first.task == e["task"]})
            continue
        sp = e.get("split") or split.get(e["task"], (None, "unknown"))[1]
        prompt = next((v for k, v in p["items"] if k == "prompt"), "")
        by_sid[p["sid"]] = Session(
            sid=p["sid"], run=e["run"], repo=e["repo"], slice=e["slice"], source=e["source"], task=e["task"],
            arm=e["arm"], phase=e["phase"], split=sp, model=p["model"], passed=e["passed"], cost_usd=e["cost_usd"],
            stopped=e["stopped"], cwd=p["cwd"], prompt=prompt, items=p["items"], calls=p["calls"], acts=p["acts"],
            responses=p["responses"], transcript=e["path"], steered_routes=p["steered_routes"],
            delivered_at=p["delivered_at"])
    corpus = Corpus(sorted(by_sid.values(), key=lambda s: (s.slice, s.run)), dups, excluded)
    if cache:
        CACHE.mkdir(parents=True, exist_ok=True)
        for old in CACHE.glob("corpus-*.pkl"):
            old.unlink()
        tmp = cp.with_suffix(".tmp")
        with tmp.open("wb") as f:
            pickle.dump(corpus, f, protocol=pickle.HIGHEST_PROTOCOL)
        tmp.replace(cp)
    _MEMO[key] = corpus
    return corpus


def session_from_transcript(path, repo: str = "repo", task: str = "t", arm: str = "control", phase: str = "eval",
                            split: str = "test", passed: bool | None = None, cost_usd: float | None = None,
                            stopped: bool = True, run: str | None = None, slice: str | None = None) -> Session | None:
    """One Session from any Claude Code transcript (for tests, or for sessions outside RepoBench)."""
    p = parse(Path(path))
    if p is None:
        return None
    prompt = next((v for k, v in p["items"] if k == "prompt"), "")
    return Session(sid=p["sid"], run=run or Path(path).stem, repo=repo, slice=slice or repo, source="file", task=task,
                   arm=arm, phase=phase, split=split, model=p["model"], passed=passed, cost_usd=cost_usd, stopped=stopped,
                   cwd=p["cwd"], prompt=prompt, items=p["items"], calls=p["calls"], acts=p["acts"],
                   responses=p["responses"], transcript=str(path), steered_routes=p["steered_routes"],
                   delivered_at=p["delivered_at"])
