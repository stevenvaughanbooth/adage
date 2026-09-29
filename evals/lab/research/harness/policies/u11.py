"""Example policy: instinct U11 ("use the project's .venv"), as bound live.

HAND-WRITTEN BASELINE (the brief's "baseline to beat"): the triggers are the U11 routes Adage
bound against each repository's checkout (`private/adage-home/index.json` of the RepoBench v2 root,
routes whose origin is instinct U11 and whose step runs the `.venv`), i.e. a runner spelling
(`python -m pytest`, `pytest`, `tox`, …) plus an error signature (`No module named pytest`, …).

Three delivery variants:
  post-hoc     at PostToolUseFailure of a matching call (as delivered live)            → Rust default
  hindsight    also at PreToolUse of a call that *will* fail with a matching signature
               (needs score(hindsight=True)); reproduces Rust `--mode pre-emptive`
  predictive   at PreToolUse, from the command alone: a system-interpreter run of a runner whose
               spelling has already failed with a U11 signature in this repository's *training*
               sessions (no hindsight; what a PreToolUse rewrite could actually do)
Each route fires at most once per session (the runtime's `emit_ok`).
"""

from __future__ import annotations

import json
from functools import lru_cache

from .. import features as F
from ..replay import Directive
from ..traces import V2

TEXT = ("That failed. In this repository, run this instead:\n  .venv/bin/python -m pytest\n"
        "Ignore this if the user is asking for something else.\n"
        "Adage default for Python repositories (from this repository's .venv/pyvenv.cfg); not requested by the user.")


def wrapped(route: str, repo_track: str) -> str:
    return (f'<adage-reflex track="{repo_track}/instincts@1.0.0" route="{route}" strength="prime" kind="avoid" '
            f'origin="instinct">\n{TEXT}\n</adage-reflex>')


@lru_cache(maxsize=None)
def routes(repo: str) -> tuple:
    """(route id, runner spelling, signature, directed command) for the repository's bound U11 routes."""
    p = V2 / repo / "private" / "adage-home" / "index.json"
    if not p.exists():
        return ()
    out = []
    for x in json.loads(p.read_text()).get("routes", []):
        r = x.get("route", x)
        origin = (r.get("evidence") or {}).get("origin") or {}
        trig, step = r.get("trigger") or {}, r.get("step") or {}
        if origin.get("instinct") != "U11" or trig.get("on") != "after_failure" or ".venv" not in step.get("command", ""):
            continue
        out.append((r["id"], trig["command"].removeprefix("exec "), trig["signature"], step["command"], x.get("track", "")))
    return tuple(out)


def spelling(cmd: str) -> str | None:
    """The runner spelling a failed call used, in a route's terms (`python -m pytest`, `pytest`, `tox`),
    for runs outside the project's environment."""
    for seg in F.segments(cmd):
        inv = F.invocation(seg)
        if not inv or inv[0].interp not in ("system", "tox", "nox"):
            continue
        w = F.words(seg)
        while w and (w[0] in ("env", "time", "exec", "nice", "command", "timeout") or ("=" in w[0] and not w[0].startswith("-"))):
            w.pop(0)
            if w and w[0][:1].isdigit():
                w.pop(0)
        if not w:
            continue
        b = F.base(w[0])
        if F.is_python(w[0]) and "-m" in w:
            m = w.index("-m")
            return f"{b} -m {w[m + 1]}" if m + 1 < len(w) else None
        return b
    return None


class U11:
    def __init__(self, mode: str = "post-hoc", train_sessions=None):
        assert mode in ("post-hoc", "hindsight", "predictive")
        self.mode = mode
        self.name = f"U11 {mode}"
        self.events = {"PostToolUse", "PreToolUse"}
        # predictive: which spellings failed with a U11 signature in training, per repository.
        self.bad = {}
        for s in train_sessions or []:
            for c, a in zip(s.calls, s.acts):
                if a.tool == "Bash" and a.failed:
                    sp = spelling(a.cmd)
                    if sp and any(sig in c.output for _, rsp, sig, _, _ in routes(s.repo) if rsp == sp):
                        self.bad.setdefault(s.repo, set()).add(sp)

    def _match(self, repo, cmd, output):
        sp = spelling(cmd)
        if sp is None:
            return None
        for rid, rsp, sig, command, track in routes(repo):
            if rsp == sp and sig in output:
                return rid, command, track
        return None

    def _give(self, sit, rid, command, track):
        fired = sit.state.setdefault("u11", set())
        if rid in fired:
            return None
        fired.add(rid)
        return Directive("run", text=wrapped(rid, track.split("/")[0]), command=command, route=rid, wrapped=True)

    def __call__(self, sit):
        repo = sit.session.repo
        if not routes(repo) or sit.call is None or sit.call.tool != "Bash":
            return None
        cmd = sit.call.input.get("command", "") if isinstance(sit.call.input, dict) else ""
        cmd = cmd.replace(sit.session.cwd.rstrip("/") + "/", "").replace(sit.session.cwd.rstrip("/"), ".")
        if sit.event == "PostToolUse":
            if not sit.failed:
                return None
            m = self._match(repo, cmd, sit.call.output)
            return self._give(sit, *m) if m else None
        # PreToolUse
        if self.mode == "hindsight" and sit.hindsight and sit.hindsight["will_fail"]:
            m = self._match(repo, cmd, sit.hindsight["output"])
            return self._give(sit, *m) if m else None
        if self.mode == "predictive":
            sp = spelling(cmd)
            if sp and sp in self.bad.get(repo, ()):
                rid, _, _, command, track = next(r for r in routes(repo) if r[1] == sp)
                return self._give(sit, "pre:" + sp, command, track)
        return None
