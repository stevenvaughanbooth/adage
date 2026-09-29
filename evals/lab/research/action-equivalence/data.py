"""Load every RepoBench session once, de-duplicated, as flat call records (cached).

Reuses the abstraction lab's loader and its label cache (`/tmp/adage-abstraction-cache.pkl`, built by
`evals/lab/abstraction/corpus.py`), which carries, per call, the label of `grammar.py`: the Python
ancestor of `crates/adage-core/src/action.rs`. That label is used ONLY as the baseline to beat.

A call record keeps: repo, session key, index, tool, raw command (sandbox root stripped), the
first 1500 characters of its output, `is_error` (the harness's flag), and the baseline labels.
The learned methods see only `tool`, `cmd`, `out`, `is_error` and the session order.
"""
from __future__ import annotations

import hashlib
import os
import pickle
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
ABS = HERE.parents[1] / "abstraction"
sys.path.insert(0, str(ABS))
CACHE = Path(os.environ.get("AE_CACHE", "/tmp/adage-research/action-equivalence"))
CACHE.mkdir(parents=True, exist_ok=True)
REPOS = ("attrs", "click", "marshmallow", "urfave-cli", "vue-core")


@dataclass
class Rec:
    repo: str
    skey: str
    idx: int
    tool: str
    cmd: str
    out: str
    is_error: bool
    turn: int
    # baselines (grammar.py = action.rs ancestor); never seen by the learned methods
    role: str = "-"
    runner: str = "-"
    envclass: str = "-"
    intent: str = "-"
    l1: str = "-"


@dataclass
class Sess:
    key: str
    repo: str
    model: str
    arm: str
    phase: str
    task: str
    passed: bool | None
    cwd: str
    cost_usd: float | None = None
    recs: list = field(default_factory=list)


def _strip(cmd: str, cwd: str) -> str:
    """Make run-specific roots generic: the checkout becomes relative, the run directory `~run`."""
    c = cwd.rstrip("/")
    if not c:
        return cmd
    cmd = cmd.replace(c + "/", "").replace(c, ".")
    run = c.rsplit("/", 1)[0]
    return cmd.replace(run, "~run") if run.count("/") > 2 else cmd


def _cost(sid: str) -> float | None:
    tag, rdir, run = sid.split("/")
    root = {"v2": "/tmp/adage-rb2", "v1": "/tmp/adage-repobench"}[tag]
    p = Path(root) / "runs" / rdir / "runs" / run / "stream.jsonl"
    try:
        m = re.findall(r'total_cost_usd\\?":([0-9.]+)', p.read_text())
    except OSError:
        return None
    return float(m[-1]) if m else None


def load(refresh: bool = False) -> list[Sess]:
    p = CACHE / "sessions.pkl"
    if p.exists() and not refresh:
        return pickle.load(open(p, "rb"))
    L = pickle.load(open("/tmp/adage-abstraction-cache.pkl", "rb"))
    seen, out = set(), []
    # v2 first (the loader's order), so a v1 copy of a v2 run is the one dropped
    for lab in L:
        s = lab.sess
        if s.arm not in ("train", "control", "adage", "agents-md"):
            continue
        sig = hashlib.sha1("\x00".join(f"{c.tool}:{_strip(c.command, s.cwd) or c.path}" for c in s.calls).encode()).hexdigest()
        dk = (s.repo, s.model, s.task, sig)
        if dk in seen:
            continue
        seen.add(dk)
        ss = Sess(key=s.sid, repo=s.repo, model=s.model, arm=s.arm, phase=s.phase, task=s.task,
                  passed={"passed": True, "failed": False}.get(s.status), cwd=s.cwd, cost_usd=_cost(s.sid))
        for c, a in zip(s.calls, lab.acts):
            r = Rec(repo=s.repo, skey=s.sid, idx=c.idx, tool=c.tool, cmd=_strip(c.command, s.cwd) if c.tool == "Bash" else _strip(c.path, s.cwd),
                    out=_strip(c.out[:1500], s.cwd), is_error=bool(c.err), turn=c.turn)
            r.role, r.runner, r.envclass = a.role, a.runner, a.envclass
            r.intent, r.l1 = a.intent(), a.level(1)
            ss.recs.append(r)
        out.append(ss)
    pickle.dump(out, open(p, "wb"))
    return out


if __name__ == "__main__":
    import collections
    import data as _d
    S = _d.load(refresh=True)
    print(len(S), "sessions after de-duplication")
    print(collections.Counter((s.repo, s.model, s.arm) for s in S))
    print(sum(1 for s in S for r in s.recs if r.tool == "Bash"), "bash calls")
