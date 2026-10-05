#!/usr/bin/env python3
"""How a retrieved-case prediction compiles to Adage's typed delivery shapes, and what the hook
would pay for it.

  ADAGE_LAB_MODELS=<dir> python3 evals/lab/compile.py [--repo click] [--theta 0.5]

1. **Learn time** (`build_table`): every training situation is embedded once and stored with its
   candidate directives: the actions that would have been useful directives there (taken later,
   successfully, after waste). Each candidate is a *typed step*: the normalised command template
   (`python -m pytest {test_file}`) plus, per slot, a value type learned from the concrete values
   that sink took in training (`{test_file}` -> `tests/test_*.py`, closed over the repository's
   files). Only commands some training session ran **successfully**, in at least
   `min_sessions` sessions, are candidates: the table can never emit a command no agent ran.
2. **Hook time** (`lookup`): embed the live situation (one static-model call), cosine-kNN over the
   table's vectors, vote, abstain below the threshold, then pick the shape from the situation:
   after a failure -> `avoid` (not that, this); after an edit and the step is a test run ->
   `obligation`; at the prompt -> `run` (or `read` for a file); otherwise `after`. The slot is
   bound from the session's own recent paths, and only to a value its learned type accepts; when
   nothing binds it yet, the step becomes fixed `advise` text showing the typed placeholder
   (never a guessed value); a slot with no learned type is never delivered.

Prints the table's size, per-lookup latency (embedding + kNN + vote), and example compiled
directives on the held-out sessions.
"""

from __future__ import annotations

import argparse
import collections
import fnmatch
import json
import re
import sys
import time

import numpy as np

import embed
import traces
from predict import Params, useful_future
from situations import situations


def value_type(values: list[str]) -> str | None:
    """A glob over repository paths, from the concrete values a slot took: the longest common
    directory, the file-name shape (`test_*.py`, `*.spec.ts`), or None when there is no common
    shape (then the slot cannot be bound and the step is not delivered)."""
    vals = [v for v in values if v and not v.startswith("/")]
    if not vals:
        return None
    if len(set(vals)) == 1 and len(vals) >= 2 and not re.search(r"\.\w+$", vals[0]):
        return "enum:" + vals[0]  # a directory the agents always used, e.g. `.` (never one task's file)
    dirs = [v.rsplit("/", 1)[0] if "/" in v else "" for v in vals]
    common = dirs[0]
    for d in dirs[1:]:
        while common and not (d == common or d.startswith(common + "/")):
            common = common.rsplit("/", 1)[0] if "/" in common else ""
    names = [v.rsplit("/", 1)[-1] for v in vals]
    for pat in ("test_*.py", "*_test.py", "*_test.go", "*.spec.ts", "*.test.ts", "*.py", "*.go", "*.ts"):
        if all(fnmatch.fnmatch(n, pat) for n in names):
            return (common + "/**/" if common else "**/") + pat
    return None


def slot_values(sessions) -> dict:
    """(directive_key, slot) -> concrete values seen at that sink in successful training runs."""
    out = collections.defaultdict(list)
    for s in sessions:
        for st in s.steps:
            a = st.action
            if st.failed or a.tool != "Bash" or not a.directive_eligible:
                continue
            args = a.full.split(" ")[len(a.key.split(" ")):]
            for v in args:
                for sl in ("{test_file}", "{test_dir}", "{src_file}", "{dir}"):
                    if sl in a.directive_key and _slot_of(v) == sl:
                        out[(a.directive_key, sl)].append(v)
    return out


def _slot_of(v):
    from normalize import slot
    return slot(v)


class Table:
    def __init__(self, repo_sessions, p: Params, model):
        self.p = p
        self.model = model
        self.sits = [s for x in repo_sessions for s in situations(x) if s.i < len(x.steps)]
        t0 = time.perf_counter()
        self.P = model.encode([s.prompt_text for s in self.sits])
        self.H = model.encode([s.hist_text for s in self.sits])
        self.build_ms = (time.perf_counter() - t0) * 1000
        self.cands = [useful_future(s) for s in self.sits]
        self.sess = [s.session.sid for s in self.sits]
        self.types = {k: value_type(v) for k, v in slot_values(repo_sessions).items()}
        self.files = set()

    def bytes(self) -> int:
        return self.P.nbytes + self.H.nbytes + sum(len(json.dumps(c)) for c in self.cands)

    def lookup(self, sit):
        p = self.p
        q = self.model.encode([sit.prompt_text, sit.hist_text])
        sim = p.w_prompt * (self.P @ q[0]) + p.w_hist * (self.H @ q[1])
        k = min(p.k, len(sim))
        idx = np.argpartition(-sim, k - 1)[:k]
        w = np.maximum(sim[idx], 0) ** p.power
        votes, sess = collections.Counter(), collections.defaultdict(set)
        for j, wt in zip(idx, w):
            for key in self.cands[j]:
                votes[key] += wt
                sess[key].add(self.sess[j])
        tot = float(w.sum()) or 1.0
        for key, v in votes.most_common():
            if p.run_only and not key.startswith("Bash:"):
                continue
            if len(sess[key]) >= p.min_sessions:
                return key, v / tot, len(sess[key])
        return None

    def compile(self, sit, key, conf, support):
        """The typed directive, or None when a slot cannot be bound to a value its type accepts."""
        hist = sit.session.steps[:sit.i]
        last = hist[-1] if hist else None
        if key.startswith("Bash:"):
            cmd = key[5:]
            slots = re.findall(r"\{\w+\}", cmd)
            binding, unbound = {}, []
            for sl in slots:
                ty = self.types.get((key, sl))
                recent = [a for h in reversed(hist) for a in re.findall(r"\S+", h.action.full)]
                if ty and ty.startswith("enum:") and len(ty[5:].split("|")) == 1:
                    val = ty[5:]
                else:
                    val = next((v for v in recent if ty and _slot_of(v) == sl and fnmatch.fnmatch(v, ty)), None)
                if val is None:
                    if ty is None:
                        return None  # no learned type: never delivered
                    unbound.append(sl)
                    binding[sl] = {"type": ty, "value": None}
                    continue
                binding[sl] = {"type": ty, "value": val}
            if unbound:
                # Nothing in the session binds the slot yet: fixed advice that shows the typed
                # placeholder, never a guessed value (`advise`: fixed text, validated values).
                shown = cmd
                for sl in unbound:
                    shown = shown.replace(sl, f"<{sl[1:-1]}: {binding[sl]['type']}>")
                return {"shape": "advise", "trigger": {"on": "prompt" if last is None else "PostToolUse"},
                        "step": {"advice": f"In this repository, run `{shown}`", "slots": binding},
                        "strength": "suggest", "evidence": {"confidence": round(float(conf), 3),
                                                            "sessions": int(support)}}
            if last is None:
                shape, trigger = "run", {"on": "prompt"}
            elif last.failed and last.action.directive_key != key:
                shape, trigger = "avoid", {"on": "PostToolUseFailure", "command": last.action.key[5:]}
            elif last.action.edit and any(w in cmd for w in ("pytest", "test")):
                shape, trigger = "obligation", {"on": "edit", "paths": last.action.full.split(" ", 1)[-1]}
            else:
                shape, trigger = "after", {"on": "PostToolUse", "command": last.action.key.removeprefix("Bash:")}
            step = {"run": cmd, "slots": binding}
        elif key.startswith("Read "):
            shape, trigger, step = "read", {"on": "prompt" if last is None else "PostToolUse"}, {"read": [key[5:]]}
        else:
            return None  # no delivery shape says "edit this file"
        return {"shape": shape, "trigger": trigger, "step": step, "strength": "suggest",
                "evidence": {"confidence": round(float(conf), 3), "sessions": int(support)}}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--repo", default="click")
    ap.add_argument("--params", default=str(traces.LAB / "results" / "lab.json"),
                    help="take the chosen kNN parameters and threshold from a lab run")
    ap.add_argument("--theta", type=float)
    a = ap.parse_args(argv)
    try:
        lab = json.loads(open(a.params).read())
        p = Params(**lab["chosen"]["params"])
        th = a.theta or next((lab["chosen"][f"op{t}"]["theta"] for t in (0.7, 0.5) if lab["chosen"][f"op{t}"]),
                             lab["chosen"]["max_precision"]["theta"])
    except Exception:
        p, th = Params(model="potion-8M", target="useful", k=20, run_only=True), a.theta or 0.5
    if p.model == "tfidf" or p.model not in embed.available():
        p.model = "potion-8M"
    S = traces.load([a.repo])[a.repo]
    train = [s for s in S if s.phase == "train"]
    test = [s for s in S if s.phase == "eval" and s.arm == "control"]
    model = embed.load(p.model)
    t = Table(train, p, model)
    print(f"{a.repo}: table of {len(t.sits)} situations, {t.bytes() / 1024:.0f} KiB, built in {t.build_ms:.0f} ms "
          f"({p.model}, k={p.k}, theta={th})")
    print("learned slot types:")
    for (k, sl), ty in sorted(t.types.items()):
        print(f"   {k}   slot {sl}: {ty}")
    lat, shown, delivered, unbound = [], collections.Counter(), 0, 0
    for s in test:
        issued = set()
        for sit in situations(s):
            t0 = time.perf_counter()
            c = t.lookup(sit)
            lat.append((time.perf_counter() - t0) * 1000)
            if not c or c[1] < th or c[0] in issued:
                continue
            issued.add(c[0])
            d = t.compile(sit, *c)
            if d is None:
                unbound += 1
                continue
            delivered += 1
            sig = (d["shape"], json.dumps(d["step"])[:90])
            if shown[sig] < 1 and sum(shown.values()) < 12:
                print(json.dumps(d))
            shown[sig] += 1
    lat.sort()
    print(f"hook lookup (embed 2 texts + kNN over {len(t.sits)} + vote): median {lat[len(lat) // 2]:.2f} ms, "
          f"p95 {lat[int(len(lat) * .95)]:.2f} ms over {len(lat)} decision points")
    print(f"directives: {delivered} delivered, {unbound} withheld (a slot with no learned type)")
    print("shapes:", dict(collections.Counter(k[0] for k in shown.elements())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
