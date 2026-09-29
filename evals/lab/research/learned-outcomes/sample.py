"""Draw the hand-labelling sample (the measurement instrument, not the method).

280 tool calls from held-out **control-arm** sessions, 40 per repository slice (7 slices: five
Haiku repositories and the two Sonnet 5 slices). Per slice: 26 Bash calls (8 of them with the
harness's `is_error` set) and 14 calls of other tools (3 with `is_error`). Stratifying on the
harness flag (not on any hand-written pattern) keeps enough failures to measure; AUC does not
depend on the class mix, and the prevalence-weighted numbers are reported separately.

`python3 sample.py` writes `labels_todo.jsonl` (ids and the text shown to the labeller);
`python3 sample.py show A B` prints items A..B for labelling.
"""

from __future__ import annotations

import json
import random
import sys
from pathlib import Path

import load

HERE = Path(__file__).resolve().parent
TODO = HERE / "labels_todo.jsonl"
SLICES = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]


def draw(seed: int = 20260929):
    rng = random.Random(seed)
    pools = {}
    for s in load.sessions():
        if s["arm"] != "control":
            continue
        for i, c in enumerate(s["calls"]):
            bash = c["tool"].lower() == "bash"
            pools.setdefault((s["repo"], bash, c["is_error"]), []).append((s["sid"], i, c))
    out = []
    for r in SLICES:
        for bash, err, n in [(True, True, 8), (True, False, 18), (False, True, 3), (False, False, 11)]:
            pool = pools.get((r, bash, err), [])
            for sid, i, c in rng.sample(pool, min(n, len(pool))):
                out.append({"id": f"{sid}:{i}", "repo": r, "tool": c["tool"], "arg": c["arg"][:300],
                            "is_error": c["is_error"], "output": c["output"]})
    rng.shuffle(out)
    with TODO.open("w") as f:
        for k, x in enumerate(out):
            x["n"] = k
            f.write(json.dumps(x) + "\n")
    print(len(out), "items ->", TODO)


def show(a: int, b: int):
    for l in TODO.open():
        x = json.loads(l)
        if a <= x["n"] <= b:
            o = x["output"]
            o = o if len(o) < 700 else o[:450] + "\n[…]\n" + o[-250:]
            print(f"=== #{x['n']} {x['repo']} {x['tool']} err={int(x['is_error'])} :: {x['arg'][:220]!r}")
            print(o.strip()[:700])


if __name__ == "__main__":
    if sys.argv[1:2] == ["show"]:
        show(int(sys.argv[2]), int(sys.argv[3]))
    else:
        draw()
