"""Per-call records for the execution-offload study, built on the common harness corpus.

Each call becomes a Rec: its tool, a normalised *key* (what a hook would have to reproduce exactly
to run the same call itself), the output as the model saw it, and whether it failed.

Normalisation is data preparation, not a taxonomy:
  - the benchmark sandbox root (the session cwd) is stripped from every string argument
    (a benchmark artefact: in a real repository the hook runs in the same cwd);
  - whitespace is collapsed; a leading `cd <cwd> &&` is dropped (same artefact).
No tool names, command names or error strings are listed anywhere in this module.
"""
from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass

sys.path.insert(0, "/home/user/adage/evals/lab/research")
from harness import load  # noqa: E402
from harness import costmodel as CM  # noqa: E402

WS = re.compile(r"\s+")


@dataclass
class Rec:
    i: int
    tool: str
    key: str  # tool + normalised arguments: equality = "the same call"
    args: dict  # normalised arguments
    text: str  # normalised argument text (for Bash: the command)
    out: str  # output as the model saw it
    out_chars: int
    failed: bool
    resp: int  # index of the API response that issued it
    solo: bool  # the only call of its response (removing it removes a model turn)


def _strip(v, cwd):
    if isinstance(v, str):
        if cwd:
            v = v.replace(cwd + "/", "").replace(cwd, ".")
        return v
    if isinstance(v, dict):
        return {k: _strip(x, cwd) for k, x in v.items()}
    if isinstance(v, list):
        return [_strip(x, cwd) for x in v]
    return v


def _norm_cmd(c: str) -> str:
    c = WS.sub(" ", c).strip()
    c = re.sub(r"^cd \. && ", "", c)
    c = re.sub(r"^cd \.$", "", c)
    return c


_REC = {}


def records(s) -> list[Rec]:
    got = _REC.get(s.sid)
    if got is not None:
        return got
    got = _REC[s.sid] = _records(s)
    return got


def _records(s) -> list[Rec]:
    cwd = (s.cwd or "").rstrip("/")
    cm = CM.of(s)
    out = []
    for i, c in enumerate(s.calls):
        inp = c.input if isinstance(c.input, dict) else {"_": str(c.input)}
        args = _strip(inp, cwd)
        args = {k: v for k, v in args.items() if k not in ("description", "timeout", "run_in_background")}
        if "command" in args and isinstance(args["command"], str):
            args["command"] = _norm_cmd(args["command"])
            text = args["command"]
        else:
            text = " ".join(f"{k}={v}" for k, v in sorted(args.items()) if not isinstance(v, (dict, list)))
        key = c.tool + "|" + json.dumps(args, sort_keys=True, default=str)
        j = cm.resp_of.get(i, -1)
        solo = j >= 0 and len(s.responses[j].calls) == 1
        rej = s.acts[i].rejected
        out.append(Rec(i, c.tool, key, args, text, _strip(c.output or "", cwd), max(c.out_chars, len(c.output or "")),
                       bool(c.is_error and not rej), j, solo))
    return out


_CORPUS = None


def corpus():
    global _CORPUS
    if _CORPUS is None:
        _CORPUS = load()
    return _CORPUS


def model_of(s) -> str:
    return "sonnet" if "sonnet" in (s.model or "") else "haiku"
