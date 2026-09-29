"""Load RepoBench transcripts into sessions of normalised steps.

A session is the list of tool calls one Claude Code transcript made, in order, each normalised at
three levels (see `normalize.py`) and marked failed / exploratory; `batches` groups the calls that
one assistant message issued together (a decision point comes after each batch).

Discovery reads the archived run directories (default `/tmp/adage-repobench/runs` and
`/tmp/adage-rb2/runs`): `<base>/<repo>/transcripts/<run dir>/<session>.jsonl`, with the runner's
`state.json` next to it for the outcome (hidden tests passed). Sessions are de-duplicated by id
(RepoBench v2 re-used v1's training traces).
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from pathlib import Path

from normalize import Action, normalize_tool

BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
LAB = Path(__file__).resolve().parent
SPLITS = LAB.parent / "repobench" / "splits"

RUN = re.compile(r"^(train|eval)-(.+)-([0-9a-f]{8})-r(\d+)(?:-(.+))?$")


@dataclass
class Step:
    action: Action
    failed: bool
    error: str  # first error-looking line of the result, when failed
    batch: int


@dataclass
class Session:
    sid: str
    repo: str
    run: str
    phase: str  # train | eval
    arm: str  # train | control | adage | agents-md
    task: str
    passed: bool | None
    prompt: str
    steps: list[Step] = field(default_factory=list)
    source: str = ""

    @property
    def n_batches(self) -> int:
        return (self.steps[-1].batch + 1) if self.steps else 0


def error_line(text: str) -> str:
    lines = [l.strip() for l in text.splitlines() if l.strip()]
    for l in lines:
        if re.search(r"(Error|error:|FAILED|No module named|not found|No such file|Traceback|panic|fatal)", l):
            return l[:200]
    return (lines[-1][:200] if lines else "")


def parse_transcript(path: Path) -> tuple[str, list[Step]]:
    prompt, steps, uses, batch = "", [], {}, -1
    rows = [json.loads(l) for l in path.open() if l.strip()]
    results = {}
    for r in rows:
        if r.get("type") != "user":
            continue
        c = r.get("message", {}).get("content")
        if isinstance(c, str) and not prompt:
            prompt = c
        elif isinstance(c, list):
            for x in c:
                if isinstance(x, dict) and x.get("type") == "text" and not prompt:
                    prompt = x.get("text", "")
                if isinstance(x, dict) and x.get("type") == "tool_result":
                    cc = x.get("content")
                    text = cc if isinstance(cc, str) else " ".join(
                        y.get("text", "") for y in (cc or []) if isinstance(y, dict))
                    results[x.get("tool_use_id")] = (bool(x.get("is_error")), text)
    for r in rows:
        if r.get("type") != "assistant" or r.get("isSidechain"):
            continue
        c = r.get("message", {}).get("content") or []
        tu = [x for x in c if isinstance(x, dict) and x.get("type") == "tool_use"]
        if not tu:
            continue
        # Claude Code writes one assistant row per content block; consecutive tool-use rows with
        # the same message id are one batch.
        mid = r.get("message", {}).get("id")
        if not uses or uses.get("_last_mid") != mid:
            batch += 1
        uses["_last_mid"] = mid
        for u in tu:
            failed, text = results.get(u.get("id"), (False, ""))
            a = normalize_tool(u.get("name", ""), u.get("input") or {})
            steps.append(Step(a, failed, error_line(text) if failed else "", batch))
    return prompt, steps


def _state(base: Path, repo: str) -> dict:
    p = base / repo / "state.json"
    try:
        return json.loads(p.read_text()).get("runs", {})
    except Exception:
        return {}


def load(repos=REPOS, bases=BASES) -> dict[str, list[Session]]:
    out: dict[str, list[Session]] = {r: [] for r in repos}
    seen = set()
    for base in bases:
        for repo in repos:
            state = _state(base, repo)
            tdir = base / repo / "transcripts"
            if not tdir.is_dir():
                continue
            for f in sorted(tdir.glob("*/*.jsonl")):
                sid = f.stem
                if sid in seen:
                    continue
                d = f.parent.name
                rid = re.sub(r"^.*-runs-(?=(train|eval)-)", "", d)
                rid = re.sub(r"-repo$", "", rid)
                m = RUN.match(rid)
                if not m:
                    continue
                phase, _, h, rnd, arm = m.groups()
                st = state.get(rid, {})
                met = st.get("metrics") or {}
                passed = met.get("hidden_passed", met.get("passed"))
                if passed is None and st.get("status") in ("passed", "failed"):
                    passed = st["status"] == "passed"
                prompt, steps = parse_transcript(f)
                if not steps:
                    continue
                seen.add(sid)
                out[repo].append(Session(sid, repo, rid, phase, arm or "train", f"{repo}-{h}", passed,
                                         prompt, steps, str(f)))
    return out


def split(repo: str) -> dict:
    return json.loads((SPLITS / f"{repo}.json").read_text())["repos"][repo]
