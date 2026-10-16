"""Decision units: one per (session, route) at the route's first trigger, delivered or withheld.

A unit is the situation in which Adage's runtime decided a directive applied. In the `adage` arm it
was delivered (`directives`); in `control` it was withheld and logged as `would_have` at the same
hook. The arm was assigned at random per session before any trigger, so the *first* trigger of a
route in a session is exchangeable between arms: comparing units across arms is a randomised
comparison conditional on the trigger firing.

Features (for the p_follow model): directive kind, delivery hook, wording (length, whether it
explains why), timing (calls from delivery to first need), model, repository.

The directive's own command is parsed from the directive text (Adage's own template, not the
agent's behaviour).
"""
from __future__ import annotations

import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

import data

TREATED_ARMS = {"adage", "e1"}
WITHHELD_ARMS = {"control", "agents-md"}


@dataclass
class Unit:
    s: data.Sess
    route: str
    kind: str            # reflex kind: avoid | run | read-batch | sequence | stop ...
    family: str          # route family (dead-end, run, finish, lookup, read, skill, stop)
    hook: str
    text: str
    treated: bool
    pos: int
    directed: list       # directive command(s) / files
    literal: int | None  # runtime's own (literal) follow judgement: 1 followed/obligation_met, 0 ignored, None none
    feats: dict = field(default_factory=dict)


def directed_of(text: str) -> list[str]:
    m = re.search(r"lists these files: (.+?)\.\s*\n", text + "\n")
    if m:
        return ["Read " + f.strip() for f in m.group(1).split(",")]
    cmds = []
    for line in text.splitlines():
        if line.startswith("  ") and line.strip():
            cmds.append(re.sub(r"^\d+\.\s*", "", line.strip()))
    if not cmds:
        cmds = re.findall(r"`([^`]+)`", text)
    return cmds


def action_text(c: data.Call, cwd: str) -> str:
    """How an agent call reads as text: tool + its main argument, run root stripped (data-derived)."""
    if c.name == "Bash":
        x = str(c.inp.get("command", ""))
    elif c.name in ("Read", "Edit", "Write", "MultiEdit", "NotebookEdit"):
        x = f"{c.name} {c.inp.get('file_path', '')}"
    elif c.name in ("Grep", "Glob"):
        x = f"{c.name} {c.inp.get('pattern', '')} {c.inp.get('path', '')}"
    else:
        x = f"{c.name} " + " ".join(str(v) for v in c.inp.values())[:200]
    if cwd:
        x = x.replace(cwd + "/", "").replace(cwd, ".")
    return x[:300]


def _kind_map(S) -> dict:
    km = {}
    for s in S:
        for r, block in s.reflex_text.items():
            m = re.search(r'kind="([^"]+)"', block)
            if m:
                km[(s.root, r)] = m.group(1)
    return km


def build(S=None) -> list[Unit]:
    S = S or data.load()
    km = _kind_map(S)
    kind_any = {r: k for (_, r), k in km.items()}
    units = []
    for s in S:
        if s.arm not in TREATED_ARMS | WITHHELD_ARMS:
            continue
        first, judg = {}, {}
        for e in s.events:
            if e.kind in ("directives", "would_have", "stop_request"):
                r = e.route or "stop"
                if r not in first or e.ts < first[r][0].ts:
                    first[r] = [e, set()]
                first[r][1].add(e.kind)
            elif e.kind in ("followed", "obligation_met"):
                judg[e.route] = 1
            elif e.kind == "ignored":
                judg.setdefault(e.route, 0)
        for r, (e, kinds) in first.items():
            treated = s.arm in TREATED_ARMS and ("directives" in kinds or "stop_request" in kinds)
            if s.arm in TREATED_ARMS and not treated:
                continue
            fam = r.split("-")[0] if r != "stop" else "stop"
            fam = {"dead": "dead-end"}.get(fam, fam)
            kind = km.get((s.root, r)) or kind_any.get(r) or ("stop" if fam == "stop" else fam)
            units.append(Unit(s, r, kind, fam, e.hook, e.text, treated, e.pos, directed_of(e.text),
                              judg.get(r)))
    return units


if __name__ == "__main__":
    import collections
    U = build()
    C = collections.Counter((u.s.pool, u.family, u.kind, u.hook, u.treated, u.s.arm) for u in U)
    for k, v in sorted(C.items(), key=str):
        print(v, k)
    for u in U[:3]:
        print(u.route, u.directed)
