"""Load RepoBench sessions for the approximate-subgraph study (cached).

Reuses `evals/lab/traces.py` (transcript parsing, de-duplication by session id) and adds what this
study needs: the session's $ cost and tool-call count from the runner's `state.json`, the model
(Haiku 4.5 for the base repositories, Sonnet 5 for the `-sonnet5` roots) and the split role.

Each event keeps
- `text`: the call as the model issued it (sandbox root stripped, 160 chars): the ONLY input to the
  learned node representation;
- `fail`: the harness's `is_error` bit (not a hand-written signature);
- `key` / `full`: `evals/lab/normalize.py`'s hand-written normaliser levels, used ONLY for the
  exact-match baselines and for reading patterns, never by the learned methods.
"""

from __future__ import annotations

import json
import pickle
import sys
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
LAB = HERE.parents[1]
sys.path.insert(0, str(LAB))

import metrics  # noqa: E402
import traces  # noqa: E402

CACHE = Path("/tmp/adage-research/approx-subgraph")
BASE_REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
ALL_ROOTS = BASE_REPOS + ["click-sonnet5", "vue-core-sonnet5"]


@dataclass
class Ev:
    tool: str
    text: str
    fail: bool
    key: str
    full: str
    batch: int
    explore: bool = False  # MEASUREMENT ONLY: hand-written exploratory-tool list (normalize.py)
    discovery: bool = False  # MEASUREMENT ONLY: hand-written repo-discovery regex (lab metrics.py)


@dataclass
class Sess:
    sid: str
    repo: str  # base repository (sonnet roots folded into their repo)
    model: str  # haiku | sonnet
    arm: str
    phase: str
    task: str
    role: str  # train | test (held-out control) | other
    passed: bool | None
    cost: float | None
    calls: int
    events: list[Ev] = field(default_factory=list)


def _costs() -> dict[str, tuple[float | None, int | None]]:
    out = {}
    for base in traces.BASES:
        for root in ALL_ROOTS:
            p = base / root / "state.json"
            if not p.exists():
                continue
            for rid, st in json.loads(p.read_text()).get("runs", {}).items():
                m = st.get("metrics") or {}
                sid = m.get("session_id")
                if sid:
                    out[sid] = (m.get("cost_usd"), m.get("tool_calls"))
    return out


def load(refresh: bool = False) -> list[Sess]:
    CACHE.mkdir(parents=True, exist_ok=True)
    pk = CACHE / "sessions.pkl"
    if pk.exists() and not refresh:
        return pickle.loads(pk.read_bytes())
    raw = traces.load(repos=ALL_ROOTS)
    costs = _costs()
    splits = {r: traces.split(r) for r in BASE_REPOS}
    out, seen = [], set()
    for root in ALL_ROOTS:
        repo = root.replace("-sonnet5", "")
        model = "sonnet" if root.endswith("sonnet5") else "haiku"
        for s in raw[root]:
            if s.sid in seen:
                continue
            seen.add(s.sid)
            sp = splits[repo]
            task = s.task.replace("-sonnet5", "")
            if s.phase == "train" and task in sp["train"]:
                role = "train"
            elif s.phase == "eval" and s.arm == "control" and task in sp["test"]:
                role = "test"
            else:
                role = "other"
            c, n = costs.get(s.sid, (None, None))
            evs = [Ev(st.action.tool, st.action.text, st.failed, st.action.key, st.action.full, st.batch,
                      st.action.explore, bool(metrics.discovery(st)))
                   for st in s.steps]
            out.append(Sess(s.sid, repo, model, s.arm, s.phase, task, role, s.passed, c,
                            n or len(evs), evs))
    pk.write_bytes(pickle.dumps(out))
    return out


if __name__ == "__main__":
    from collections import Counter
    import data as _d
    S = _d.load(refresh=True)
    print(len(S), "sessions")
    print(Counter((s.repo, s.model, s.role) for s in S))
    import statistics
    for r in BASE_REPOS:
        t = [s for s in S if s.repo == r and s.role == "test" and s.model == "haiku"]
        print(r, len(t), statistics.mean(len(s.events) for s in t),
              statistics.mean(s.cost for s in t if s.cost), sum(s.cost is None for s in t))
