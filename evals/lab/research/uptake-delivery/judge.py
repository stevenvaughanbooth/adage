"""Three follow judges over the calls after a decision unit.

1. `literal`   the runtime's own logged judgement (`followed` / `obligation_met` vs `ignored` / none),
               i.e. `adage_core::pattern::meets`: the directed command plus options only.
2. `learned`   LEARNED similarity: cosine between an embedding of the directed action and of each agent
               action (potion-base-8M static embeddings, or a char n-gram TF-IDF fitted on the agents'
               own commands), with the threshold tau fitted on held-out repositories (leave-one-repo-out).
               No tool names, flags or command strings are written by hand.
3. `instrument` a HAND-WRITTEN measurement instrument (not shippable, used only to score 1 and 2):
               "roughly the same action" = the directive's program part (tokens before the first file
               argument) appears as the start of some shell segment of the agent's command, after
               peeling `cd`, env assignments and a venv `activate` (which makes a later `python` the
               venv's). For read-batch directives: the listed file is read. Disagreements with the
               learned judge are audited by hand (`audit.py`) and the audit overrides it.

Each judge returns the lag (calls from delivery to the first matching call), or None.
"""
from __future__ import annotations

import re
import shlex
import sys
from functools import lru_cache
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # evals/lab
import embed  # noqa: E402

from units import action_text  # noqa: E402

# ---------------------------------------------------------------- instrument (hand-written) --


def _is_file_arg(t: str) -> bool:
    return ("/" in t and "." in t.split("/")[-1] and not t.endswith("/python") and "bin/" not in t) or "::" in t


def _prog(tokens: list[str]) -> list[str]:
    out, i = [], 0
    while i < len(tokens):
        t = tokens[i]
        if t == "-m" and i + 1 < len(tokens):  # `python -m pytest`: the module is the program
            out += tokens[i:i + 2]
            i += 2
            continue
        if _is_file_arg(t) or t.startswith("-"):
            break
        out.append(t)
        i += 1
    return out


def _canon_segments(cmd: str) -> list[list[str]]:
    segs, venv = [], None
    for part in re.split(r"&&|\|\||;|\|", cmd.split("<<")[0]):
        try:
            toks = shlex.split(part)
        except ValueError:
            toks = part.split()
        while toks and (re.match(r"^[A-Z_][A-Z0-9_]*=", toks[0]) or toks[0] in ("timeout", "exec", "env")
                        or re.match(r"^\d+[sm]?$", toks[0])):
            toks = toks[1:]
        if not toks:
            continue
        if toks[0] in ("source", ".") and len(toks) > 1 and toks[1].endswith("/bin/activate"):
            venv = toks[1][: -len("/activate")]
            continue
        if toks[0] == "cd":
            continue
        t0 = toks[0][2:] if toks[0].startswith("./") else toks[0]
        t0 = re.sub(r"python3(\.\d+)?$", "python", t0)
        if venv and t0 in ("python", "pytest"):
            t0 = f"{venv}/{t0}"
        segs.append([t0] + toks[1:])
    return segs


def instrument_match(directed: str, call, cwd: str) -> bool:
    if directed.startswith("Read "):
        f = directed[5:].strip()
        return call.name == "Read" and str(call.inp.get("file_path", "")).replace(cwd + "/", "").endswith(f)
    if call.name != "Bash":
        return False
    d = _canon_segments(directed)
    if not d:
        return False
    want = _prog(d[0])
    cmd = str(call.inp.get("command", "")).replace(cwd + "/", "")
    for seg in _canon_segments(cmd):
        if seg[: len(want)] == want:
            return True
    return False


# ---------------------------------------------------------------- learned similarity --------

_MODELS = {}


def _model(name):
    if name not in _MODELS:
        m = embed.load(name)
        _MODELS[name] = m
    return _MODELS[name]


class Sim:
    """cos(directed, action) for one embedding model; texts cached."""

    def __init__(self, name: str, fit_texts: list[str] | None = None):
        self.name = name
        self.m = embed.load(name)
        if name == "tfidf":
            self.m.fit(fit_texts)
        self.cache = {}

    def vec(self, texts):
        miss = [t for t in set(texts) if t not in self.cache]
        if miss:
            V = self.m.encode(miss)
            if hasattr(V, "toarray"):
                V = V.toarray()
            for t, v in zip(miss, V):
                self.cache[t] = np.asarray(v, np.float32)
        return np.stack([self.cache[t] for t in texts])

    def sims(self, directed: list[str], actions: list[str]) -> np.ndarray:
        """max over directed items, per action."""
        if not actions:
            return np.zeros(0)
        D, A = self.vec(directed), self.vec(actions)
        return (A @ D.T).max(1)


def lags(u, horizon: int | None = None):
    """Calls after the unit: (texts, calls)."""
    calls = u.s.calls[u.pos:] if horizon is None else u.s.calls[u.pos:u.pos + horizon]
    return [action_text(c, u.s.cwd) for c in calls], calls


def instrument_lag(u, horizon=None):
    _, calls = lags(u, horizon)
    for i, c in enumerate(calls):
        if any(instrument_match(d, c, u.s.cwd) for d in u.directed):
            return i
    return None


def sim_profile(u, sim: Sim, horizon=None) -> np.ndarray:
    texts, _ = lags(u, horizon)
    return sim.sims(u.directed, texts) if u.directed else np.zeros(len(texts))


def learned_lag(profile: np.ndarray, tau: float):
    idx = np.nonzero(profile >= tau)[0]
    return int(idx[0]) if len(idx) else None


# ---------------------------------------------------------------- learned token containment --

_TOK = re.compile(r"[A-Za-z0-9_.\-]+")


def tokens(x: str) -> list[str]:
    """Generic lexing (no command semantics): the first line (a heredoc body is data), split on
    anything that is not a word character, dot or dash; `/` separates tokens."""
    first = x.split("\n", 1)[0]
    return [t for t in _TOK.findall(first.replace("/", " ")) if t not in (".", "..")]


class Containment:
    """Learned, weighted containment of the directive's tokens in the agent's action.

    Token weight = log(1 + number of distinct training *tasks* whose actions contain it): tokens
    that recur across tasks are how this repository does things (program, module, subcommand),
    tokens seen in one task are that task's arguments. Fitted on training-split traces only.
    sim(d, a) = sum_{t in d, t in a} w(t) / sum_{t in d} w(t).
    """

    name = "containment"

    def __init__(self, train_actions: list[tuple[str, str]], boost: float = 1.0):
        self.boost = boost  # extra weight on directive tokens absent from the failed trigger call
        by = {}
        for task, text in train_actions:
            for t in set(tokens(text)):
                by.setdefault(t, set()).add(task)
        self.w = {t: float(np.log1p(len(v))) for t, v in by.items()}

    def one(self, d: str, a: str, trigger: str = "") -> float:
        dt = set(tokens(d))
        if not dt:
            return 0.0
        at, tt = set(tokens(a)), set(tokens(trigger))
        floor = float(np.log1p(1))
        ws = {t: max(self.w.get(t, 0.0), floor) * (self.boost if tt and t not in tt else 1.0) for t in dt}
        tot = sum(ws.values())
        return sum(w for t, w in ws.items() if t in at) / tot

    def sims(self, directed: list[str], actions: list[str], trigger: str = "") -> np.ndarray:
        return (np.array([max(self.one(d, a, trigger) for d in directed) for a in actions])
                if actions else np.zeros(0))


def train_actions() -> list[tuple[str, str]]:
    """(task, action text) for every call of every training-split session on disk."""
    import glob
    import json
    import os

    import data
    from units import action_text
    out, seen = [], set()
    for root in data.PRIMARY + data.SECONDARY:
        try:
            st = json.load(open(f"{root}/state.json"))
        except Exception:
            continue
        tr = data._find_transcripts(root)
        for rid, v in st.get("runs", {}).items():
            m = v.get("metrics") or {}
            sid = m.get("session_id")
            if v.get("phase") != "train" or not sid or sid in seen or sid not in tr:
                continue
            seen.add(sid)
            _, cwd, calls, _ = data.parse_transcript(tr[sid], "haiku")
            out += [(v.get("task", rid), action_text(c, cwd)) for c in calls]
    return out
