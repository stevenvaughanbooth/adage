"""Shortcut instances, their learned equivalence across sessions, and the library of shortcut classes.

An *instance* is one hindsight shortcut of one session (`hindsight.Shortcut`): what an oracle could
have said, the earliest call t0 it could have said it at, the achieving call t* it leads to, and the
set D of calls it would have removed.

Equivalence ("roughly the same shortcut", learned, no hand-written normalisation):

- command-like kinds (command, procedure, action): same tool, and the IDF-weighted Jaccard of their
  *stable* input tokens >= `T_CMD`. Stable = seen in the inputs of >= 2 memory sessions, so the
  task-specific parts (a test id, a scratch script's body) drop out and the reusable skeleton stays
  (`python -m pytest tests/...`, `.venv/bin/activate`, `npm test --`, `pip install -e .`).
- file-like kinds (file, edit): share a path.
- fact: share a rare token.

A *class* is a greedy cluster of memory instances under that relation; its support is the number of
distinct memory sessions in it.
"""

from __future__ import annotations

from collections import Counter, defaultdict
from dataclasses import dataclass, field

from hindsight import Idf, toks

CMD = {"command", "procedure", "action"}
FILE = {"file"}
FACT = {"fact"}
DELIVERABLE = CMD | FILE | FACT
T_CMD = 0.6
M_MIN = 1.0  # min IDF mass of a command skeleton (about one token absent from 2 in 3 sessions)


def family(kind: str) -> str:
    return "cmd" if kind in CMD else "file" if kind in FILE else "fact"


@dataclass
class Inst:
    sid: str
    task: str
    repo: str
    kind: str
    tool: str
    t0: int
    anchor: int
    D: tuple
    key: frozenset
    text: str
    content: tuple = ()

    @property
    def fam(self) -> str:
        return family(self.kind)


def instances(s, lab) -> list[Inst]:
    out = []
    for sc in lab.shortcuts:
        if sc.kind not in DELIVERABLE:
            continue
        tool = s.calls[sc.content[0]].tool
        if sc.kind == "procedure":
            text = "first: " + " ; ".join(s.calls[c].inp_text[:150] for c in sc.content[:-1]) + \
                   " ; then: " + s.calls[sc.content[-1]].inp_text[:150]
        elif sc.kind in CMD:
            text = " ; then ".join(s.calls[c].inp_text[:200] for c in sc.content)
        elif sc.kind in FILE:
            text = "the file that matters: " + ", ".join(sorted(sc.tokens))
        else:
            text = "fact: " + ", ".join(sorted(sc.tokens)[:5])
        out.append(Inst(s.sid, s.task, s.repo, sc.kind, tool, sc.t0, sc.anchor, tuple(sc.D), sc.tokens, text,
                        tuple(sc.content)))
    return out


class Vocab:
    """Input tokens seen in >= 2 memory sessions (the reusable part of a command)."""

    def __init__(self, sessions):
        df = Counter()
        for s in sessions:
            seen = set()
            for c in s.calls:
                seen |= toks(c.inp_text)
            df.update(seen)
        self.stable = {t for t, v in df.items() if v >= 2}

    def skel(self, key) -> frozenset:
        return frozenset(t for t in key if t in self.stable)


def match(a: Inst, b: Inst, idf: Idf, vocab: Vocab, t_cmd: float = T_CMD) -> bool:
    if a.fam != b.fam:
        return False
    if a.fam == "cmd":
        if a.tool != b.tool:
            return False
        ka, kb = vocab.skel(a.key), vocab.skel(b.key)
        if idf.mass(ka) < M_MIN or idf.mass(kb) < M_MIN:
            return False  # a skeleton of schema words only (`Read file_path offset`) says nothing
        return idf.wjacc(ka, kb) >= t_cmd
    return bool(a.key & b.key)


@dataclass
class Class:
    cid: int
    fam: str
    members: list = field(default_factory=list)

    @property
    def sessions(self) -> set:
        return {m.sid for m in self.members}

    @property
    def rep(self) -> Inst:
        # the most common kind's first member (the text delivered)
        return self.members[0]


class Library:
    def __init__(self, insts: list[Inst], idf: Idf, vocab: Vocab, t_cmd: float = T_CMD):
        self.idf, self.vocab, self.t_cmd = idf, vocab, t_cmd
        self.classes: list[Class] = []
        self.of: dict = {}  # id(inst) -> cid
        by_tok = defaultdict(list)  # file/fact token -> cids
        for x in insts:
            cid = None
            if x.fam == "cmd":
                for c in self.classes:
                    if c.fam == "cmd" and match(c.rep, x, idf, vocab, t_cmd):
                        cid = c.cid
                        break
            else:
                for t in x.key:
                    for c in by_tok.get((x.fam, t), []):
                        cid = c
                        break
                    if cid is not None:
                        break
            if cid is None:
                cid = len(self.classes)
                self.classes.append(Class(cid, x.fam))
            self.classes[cid].members.append(x)
            self.of[id(x)] = cid
            if x.fam != "cmd":
                for t in x.key:
                    if cid not in by_tok[(x.fam, t)]:
                        by_tok[(x.fam, t)].append(cid)

    def cls(self, x: Inst) -> int:
        return self.of[id(x)]

    def matches(self, cid: int, h: Inst) -> bool:
        c = self.classes[cid]
        return any(match(m, h, self.idf, self.vocab, self.t_cmd) for m in c.members[:5])

    def support(self, cid: int, exclude_sid: str | None = None, exclude_task: str | None = None) -> int:
        return len({m.sid for m in self.classes[cid].members if m.sid != exclude_sid and m.task != exclude_task})
